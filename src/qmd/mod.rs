//! The `qmd` packed container: an immutable, mmap'd, binary-searchable
//! key → record(s) store.
//!
//! Modeled on qingjian's `Dictionary` (two byte arenas + a key-sorted index
//! table + a parallel slot table) but with **64-bit offsets and lengths** so a
//! single record can be arbitrarily large (an HTML page, an image) and the arena
//! can exceed 4 GiB. Everything is little-endian and read with plain slice
//! indexing — no `unsafe`, no alignment requirements.
//!
//! File layout:
//! ```text
//! [0..128)   header (magic "QMD1", counts, section offsets/lengths)
//! TEXT arena   record payloads, concatenated (written first, streamed)
//! KEYS arena   normalized keys, concatenated, sorted ascending
//! INDX         KeyIndex[24] * num_keys, sorted by key bytes
//! SLOT         Slot[24] * num_slots, grouped to match INDX order
//! META         JSON metadata blob
//! ```
//! `KeyIndex { key_start:u64, first_slot:u64, key_len:u32, slot_count:u32 }`
//! `Slot     { text_start:u64, text_len:u64, kind:u32, _pad:u32 }`

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use memmap2::Mmap;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const MAGIC: &[u8; 4] = b"QMD1";
pub const FORMAT_VERSION: u32 = 1;
pub const HEADER_LEN: u64 = 128;
pub const KEYINDEX_LEN: usize = 24;
pub const SLOT_LEN: usize = 24;

pub const FLAG_RESOURCE: u32 = 1 << 0;

/// Record kinds stored in `Slot::kind`.
pub const KIND_HTML: u32 = 0;
pub const KIND_RESOURCE: u32 = 1;

fn align8(x: u64) -> u64 {
    (x + 7) & !7
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// Metadata carried from the source MDict header into the container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub creation_date: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub is_resource: bool,
    /// Whether keys are case-sensitive. Lookups must normalize the same way the
    /// build did, so this is persisted here.
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub num_keys: u64,
    #[serde(default)]
    pub num_slots: u64,
}

impl Default for Meta {
    fn default() -> Self {
        Meta {
            title: String::new(),
            description: String::new(),
            creation_date: String::new(),
            source: String::new(),
            is_resource: false,
            case_sensitive: false,
            num_keys: 0,
            num_slots: 0,
        }
    }
}

#[derive(Debug)]
struct Entry {
    key: String,
    text_start: u64,
    text_len: u64,
    kind: u32,
}

/// Builds a `qmd` file. Record payloads are streamed straight to disk; only the
/// small `(key, offset, len)` metadata is held in memory, so a large `.mdd`
/// imports without loading it all into RAM.
pub struct QmdBuilder {
    file: File,
    entries: Vec<Entry>,
    cursor: u64,
    is_resource: bool,
    meta: Meta,
}

/// Counts returned by [`QmdBuilder::finish`].
#[derive(Debug, Clone, Copy)]
pub struct BuildStats {
    pub num_keys: u64,
    pub num_slots: u64,
    pub text_bytes: u64,
}

impl QmdBuilder {
    pub fn create<P: AsRef<Path>>(path: P, is_resource: bool, mut meta: Meta) -> Result<Self> {
        meta.is_resource = is_resource;
        let mut file = File::create(path)?;
        // Reserve the header; it is written last, once offsets are known.
        file.set_len(HEADER_LEN)?;
        file.seek(SeekFrom::Start(HEADER_LEN))?;
        Ok(Self {
            file,
            entries: Vec::new(),
            cursor: HEADER_LEN,
            is_resource,
            meta,
        })
    }

    /// Append one record. `key` must already be normalized (see
    /// [`crate::lookup::normalize`]).
    pub fn add(&mut self, key: &str, data: &[u8], kind: u32) -> Result<()> {
        let text_start = self.cursor - HEADER_LEN;
        self.file.seek(SeekFrom::Start(self.cursor))?;
        self.file.write_all(data)?;
        self.cursor += data.len() as u64;
        self.entries.push(Entry {
            key: key.to_string(),
            text_start,
            text_len: data.len() as u64,
            kind,
        });
        Ok(())
    }

