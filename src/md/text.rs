//! Decoding stored record bytes into text.

/// Decode a stored record's bytes into text.
///
/// MDict dictionaries vary: modern v2.0 dictionaries store UTF-8 HTML, while many
/// v1.x dictionaries (e.g. Collins COBUILD) store UTF-16LE. Rather than trust the
/// header, sniff the bytes: an explicit BOM wins, otherwise a run of ASCII-with-
/// NUL-high-bytes indicates UTF-16LE. Falls back to UTF-8.
///
/// Every record in the corpus is padded with a trailing NUL, so that is stripped;
/// left in place the HTML parser turns it into a U+FFFD at the end of each entry.
pub fn decode_record(bytes: &[u8]) -> String {
    let s = if bytes.starts_with(&[0xFF, 0xFE]) {
        utf16le(&bytes[2..])
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        utf16be(&bytes[2..])
    } else if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else if looks_like_utf16le(bytes) {
        utf16le(bytes)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    trim_nul(s)
}

/// Drop trailing NUL padding. Records are stored NUL-terminated, and a UTF-16
/// record decodes to a trailing `'\0'` character just the same.
pub fn trim_nul(mut s: String) -> String {
    while s.ends_with('\0') {
        s.pop();
    }
    s
}

/// True when the first code units look like ASCII encoded as UTF-16LE, i.e. most
/// high bytes are zero. HTML records start with markup/whitespace, so this is a
/// reliable discriminator against UTF-8 (which has no interleaved NULs).
fn looks_like_utf16le(bytes: &[u8]) -> bool {
    let units = (bytes.len() / 2).min(16);
    if units == 0 {
        return false;
    }
    let zeros = (0..units).filter(|&i| bytes[2 * i + 1] == 0).count();
    zeros * 4 >= units * 3
}

fn utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn utf16be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_record() {
        assert_eq!(decode_record("<b>hi</b>".as_bytes()), "<b>hi</b>");
    }

    #[test]
    fn decodes_utf16le_record() {
        let utf16: Vec<u8> = "<b>hi</b>".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode_record(&utf16), "<b>hi</b>");
    }

    #[test]
    fn decodes_bom_prefixed_records() {
        let mut le = vec![0xFF, 0xFE];
        le.extend("x".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode_record(&le), "x");

        let mut be = vec![0xFE, 0xFF];
        be.extend("x".encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(decode_record(&be), "x");

        let mut u8bom = vec![0xEF, 0xBB, 0xBF];
        u8bom.extend_from_slice(b"y");
        assert_eq!(decode_record(&u8bom), "y");
    }

    /// Every record in the corpus ends "\r\n\x00"; the NUL must not reach the
    /// converter, where html5ever would turn it into U+FFFD.
    #[test]
    fn strips_trailing_nul() {
        assert_eq!(decode_record(b"<br>\r\n\x00"), "<br>\r\n");
        assert_eq!(decode_record(b"@@@LINK=will\r\n\x00"), "@@@LINK=will\r\n");
        // A UTF-16LE record's NUL is two bytes and decodes to '\0' all the same.
        let mut u16: Vec<u8> = "x".encode_utf16().flat_map(u16::to_le_bytes).collect();
        u16.extend_from_slice(&[0, 0]);
        assert_eq!(decode_record(&u16), "x");
    }

    #[test]
    fn strips_only_trailing_nuls() {
        // Long enough and HTML-shaped enough that the UTF-16LE sniff stays off.
        assert_eq!(decode_record(b"<p>a\x00b</p>\x00"), "<p>a\x00b</p>");
    }
}
