//! MDict file header parsing.
//!
//! Layout at offset 0:
//!   * `u32 BE` header block size
//!   * header block bytes (UTF-16LE for v2.0; UTF-8/GB18030 for v1.x), an
//!     attribute tag like `<Library_Data GeneratedByEngineVersion="2.0" ... />`
//!   * `u32 LE` adler32 of the header block
//! The keyword section begins immediately after.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use crate::error::{Error, Result};
use crate::mdx::decompress::adler32_of;

/// Text encoding used for keywords (and to decode the header block for v1.x).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf16le,
    Utf8,
    Gb18030,
}

impl Encoding {
    /// Bytes per character unit (2 for UTF-16, 1 otherwise). Used to size the
    /// first/last key fields in the keyword-index block.
    pub fn unit_size(self) -> usize {
        match self {
            Encoding::Utf16le => 2,
            _ => 1,
        }
    }

    pub fn decode(self, bytes: &[u8]) -> String {
        match self {
            Encoding::Utf16le => {
                let units: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&units)
            }
            Encoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            Encoding::Gb18030 => encoding_rs::GB18030.decode(bytes).0.into_owned(),
        }
    }

    fn from_attr(name: &str, version_2: bool) -> Encoding {
        match name.trim() {
            "" => {
                if version_2 {
                    Encoding::Utf16le
                } else {
                    Encoding::Utf8
                }
            }
            s => {
                let u = s.to_ascii_uppercase();
                match u.as_str() {
                    "UTF-16" | "UTF-16LE" | "UTF16" => Encoding::Utf16le,
                    "GBK" | "GB2312" | "GB18030" => Encoding::Gb18030,
                    _ => Encoding::Utf8,
                }
            }
        }
    }
}

/// Parsed MDict header.
#[derive(Debug, Clone)]
pub struct Header {
    /// Engine version, e.g. 2.0.
    pub version: f32,
    /// 8 for v2.0+, 4 for v1.x — the width of every count/offset/size field.
    pub number_width: usize,
    pub key_encoding: Encoding,
    /// Raw `Encrypted` attribute (bit 0x01 = passcode-encrypted keyword header,
    /// bit 0x02 = RIPEMD128-obfuscated keyword index).
    pub encrypted: u8,
    pub key_case_sensitive: bool,
    pub strip_key: bool,
    pub title: String,
    pub description: String,
    pub creation_date: String,
    /// Byte offset of the keyword section (right after the header + checksum).
    pub key_block_offset: u64,
}

impl Header {
    pub fn is_v2(&self) -> bool {
        self.version >= 2.0
    }

    /// True when the keyword index block uses the RIPEMD128 obfuscation.
    pub fn key_index_encrypted(&self) -> bool {
        self.encrypted & 0x02 != 0
    }

    /// True when the 40-byte keyword-section header needs a passcode (Salsa20).
    pub fn keyword_header_passcode(&self) -> bool {
        self.encrypted & 0x01 != 0
    }
}

fn read_u32_be<R: Read>(r: &mut R) -> Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_be_bytes(b))
}

fn read_u32_le<R: Read>(r: &mut R) -> Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