    pub fn finish(mut self) -> Result<BuildStats> {
        // Sort by key bytes; equal keys stay grouped (stable) so their slots are
        // contiguous and one KeyIndex covers them all.
        self.entries.sort_by(|a, b| a.key.as_bytes().cmp(b.key.as_bytes()));

        // Pad the TEXT arena to an 8-byte boundary.
        let text_len = self.cursor - HEADER_LEN;
        let mut cursor = align8(self.cursor);
        let pad = cursor - self.cursor;
        if pad > 0 {
            self.file.seek(SeekFrom::Start(self.cursor))?;
            self.file.write_all(&vec![0u8; pad as usize])?;
        }

        // KEYS arena: unique keys in sorted order.
        let keys_off = cursor;
        let mut keys_arena: Vec<u8> = Vec::new();
        let mut index: Vec<(u64, u32, u64, u32)> = Vec::new(); // (key_start,key_len,first_slot,slot_count)
        let mut slots: Vec<(u64, u64, u32)> = Vec::with_capacity(self.entries.len()); // (start,len,kind)

        let mut i = 0usize;
        while i < self.entries.len() {
            let key = self.entries[i].key.as_bytes();
            let key_start = keys_arena.len() as u64;
            keys_arena.extend_from_slice(key);
            let key_len = key.len() as u32;
            let first_slot = slots.len() as u64;
            let mut count = 0u32;
            while i < self.entries.len() && self.entries[i].key.as_bytes() == key {
                let e = &self.entries[i];
                slots.push((e.text_start, e.text_len, e.kind));
                count += 1;
                i += 1;
            }
            index.push((key_start, key_len, first_slot, count));
        }

        self.file.seek(SeekFrom::Start(keys_off))?;
        self.file.write_all(&keys_arena)?;
        cursor += keys_arena.len() as u64;
        cursor = align8(cursor);

        // INDX
        let index_off = cursor;
        let mut index_bytes = Vec::with_capacity(index.len() * KEYINDEX_LEN);
        for (key_start, key_len, first_slot, slot_count) in &index {
            index_bytes.extend_from_slice(&key_start.to_le_bytes());
            index_bytes.extend_from_slice(&first_slot.to_le_bytes());
            index_bytes.extend_from_slice(&key_len.to_le_bytes());
            index_bytes.extend_from_slice(&slot_count.to_le_bytes());
        }
        self.file.seek(SeekFrom::Start(index_off))?;
        self.file.write_all(&index_bytes)?;
        cursor = align8(index_off + index_bytes.len() as u64);

        // SLOT
        let slot_off = cursor;
        let mut slot_bytes = Vec::with_capacity(slots.len() * SLOT_LEN);
        for (start, len, kind) in &slots {
            slot_bytes.extend_from_slice(&start.to_le_bytes());
            slot_bytes.extend_from_slice(&len.to_le_bytes());
            slot_bytes.extend_from_slice(&kind.to_le_bytes());
            slot_bytes.extend_from_slice(&0u32.to_le_bytes());
        }
        self.file.seek(SeekFrom::Start(slot_off))?;
        self.file.write_all(&slot_bytes)?;
        cursor = align8(slot_off + slot_bytes.len() as u64);

        // META
        self.meta.num_keys = index.len() as u64;
        self.meta.num_slots = slots.len() as u64;
        let meta_bytes = serde_json::to_vec(&self.meta).map_err(|e| Error::Other(e.to_string()))?;
        let meta_off = cursor;
        self.file.seek(SeekFrom::Start(meta_off))?;
        self.file.write_all(&meta_bytes)?;
        let meta_len = meta_bytes.len() as u64;

        // Header
        let mut flags = 0u32;
        if self.is_resource {
            flags |= FLAG_RESOURCE;
        }
        let mut hdr = Vec::with_capacity(HEADER_LEN as usize);
        hdr.extend_from_slice(MAGIC);
        hdr.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        hdr.extend_from_slice(&flags.to_le_bytes());
        hdr.extend_from_slice(&0u32.to_le_bytes()); // pad
        hdr.extend_from_slice(&(index.len() as u64).to_le_bytes()); // num_keys
        hdr.extend_from_slice(&(slots.len() as u64).to_le_bytes()); // num_slots
        for (off, len) in [
            (keys_off, keys_arena.len() as u64),
            (HEADER_LEN, text_len),
            (index_off, index_bytes.len() as u64),
            (slot_off, slot_bytes.len() as u64),
            (meta_off, meta_len),
        ] {
            hdr.extend_from_slice(&off.to_le_bytes());
            hdr.extend_from_slice(&len.to_le_bytes());
        }
        hdr.resize(HEADER_LEN as usize, 0);
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&hdr)?;
        self.file.flush()?;

