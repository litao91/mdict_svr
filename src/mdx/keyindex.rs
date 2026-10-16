//! MDict keyword section: decode the keyword index into `(record_offset, key)`
//! entries.
//!
//! v2.0 keyword section (all counts/offsets `u64 BE`):
//!   * 40-byte section header: num_key_blocks, num_entries,
//!     key_block_info_decomp_size, key_block_info_size, key_block_size
//!   * `u32 BE` adler32 of those 40 bytes
//!   * key_block_info block (tagged; RIPEMD128-obfuscated when `Encrypted&2`)
//!   * key_block_size bytes of tagged key blocks
//! Each decoded key block is a run of `{ record_offset: u64, key_text: NUL-term }`.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use crate::error::{Error, Result};
use crate::mdx::crypto::decrypt_key_index;
use crate::mdx::decompress::{adler32_of, decode_block};
use crate::mdx::header::Header;

/// One keyword entry: the byte offset of its record in the decompressed record
/// stream, plus the decoded key text.
#[derive(Debug, Clone)]
pub struct KeyEntry {
    pub record_start: u64,
    pub key: String,
}

/// Result of parsing the keyword section.
pub struct KeyIndex {
    pub num_entries: u64,
    pub keys: Vec<KeyEntry>,
    /// File offset where the record section begins.
    pub record_block_offset: u64,
}

fn read_exact(file: &mut File, n: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

/// Big-endian integer of `width` (4 or 8) bytes at `*i`, advancing `*i`.
fn read_num(slice: &[u8], i: &mut usize, width: usize) -> Result<u64> {
    if *i + width > slice.len() {
        return Err(Error::Mdx("keyword index truncated".into()));
    }
    let mut buf = [0u8; 8];
    buf[8 - width..].copy_from_slice(&slice[*i..*i + width]);
    *i += width;
    Ok(u64::from_be_bytes(buf))
}

fn read_u16_be(slice: &[u8], i: &mut usize) -> Result<u64> {
    if *i + 2 > slice.len() {
        return Err(Error::Mdx("keyword index truncated".into()));
    }
    let v = u16::from_be_bytes([slice[*i], slice[*i + 1]]) as u64;
    *i += 2;
    Ok(v)
}

fn read_u8(slice: &[u8], i: &mut usize) -> Result<u64> {
    if *i + 1 > slice.len() {
        return Err(Error::Mdx("keyword index truncated".into()));
    }
    let v = slice[*i] as u64;
    *i += 1;
    Ok(v)
}

pub fn read_key_index(file: &mut File, header: &Header) -> Result<KeyIndex> {
    if header.keyword_header_passcode() {
        return Err(Error::Mdx(
            "keyword header is passcode-encrypted (Encrypted bit 0x01); \
             a registration passcode is required and not supported"
                .into(),
        ));
    }
    let v2 = header.is_v2();
    let nw = header.number_width;

    file.seek(SeekFrom::Start(header.key_block_offset))?;
    let section_header_len = if v2 { 40 } else { 16 };
    let block = read_exact(file, section_header_len)?;

    let mut i = 0usize;
    let num_key_blocks = read_num(&block, &mut i, nw)?;
    let num_entries = read_num(&block, &mut i, nw)?;
    let info_decomp_size = if v2 { read_num(&block, &mut i, nw)? } else { 0 };
    let info_size = read_num(&block, &mut i, nw)?;
    let key_blocks_size = read_num(&block, &mut i, nw)?;

    if v2 {
        let mut adler = [0u8; 4];
        file.read_exact(&mut adler)?;
        let expected = u32::from_be_bytes(adler);
        if expected != 0 && adler32_of(&block) != expected {
            return Err(Error::Mdx(format!(
                "keyword-section adler32 mismatch: expected {expected:#010x}, got {:#010x}",
                adler32_of(&block)
            )));
        }
    }

    // Keyword-index block.
    let mut raw_info = read_exact(file, info_size as usize)?;
    let info_bytes = if v2 {
        if header.key_index_encrypted() {
            decrypt_key_index(&mut raw_info);
        }
        decode_block(&raw_info, info_decomp_size as usize)?
    } else {
        raw_info
    };

    let block_sizes = parse_block_sizes(&info_bytes, header)?;
    if block_sizes.len() as u64 != num_key_blocks {
        return Err(Error::Mdx(format!(
            "key block count mismatch: header {num_key_blocks}, parsed {}",
            block_sizes.len()
        )));
    }

    // Key blocks.
    let key_blocks = read_exact(file, key_blocks_size as usize)?;
    let units = header.key_encoding.unit_size();
    let mut keys: Vec<KeyEntry> = Vec::with_capacity(num_entries as usize);
    let mut off = 0usize;
    for (comp, decomp) in &block_sizes {
        let end = off + *comp as usize;
        if end > key_blocks.len() {
            return Err(Error::Mdx("key block extends past the key-block region".into()));
        }
        let decoded = decode_block(&key_blocks[off..end], *decomp as usize)?;
        split_key_block(&decoded, header, units, &mut keys)?;
        off = end;
    }

    let record_block_offset = file.stream_position()?;
    Ok(KeyIndex {
        num_entries,
        keys,
        record_block_offset,
    })
}

/// Parse the per-key-block `(compressed, decompressed)` sizes, skipping the
/// first/last key previews.
fn parse_block_sizes(info: &[u8], header: &Header) -> Result<Vec<(u64, u64)>> {
    let v2 = header.is_v2();
    let nw = header.number_width;
    let units = header.key_encoding.unit_size();
    // v2: text-length fields are u16 and each key preview carries a 1-unit NUL
    // terminator; v1: u8 lengths, no terminator.
    let (text_term, use_u16) = if v2 { (1usize, true) } else { (0usize, false) };

    let mut out = Vec::new();
    let mut i = 0usize;
    while i < info.len() {
        // entries in this block (unused here)
        let _ = read_num(info, &mut i, nw)?;
        let head = if use_u16 {
            read_u16_be(info, &mut i)?
        } else {
            read_u8(info, &mut i)?
        };
        i += (head as usize + text_term) * units;
        let tail = if use_u16 {
            read_u16_be(info, &mut i)?
        } else {
            read_u8(info, &mut i)?
        };
        i += (tail as usize + text_term) * units;
        let comp = read_num(info, &mut i, nw)?;
        let decomp = read_num(info, &mut i, nw)?;
        if i > info.len() {
            return Err(Error::Mdx("keyword-index block sizes truncated".into()));
        }
        out.push((comp, decomp));
    }
    Ok(out)
}

/// Split a decoded key block into entries: `{ record_offset, key NUL-term }`.
fn split_key_block(
    block: &[u8],
    header: &Header,
    units: usize,
    out: &mut Vec<KeyEntry>,
) -> Result<()> {
    let nw = header.number_width;
    let mut i = 0usize;
    while i < block.len() {
        let record_start = read_num(block, &mut i, nw)?;
        // Scan for the NUL terminator, stepping by the encoding unit size.
        let text_start = i;
        let mut end = text_start;
        while end + units <= block.len() {
            if block[end..end + units].iter().all(|&b| b == 0) {
                break;
            }
            end += units;
        }
        let key_bytes = &block[text_start..end.min(block.len())];
        let mut key = header.key_encoding.decode(key_bytes);
        if header.strip_key {
            key = key.trim().to_string();
        } else {
            // Canonical readers still trim surrounding whitespace.
            key = key.trim().to_string();
        }
        out.push(KeyEntry { record_start, key });
        i = end + units;
    }
    Ok(())
}
