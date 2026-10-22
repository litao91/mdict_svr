//! Word/resource lookup over a `qmd` container, plus the shared key
//! normalization used by both the importer and the query path.

use std::collections::{HashSet, VecDeque};
use std::path::Path;

use crate::error::Result;
use crate::md::decode_record;
use crate::qmd::{Meta, Qmd};

/// MDict redirect stub. Instead of HTML, the record body is `@@@LINK=<target>`;
/// OALD9 stores 24,298 of these — 29% of the dictionary — for inflected forms
/// and variants, so `'ll` points at `will` and `abased` at `abase`.
const LINK_PREFIX: &str = "@@@LINK=";

/// Keys to visit before giving up on a redirect chain. A real chain is one hop;
/// the cap only exists so a dictionary that points a key at itself terminates.
const MAX_REDIRECTS: usize = 8;

/// Normalize a key for indexing/lookup. The importer and the query path MUST use
/// the same rules, or lookups miss.
///
/// * Trim surrounding whitespace.
/// * Resources: fold `\` to `/`, strip leading `/`, and drop `entry://`/`file://`
///   schemes (MDX HTML references assets these ways).
/// * Case-insensitive dictionaries (and all resources) are lowercased.
pub fn normalize_key(raw: &str, case_sensitive: bool, is_resource: bool) -> String {
    let mut s = raw.trim().to_string();
    if is_resource {
        s = s.replace('\\', "/");
        for prefix in ["entry://", "file://"] {
            if let Some(rest) = s.strip_prefix(prefix) {
                s = rest.to_string();
                break;
            }
        }
        while s.starts_with('/') {
            s.remove(0);
        }
    }
    if is_resource || !case_sensitive {
        s = s.to_lowercase();
    }
    s
}

/// The redirect target of a record, if it is a `@@@LINK=` stub.
fn redirect_target(bytes: &[u8]) -> Option<String> {
    // A stub is a few bytes; every real record is HTML. Rejecting on length and
    // first byte keeps this off the hot path for normal lookups. A UTF-16LE
    // dictionary encodes the prefix as `@\0@\0@\0L\0…`, which `b"@"` also covers.
    if bytes.len() > 256 || !bytes.starts_with(b"@") {
        return None;
    }
    let text = decode_record(bytes);
    let trimmed = text.trim();
    // The caller joins multi-slot records with markup, so only treat this as a
    // stub when the control string is the whole record.
    if trimmed.contains('<') {
        return None;
    }
    let target = trimmed.strip_prefix(LINK_PREFIX)?.trim();
    (!target.is_empty()).then(|| target.to_owned())
}

/// An opened dictionary (or resource store) with query-time normalization.
pub struct Dict {
    qmd: Qmd,
    case_sensitive: bool,
    is_resource: bool,
}

impl Dict {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Dict> {
        let qmd = Qmd::open(path)?;
        let meta = qmd.meta();
        Ok(Dict {
            case_sensitive: meta.case_sensitive,
            is_resource: meta.is_resource,
            qmd,
        })
    }

    pub fn meta(&self) -> Meta {
        self.qmd.meta()
    }
    pub fn is_resource(&self) -> bool {
        self.is_resource
    }
    pub fn num_keys(&self) -> u64 {
        self.qmd.num_keys()
    }

    /// Look up a word; returns the raw record bytes for every matching entry
    /// (usually one; homographs yield several).
    ///
    /// `@@@LINK=` stubs are followed to their target, so an inflected form
    /// returns the definition of the headword it points at. A stub whose target
    /// cannot be resolved is returned as-is rather than dropped.
    pub fn lookup_word(&self, word: &str) -> Vec<&[u8]> {
        let mut out = Vec::new();
        let mut stubs = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        queue.push_back(normalize_key(word, self.case_sensitive, false));

        while let Some(key) = queue.pop_front() {
            // `visited` both breaks a redirect cycle and dedupes a key whose
            // slots all point at the same target (`abased` stores the stub twice).
            if !visited.insert(key.clone()) {
                continue;
            }
            for (_, bytes) in self.qmd.lookup(key.as_bytes()) {
                match redirect_target(bytes) {
                    Some(target) if visited.len() + queue.len() < MAX_REDIRECTS => {
                        stubs.push(bytes);
                        queue.push_back(normalize_key(&target, self.case_sensitive, false));
                    }
                    _ => out.push(bytes),
                }
            }
        }
        // A chain that dead-ends (target absent) or loops would otherwise return
        // nothing at all for a key that is in the dictionary. The stubs still say
        // what the dictionary meant, and `md::html_to_markdown` renders them as
        // `→ target` rather than leaking the control string.
        if out.is_empty() {
            stubs
        } else {
            out
        }
    }

