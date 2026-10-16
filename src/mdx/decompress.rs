//! Block decompression for MDict.
//!
//! Every key block, record block, and the keyword-index block is prefixed with a
//! 4-byte compression-type tag and a 4-byte big-endian adler32 of the
//! *decompressed* payload, followed by the body:
//!   * `00 00 00 00` — stored (no compression)
//!   * `01 00 00 00` — LZO1X (not yet supported here)
//!   * `02 00 00 00` — zlib
//!
//! The Oxford Primary sample (and virtually all v2.0 dictionaries) use zlib.

use std::io::Read;

use adler32::RollingAdler32;
use flate2::read::ZlibDecoder;

use crate::error::{Error, Result};

pub const TYPE_NONE: [u8; 4] = [0, 0, 0, 0];
pub const TYPE_LZO: [u8; 4] = [1, 0, 0, 0];
pub const TYPE_ZLIB: [u8; 4] = [2, 0, 0, 0];

/// adler32 checksum of `data`.
pub fn adler32_of(data: &[u8]) -> u32 {
    RollingAdler32::from_buffer(data).hash()
}

fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|e| Error::Decompress(format!("zlib: {e}")))?;
    Ok(out)
}

/// Decompress a tagged block (`[type:4][adler:4][body..]`), verifying the
/// embedded adler32 against the decompressed output.
///
/// `expected_len`, when known, is only a capacity hint; the authoritative length
/// comes from the decompressor.
pub fn decode_block(block: &[u8], expected_len: usize) -> Result<Vec<u8>> {
    if block.len() < 8 {
        return Err(Error::Mdx(format!(
            "compressed block too short: {} bytes",
            block.len()
        )));
    }
    let tag: [u8; 4] = block[0..4].try_into().unwrap();
    let adler_expected = u32::from_be_bytes([block[4], block[5], block[6], block[7]]);
    let body = &block[8..];

    let out = match tag {
        TYPE_NONE => body.to_vec(),
        TYPE_ZLIB => zlib_decompress(body)?,
        TYPE_LZO => {
            return Err(Error::Decompress(
                "LZO-compressed block: not supported yet (this dictionary uses LZO; \
                 re-export it with zlib, or enable the LZO backend)"
                    .to_string(),
            ))
        }
        other => {
            return Err(Error::Mdx(format!(
                "unknown block compression tag {other:?}"
            )))
        }
    };

    if adler_expected != 0 && adler32_of(&out) != adler_expected {
        return Err(Error::Mdx(format!(
            "block adler32 mismatch: expected {adler_expected:#010x}, got {:#010x}",
            adler32_of(&out)
        )));
    }
    if expected_len != 0 && out.len() != expected_len {
        return Err(Error::Mdx(format!(
            "block length mismatch: expected {expected_len}, got {}",
            out.len()
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    #[test]
    fn roundtrip_zlib_block() {
        let payload = b"hello mdict world".repeat(8);
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&payload).unwrap();
        let body = enc.finish().unwrap();

        let mut block = Vec::new();
        block.extend_from_slice(&TYPE_ZLIB);
        block.extend_from_slice(&adler32_of(&payload).to_be_bytes());
        block.extend_from_slice(&body);

        let out = decode_block(&block, payload.len()).unwrap();
        assert_eq!(out, payload);
    }

    #[test]
    fn roundtrip_stored_block() {
        let payload = b"raw bytes".to_vec();
        let mut block = Vec::new();
        block.extend_from_slice(&TYPE_NONE);
        block.extend_from_slice(&adler32_of(&payload).to_be_bytes());
        block.extend_from_slice(&payload);
        assert_eq!(decode_block(&block, payload.len()).unwrap(), payload);
    }

    #[test]
    fn bad_checksum_is_rejected() {
        let mut block = Vec::new();
        block.extend_from_slice(&TYPE_NONE);
        block.extend_from_slice(&0xdeadbeefu32.to_be_bytes());
        block.extend_from_slice(b"data");
        assert!(decode_block(&block, 4).is_err());
    }
}