/// Parse `Name="value"` attributes out of the header tag text.
fn parse_attrs(text: &str) -> Vec<(String, String)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        // find '='
        while i < bytes.len() && bytes[i] != b'=' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // attribute name: run of word chars immediately before '='
        let eq = i;
        let mut start = eq;
        while start > 0 {
            let c = bytes[start - 1];
            if c.is_ascii_alphanumeric() || c == b'_' {
                start -= 1;
            } else {
                break;
            }
        }
        i += 1; // skip '='
        // optional quote
        let quoted = i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'');
        let value = if quoted {
            let q = bytes[i];
            i += 1;
            let vstart = i;
            while i < bytes.len() && bytes[i] != q {
                i += 1;
            }
            let v = text[vstart..i].to_string();
            if i < bytes.len() {
                i += 1; // skip closing quote
            }
            v
        } else {
            let vstart = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'/' && bytes[i] != b'>'
            {
                i += 1;
            }
            text[vstart..i].to_string()
        };
        if eq > start {
            out.push((text[start..eq].to_string(), value));
        }
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Read and parse the header from the start of `file`.
pub fn read_header(file: &mut File) -> Result<Header> {
    file.seek(SeekFrom::Start(0))?;
    let header_size = read_u32_be(file)? as usize;
    if header_size == 0 || header_size > 1 << 20 {
        return Err(Error::Mdx(format!("implausible header size {header_size}")));
    }
    let mut block = vec![0u8; header_size];
    file.read_exact(&mut block)?;
    let checksum = read_u32_le(file)?;
    if checksum != 0 && adler32_of(&block) != checksum {
        return Err(Error::Mdx(format!(
            "header adler32 mismatch: expected {checksum:#010x}, got {:#010x}",
            adler32_of(&block)
        )));
    }
    let key_block_offset = file.stream_position()?;

    // Decode the header text. v2.0 is UTF-16LE; detect it by the interleaved NUL
    // bytes of ASCII, else fall back to UTF-8 then GB18030.
    let text = decode_header_text(&block);
    let attrs = parse_attrs(&text);
    let get = |k: &str| -> String {
        attrs
            .iter()
            .find(|(name, _)| name == k)
            .map(|(_, v)| unescape(v))
            .unwrap_or_default()
    };

    let version_str = get("GeneratedByEngineVersion");
    let version: f32 = version_str.parse().unwrap_or(0.0);
    if version == 0.0 {
        return Err(Error::Mdx(
            "missing/invalid GeneratedByEngineVersion (not an MDict file?)".into(),
        ));
    }
    let v2 = version >= 2.0;
    let number_width = if v2 { 8 } else { 4 };

    let key_encoding = Encoding::from_attr(&get("Encoding"), v2);

    let encrypted = match get("Encrypted").as_str() {
        "" | "No" | "no" => 0u8,
        "Yes" | "yes" => 1u8,
        other => other.parse::<u8>().unwrap_or(0),
    };

    Ok(Header {
        version,
        number_width,
        key_encoding,
        encrypted,
        key_case_sensitive: matches!(get("KeyCaseSensitive").as_str(), "Yes" | "yes"),
        strip_key: matches!(get("StripKey").as_str(), "Yes" | "yes"),
        title: get("Title"),
        description: get("Description"),
        creation_date: get("CreationDate"),
        key_block_offset,
    })
}

fn decode_header_text(block: &[u8]) -> String {
    let looks_utf16 = block.len() >= 2 && block[1] == 0 && block[0] == b'<';
    if looks_utf16 {
        // Strip a trailing UTF-16 NUL terminator if present.
        let end = if block.len() >= 2 && block[block.len() - 1] == 0 && block[block.len() - 2] == 0 {
            block.len() - 2
        } else {
            block.len()
        };
        return Encoding::Utf16le.decode(&block[..end]);
    }
    match std::str::from_utf8(block) {
        Ok(s) => s.trim_end_matches('\0').to_string(),
        Err(_) => encoding_rs::GB18030.decode(block).0.into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_attributes() {
        let text = r##"<Library_Data GeneratedByEngineVersion="2.0" Encrypted="2" Encoding="" Title="Hi""##;
        let attrs = parse_attrs(text);
        let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(get("GeneratedByEngineVersion").unwrap(), "2.0");
        assert_eq!(get("Encrypted").unwrap(), "2");
        assert_eq!(get("Encoding").unwrap(), "");
        assert_eq!(get("Title").unwrap(), "Hi");
    }

    #[test]
    fn unescapes_entities() {
        assert_eq!(unescape("a &lt;b&gt; &amp; c"), "a <b> & c");
    }

    #[test]
    fn encoding_from_attr_defaults() {
        assert_eq!(Encoding::from_attr("", true), Encoding::Utf16le);
        assert_eq!(Encoding::from_attr("GBK", true), Encoding::Gb18030);
        assert_eq!(Encoding::from_attr("UTF-8", false), Encoding::Utf8);
    }
}