    /// Look up a resource by path; returns its raw bytes.
    pub fn lookup_resource(&self, path: &str) -> Option<&[u8]> {
        let key = normalize_key(path, false, true);
        self.qmd
            .lookup(key.as_bytes())
            .into_iter()
            .next()
            .map(|(_, bytes)| bytes)
    }

    /// Suggest headwords that begin with `partial`, in ascending order, up to
    /// `limit`. The partial input is normalized exactly like a lookup query, so
    /// matching is case-insensitive for case-insensitive dictionaries. Returned
    /// headwords are the stored (normalized) keys — the same form `lookup_word`
    /// expects.
    pub fn suggest(&self, partial: &str, limit: usize) -> Vec<String> {
        let key = normalize_key(partial, self.case_sensitive, false);
        self.qmd
            .keys_with_prefix(key.as_bytes(), limit)
            .into_iter()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qmd::{Meta, QmdBuilder, KIND_HTML};

    #[test]
    fn normalizes_words() {
        assert_eq!(normalize_key("  Apple ", false, false), "apple");
        assert_eq!(normalize_key("Apple", true, false), "Apple");
    }

    #[test]
    fn normalizes_resource_paths() {
        assert_eq!(
            normalize_key(r"entry://\Img\X.PNG", false, true),
            "img/x.png"
        );
        assert_eq!(normalize_key("/img/y.png", false, true), "img/y.png");
        assert_eq!(normalize_key("file:///a/b.css", false, true), "a/b.css");
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("lookup_test_{}_{}.qmd", name, std::process::id()));
        p
    }

    /// OALD9 stores 24,298 of these (29% of the dictionary) for inflected forms
    /// and variants, so `'ll` has to return the `will` entry.
    #[test]
    fn follows_at_link_redirects() {
        let path = tmp("redirect");
        {
            let mut b = QmdBuilder::create(&path, false, Meta::default()).unwrap();
            b.add("will", b"<p>modal verb</p>", KIND_HTML).unwrap();
            // Records are stored NUL-terminated.
            b.add("'ll", b"@@@LINK=will\r\n\x00", KIND_HTML).unwrap();
            b.add("abase", b"<p>to humble</p>", KIND_HTML).unwrap();
            // `abased` stores the same stub in two slots; it must resolve once.
            b.add("abased", b"@@@LINK=abase\r\n\x00", KIND_HTML).unwrap();
            b.add("abased", b"@@@LINK=abase\r\n\x00", KIND_HTML).unwrap();
            b.add("loop_a", b"@@@LINK=loop_b", KIND_HTML).unwrap();
            b.add("loop_b", b"@@@LINK=loop_a", KIND_HTML).unwrap();
            b.add("nowhere", b"@@@LINK=missing", KIND_HTML).unwrap();
            b.finish().unwrap();
        }
        let d = Dict::open(&path).unwrap();
        let text = |v: Vec<&[u8]>| {
            v.iter()
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .collect::<Vec<_>>()
        };

        assert_eq!(text(d.lookup_word("'ll")), ["<p>modal verb</p>"]);
        assert_eq!(text(d.lookup_word("abased")), ["<p>to humble</p>"]);
        // A cycle terminates instead of spinning.
        assert_eq!(text(d.lookup_word("loop_a")), ["@@@LINK=loop_b", "@@@LINK=loop_a"]);
        // A dead end still returns the stub rather than nothing at all.
        assert_eq!(text(d.lookup_word("nowhere")), ["@@@LINK=missing"]);
        // Ordinary lookups are unaffected.
        assert_eq!(text(d.lookup_word("will")), ["<p>modal verb</p>"]);
        assert!(d.lookup_word("missing").is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_long_html_record_is_not_mistaken_for_a_stub() {
        let path = tmp("notastub");
        {
            let mut b = QmdBuilder::create(&path, false, Meta::default()).unwrap();
            let mut big = b"<p>@@@LINK=lookalike".to_vec();
            big.extend(vec![b'x'; 4096]);
            big.extend_from_slice(b"</p>");
            b.add("big", &big, KIND_HTML).unwrap();
            b.finish().unwrap();
        }
        let d = Dict::open(&path).unwrap();
        let got = d.lookup_word("big");
        assert_eq!(got.len(), 1);
        assert!(got[0].starts_with(b"<p>@@@LINK=lookalike"));
        let _ = std::fs::remove_file(&path);
    }
}