        Ok(BuildStats {
            num_keys: index.len() as u64,
            num_slots: slots.len() as u64,
            text_bytes: text_len,
        })
    }
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)] // length fields document the layout; offsets are what we index by
struct Sections {
    num_keys: u64,
    num_slots: u64,
    keys_off: u64,
    keys_len: u64,
    text_off: u64,
    text_len: u64,
    index_off: u64,
    index_len: u64,
    slot_off: u64,
    slot_len: u64,
    meta_off: u64,
    meta_len: u64,
    flags: u32,
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap())
}
fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

/// An opened, memory-mapped `qmd` container.
pub struct Qmd {
    mmap: Mmap,
    s: Sections,
}

impl Qmd {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Qmd> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file) }.map_err(|e| Error::Io(e))?;
        if mmap.len() < HEADER_LEN as usize {
            return Err(Error::Qmd("file smaller than header".into()));
        }
        if &mmap[0..4] != MAGIC {
            return Err(Error::Qmd("bad magic (not a qmd file)".into()));
        }
        let version = u32_at(&mmap, 4);
        if version != FORMAT_VERSION {
            return Err(Error::Qmd(format!("unsupported qmd version {version}")));
        }
        let flags = u32_at(&mmap, 8);
        let num_keys = u64_at(&mmap, 16);
        let num_slots = u64_at(&mmap, 24);
        let mut o = 32usize;
        let next = |m: &Mmap, o: &mut usize| {
            let off = u64_at(m, *o);
            let len = u64_at(m, *o + 8);
            *o += 16;
            (off, len)
        };
        let (keys_off, keys_len) = next(&mmap, &mut o);
        let (text_off, text_len) = next(&mmap, &mut o);
        let (index_off, index_len) = next(&mmap, &mut o);
        let (slot_off, slot_len) = next(&mmap, &mut o);
        let (meta_off, meta_len) = next(&mmap, &mut o);

        // Bounds-check every section once, up front.
        let total = mmap.len() as u64;
        for (name, off, len) in [
            ("keys", keys_off, keys_len),
            ("text", text_off, text_len),
            ("index", index_off, index_len),
            ("slot", slot_off, slot_len),
            ("meta", meta_off, meta_len),
        ] {
            if off.checked_add(len).map_or(true, |end| end > total) {
                return Err(Error::Qmd(format!("{name} section out of bounds")));
            }
        }
        if index_len != num_keys * KEYINDEX_LEN as u64 {
            return Err(Error::Qmd("index length inconsistent with num_keys".into()));
        }
        if slot_len != num_slots * SLOT_LEN as u64 {
            return Err(Error::Qmd("slot length inconsistent with num_slots".into()));
        }

        Ok(Qmd {
            mmap,
            s: Sections {
                num_keys,
                num_slots,
                keys_off,
                keys_len,
                text_off,
                text_len,
                index_off,
                index_len,
                slot_off,
                slot_len,
                meta_off,
                meta_len,
                flags,
            },
        })
    }

    pub fn is_resource(&self) -> bool {
        self.s.flags & FLAG_RESOURCE != 0
    }
    pub fn num_keys(&self) -> u64 {
        self.s.num_keys
    }
    pub fn num_slots(&self) -> u64 {
        self.s.num_slots
    }

    pub fn meta(&self) -> Meta {
        let bytes = &self.mmap[self.s.meta_off as usize..(self.s.meta_off + self.s.meta_len) as usize];
        serde_json::from_slice(bytes).unwrap_or_else(|_| Meta {
            title: Default::default(),
            description: Default::default(),
            creation_date: Default::default(),
            source: Default::default(),
            is_resource: self.is_resource(),
            case_sensitive: false,
            num_keys: self.s.num_keys,
            num_slots: self.s.num_slots,
        })
    }

    fn key_index(&self, i: u64) -> (u64, u64, u32, u32) {
        let base = self.s.index_off as usize + (i as usize) * KEYINDEX_LEN;
        let key_start = u64_at(&self.mmap, base);
        let first_slot = u64_at(&self.mmap, base + 8);
        let key_len = u32_at(&self.mmap, base + 16);
        let slot_count = u32_at(&self.mmap, base + 20);
        (key_start, first_slot, key_len, slot_count)
    }

    fn key_bytes(&self, i: u64) -> &[u8] {
        let (key_start, _, key_len, _) = self.key_index(i);
        let start = self.s.keys_off as usize + key_start as usize;
        &self.mmap[start..start + key_len as usize]
    }

    fn slot(&self, j: u64) -> (u64, u64, u32) {
        let base = self.s.slot_off as usize + (j as usize) * SLOT_LEN;
        let text_start = u64_at(&self.mmap, base);
        let text_len = u64_at(&self.mmap, base + 8);
        let kind = u32_at(&self.mmap, base + 16);
        (text_start, text_len, kind)
    }

    /// Slice a record's bytes out of the TEXT arena.
    pub fn record_bytes(&self, text_start: u64, text_len: u64) -> &[u8] {
        let start = self.s.text_off as usize + text_start as usize;
        let end = start + text_len as usize;
        &self.mmap[start.min(self.mmap.len())..end.min(self.mmap.len())]
    }

    /// Binary search for `norm_key`; on hit return `(first_slot, slot_count)`.
    pub fn find(&self, norm_key: &[u8]) -> Option<(u64, u32)> {
        let n = self.s.num_keys;
        let (mut lo, mut hi) = (0u64, n);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.key_bytes(mid).cmp(norm_key) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => {
                    let (_, first_slot, _, slot_count) = self.key_index(mid);
                    return Some((first_slot, slot_count));
                }
            }
        }
        None
    }

    /// All records for `norm_key` as `(kind, bytes)`.
    pub fn lookup(&self, norm_key: &[u8]) -> Vec<(u32, &[u8])> {
        match self.find(norm_key) {
            None => Vec::new(),
            Some((first_slot, slot_count)) => (0..slot_count as u64)
                .map(|k| {
                    let (start, len, kind) = self.slot(first_slot + k);
                    (kind, self.record_bytes(start, len))
                })
                .collect(),
        }
    }

    /// Index of the first key that sorts >= `prefix` (`num_keys` if none). Since
    /// the index is byte-sorted, every key starting with `prefix` is contiguous
    /// from here — this is the lower bound of the prefix range.
    fn lower_bound(&self, prefix: &[u8]) -> u64 {
        let (mut lo, mut hi) = (0u64, self.s.num_keys);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.key_bytes(mid) < prefix {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// Collect up to `limit` keys that begin with `prefix`, in ascending order.
    /// Cost is O(log n) to find the range plus O(limit) to walk it, independent
    /// of record sizes. An empty `prefix` yields the alphabetically first keys.
    pub fn keys_with_prefix(&self, prefix: &[u8], limit: usize) -> Vec<&[u8]> {
        let mut out = Vec::new();
        if limit == 0 {
            return out;
        }
        let mut i = self.lower_bound(prefix);
        while i < self.s.num_keys && out.len() < limit {
            let k = self.key_bytes(i);
            if !k.starts_with(prefix) {
                break;
            }
            out.push(k);
            i += 1;
        }
        out
    }

    /// The stored key at index `i` in byte-sorted order, if it exists.
    pub fn key_at(&self, i: u64) -> Option<&[u8]> {
        (i < self.s.num_keys).then(|| self.key_bytes(i))
    }

    /// Whether `norm_key` is a stored key, and the index of the first key that
    /// sorts >= it either way. The index is what makes "the words around this
    /// one" an O(log n + window) read instead of a prefix scan.
    pub fn position_of(&self, norm_key: &[u8]) -> (bool, u64) {
        let i = self.lower_bound(norm_key);
        let exact = i < self.s.num_keys && self.key_bytes(i) == norm_key;
        (exact, i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("qmd_test_{}_{}.qmd", name, std::process::id()));
        p
    }

    #[test]
    fn roundtrip_and_lookup() {
        let path = tmp("basic");
        {
            let mut b = QmdBuilder::create(&path, false, Meta {
                title: "t".into(),
                ..Default::default()
            })
            .unwrap();
            b.add("apple", b"<p>fruit</p>", KIND_HTML).unwrap();
            b.add("banana", b"<p>yellow</p>", KIND_HTML).unwrap();
            // duplicate key -> two slots
            b.add("apple", b"<p>a variety</p>", KIND_HTML).unwrap();
            let stats = b.finish().unwrap();
            assert_eq!(stats.num_keys, 2);
            assert_eq!(stats.num_slots, 3);
        }
        let q = Qmd::open(&path).unwrap();
        assert_eq!(q.num_keys(), 2);
        assert!(!q.is_resource());
        let apple = q.lookup(b"apple");
        assert_eq!(apple.len(), 2);
        assert_eq!(apple[0].1, b"<p>fruit</p>");
        assert_eq!(apple[1].1, b"<p>a variety</p>");
        assert_eq!(q.lookup(b"banana")[0].1, b"<p>yellow</p>");
        assert!(q.lookup(b"cherry").is_empty());
        assert_eq!(q.meta().title, "t");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn large_record_over_64kib() {
        // Proves the widened (u64) length/offset fields: qingjian's u16 text_len
        // would overflow here.
        let path = tmp("large");
        let big = vec![b'x'; 200_000]; // > 64 KiB
        {
            let mut b = QmdBuilder::create(&path, false, Meta::default()).unwrap();
            b.add("big", &big, KIND_HTML).unwrap();
            b.finish().unwrap();
        }
        let q = Qmd::open(&path).unwrap();
        let rec = q.lookup(b"big");
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].1.len(), 200_000);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn resource_flag_and_binary_payload() {
        let path = tmp("res");
        let bytes: Vec<u8> = (0..256).map(|i| i as u8).collect();
        {
            let mut b = QmdBuilder::create(&path, true, Meta::default()).unwrap();
            b.add("img/x.png", &bytes, KIND_RESOURCE).unwrap();
            b.finish().unwrap();
        }
        let q = Qmd::open(&path).unwrap();
        assert!(q.is_resource());
        assert_eq!(q.lookup(b"img/x.png")[0].1, &bytes[..]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn prefix_scan() {
        let path = tmp("prefix");
        {
            let mut b = QmdBuilder::create(&path, false, Meta::default()).unwrap();
            for w in ["apple", "app", "apply", "banana", "band", "bandage", "cat"] {
                b.add(w, b"<p/>", KIND_HTML).unwrap();
            }
            b.finish().unwrap();
        }
        let q = Qmd::open(&path).unwrap();
        let got = |p: &[u8], n: usize| -> Vec<String> {
            q.keys_with_prefix(p, n)
                .into_iter()
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .collect()
        };
        // Sorted ascending; exact-prefix matches only.
        assert_eq!(got(b"app", 10), ["app", "apple", "apply"]);
        assert_eq!(got(b"ban", 10), ["banana", "band", "bandage"]);
        // limit truncates
        assert_eq!(got(b"ba", 2), ["banana", "band"]);
        // no match -> empty
        assert!(got(b"zebra", 10).is_empty());
        // empty prefix -> alphabetically first keys
        assert_eq!(got(b"", 2), ["app", "apple"]);
        let _ = std::fs::remove_file(&path);
    }
}
