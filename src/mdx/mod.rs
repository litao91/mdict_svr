//! MDict reader: open a `.mdx`/`.mdd`, decode the keyword index, and stream the
//! `(key, record)` pairs.
//!
//! `.mdx` (words → HTML) and `.mdd` (resource path → binary) share one container
//! format, so the same reader serves both. The keyword index is loaded into
//! memory; records are streamed block-by-block so a multi-hundred-megabyte `.mdd`
//! never has to fit in RAM.

pub mod crypto;
pub mod decompress;
pub mod header;
pub mod keyindex;
pub mod lzo;

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use decompress::decode_block;
pub use header::{Encoding, Header};
pub use keyindex::KeyEntry;

/// An opened MDict file with its keyword index decoded.
pub struct Mdx {
    pub header: Header,
    /// Keyword entries, sorted by `record_start` (the order records are laid out).
    pub keys: Vec<KeyEntry>,
    pub num_entries: u64,
    pub record_block_offset: u64,
    path: PathBuf,
}

fn read_be(file: &mut File, width: usize) -> Result<u64> {
    let mut buf = [0u8; 8];
    file.read_exact(&mut buf[8 - width..])?;
    Ok(u64::from_be_bytes(buf))
}

impl Mdx {
    /// Open `path`, parse the header and keyword index.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Mdx> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;
        let header = header::read_header(&mut file)?;
        let index = keyindex::read_key_index(&mut file, &header)?;
        let mut keys = index.keys;
        // Records are laid out in record_start order; sort to be safe.
        keys.sort_by_key(|k| k.record_start);
        Ok(Mdx {
            header,
            keys,
            num_entries: index.num_entries,
            record_block_offset: index.record_block_offset,
            path,
        })
    }

    /// Stream every record, invoking `f(key, data)`.
    ///
    /// Records are located by the keyword entries' byte offsets into the
    /// concatenated decompressed record stream; each record ends where the next
    /// one begins (the last ends at the stream end).
    pub fn visit_records<F>(&self, mut f: F) -> Result<()>
    where
        F: FnMut(&str, &[u8]) -> Result<()>,
    {
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(self.record_block_offset))?;
        let nw = self.header.number_width;

        let num_record_blocks = read_be(&mut file, nw)?;
        let num_entries = read_be(&mut file, nw)?;
        let _info_size = read_be(&mut file, nw)?;
        let _blocks_size = read_be(&mut file, nw)?;
        if num_entries != self.num_entries && self.num_entries != 0 {
            tracing::warn!(
                "record-section entry count {num_entries} != keyword count {}",
                self.num_entries
            );
        }

        let mut infos = Vec::with_capacity(num_record_blocks as usize);
        for _ in 0..num_record_blocks {
            let comp = read_be(&mut file, nw)?;
            let decomp = read_be(&mut file, nw)?;
            infos.push((comp as usize, decomp as usize));
        }

        let mut offset = 0u64; // start of the current block in the record stream
        let mut ki = 0usize; // cursor into self.keys
        for (comp, decomp) in infos {
            let mut cbuf = vec![0u8; comp];
            file.read_exact(&mut cbuf)?;
            let block = decode_block(&cbuf, decomp)?;
            let block_len = block.len() as u64;
            let block_end = offset + block_len;

            while ki < self.keys.len() {
                let start = self.keys[ki].record_start;
                if start < offset {
                    // Should not happen (keys are sorted); skip defensively.
                    ki += 1;
                    continue;
                }
                if start >= block_end {
                    break; // this key lives in a later block
                }
                let end = if ki + 1 < self.keys.len() {
                    self.keys[ki + 1].record_start.min(block_end)
                } else {
                    block_end
                };
                let s = (start - offset) as usize;
                let e = ((end - offset) as usize).min(block.len());
                if s <= e {
                    f(&self.keys[ki].key, &block[s..e])?;
                }
                ki += 1;
            }
            offset = block_end;
        }

        if ki != self.keys.len() {
            return Err(Error::Mdx(format!(
                "only {ki} of {} keyword records were found in the record blocks",
                self.keys.len()
            )));
        }
        Ok(())
    }
}
