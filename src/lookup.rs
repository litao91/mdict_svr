//! Word/resource lookup over a `qmd` container, plus the shared key
//! normalization used by both the importer and the query path.

use std::path::Path;

use crate::error::Result;
use crate::qmd::{Meta, Qmd};

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
    pub fn lookup_word(&self, word: &str) -> Vec<&[u8]> {
        let key = normalize_key(word, self.case_sensitive, false);
        self.qmd
            .lookup(key.as_bytes())
            .into_iter()
            .map(|(_, bytes)| bytes)
            .collect()
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
