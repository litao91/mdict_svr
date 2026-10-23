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

    /// The headwords immediately before and after `word` in this dictionary's
    /// byte-sorted index, plus whether `word` is itself a headword.
    ///
    /// `/suggest` cannot answer this: it is a forward-only prefix scan, so
    /// "the word before this one" is unreachable from it. Reading the index
    /// position directly is exact and O(log n + window).
    pub fn nearby(&self, word: &str, before: usize, after: usize) -> (bool, Vec<String>, Vec<String>) {
        let key = normalize_key(word, self.case_sensitive, false);
        let (exact, at) = self.qmd.position_of(key.as_bytes());
        let decode = |b: &[u8]| String::from_utf8_lossy(b).into_owned();

        let mut prev = Vec::with_capacity(before);
        let mut i = at;
        while prev.len() < before && i > 0 {
            i -= 1;
            if let Some(k) = self.qmd.key_at(i) {
                prev.push(decode(k));
            }
        }
        prev.reverse();

        let mut next = Vec::with_capacity(after);
        let mut j = if exact { at + 1 } else { at };
        while next.len() < after {
            match self.qmd.key_at(j) {
                Some(k) => next.push(decode(k)),
                None => break,
            }
            j += 1;
        }
        (exact, prev, next)
    }

    /// Headwords matching a glob pattern (`*` any run, `?` one character), in
    /// index order. Scans every key, so it is a fallback for when the user
    /// reaches for wildcards, not an as-you-type path.
    pub fn search_glob(&self, pattern: &str, limit: usize) -> Vec<String> {
        let pat = normalize_key(pattern, self.case_sensitive, false).into_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while out.len() < limit {
            match self.qmd.key_at(i) {
                Some(k) => {
                    if glob_match(&pat, k) {
                        out.push(String::from_utf8_lossy(k).into_owned());
                    }
                }
                None => break,
            }
            i += 1;
        }
        out
    }

    /// Headwords within Levenshtein distance `max_dist` of `word`, closest
    /// first, ties in index order. Also a full-index scan; meant for "did you
    /// mean" after a prefix search came up empty.
    pub fn fuzzy(&self, word: &str, max_dist: u8, limit: usize) -> Vec<String> {
        let target = normalize_key(word, self.case_sensitive, false);
        let t: Vec<char> = target.chars().collect();
        if t.is_empty() || limit == 0 {
            return Vec::new();
        }
        let max = max_dist as usize;
        let mut prev_row: Vec<usize> = (0..=t.len()).collect();
        let mut cur_row = vec![0usize; t.len() + 1];
        let mut hits: Vec<(usize, usize, u64, String)> = Vec::new();
        let mut i = 0u64;
        while let Some(k) = self.qmd.key_at(i) {
            let s: Vec<char> = String::from_utf8_lossy(k).chars().collect();
            // Length alone can put a key out of reach; skip the row work.
            if s.len().abs_diff(t.len()) <= max {
                let dist = levenshtein_bounded(&s, &t, max, &mut prev_row, &mut cur_row);
                if let Some(d) = dist {
                    // A dropped or substituted letter keeps the head of the word,
                    // so shared-prefix length separates "apple" from the other
                    // distance-1 candidates of "aple".
                    let shared = s.iter().zip(t.iter()).take_while(|(a, b)| a == b).count();
                    hits.push((d, shared, i, String::from_utf8_lossy(k).into_owned()));
                }
            }
            i += 1;
        }
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
        hits.truncate(limit);
        hits.into_iter().map(|h| h.3).collect()
    }
}

/// Glob match with `*` (any run, including empty) and `?` (one character),
/// using the classic two-pointer algorithm with a single backtrack point.
fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let (mut ti, mut pi) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star = pi;
            mark = ti;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}

/// Levenshtein distance, or `None` when it must exceed `max`. The row is cut off
/// at `max + 1` on both sides of the diagonal, which keeps a miss cheap: most
/// keys in a 850k-headword index are nowhere near the query.
fn levenshtein_bounded(
    s: &[char],
    t: &[char],
    max: usize,
    prev_row: &mut Vec<usize>,
    cur_row: &mut Vec<usize>,
) -> Option<usize> {
    let inf = max + 1;
    for (j, cell) in prev_row.iter_mut().enumerate().take(t.len() + 1) {
        *cell = j.min(inf);
    }
    for (i, sc) in s.iter().enumerate() {
        cur_row[0] = (i + 1).min(inf);
        let mut row_min = cur_row[0];
        for (j, tc) in t.iter().enumerate() {
            let cost = if sc == tc { 0 } else { 1 };
            let v = (prev_row[j + 1] + 1)
                .min(cur_row[j] + 1)
                .min(prev_row[j] + cost);
            cur_row[j + 1] = v.min(inf);
            row_min = row_min.min(cur_row[j + 1]);
        }
        // Every later cell in this row is at least row_min - 0 away from the
        // final value's floor; a fully-saturated row can never recover.
        if row_min >= inf {
            return None;
        }
        std::mem::swap(prev_row, cur_row);
    }
    let d = prev_row[t.len()];
    (d <= max).then_some(d)
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

    fn words_dict(name: &str) -> (std::path::PathBuf, Dict) {
        let path = tmp(name);
        {
            let mut b = QmdBuilder::create(&path, false, Meta::default()).unwrap();
            for w in [
                "apple", "apple pie", "applet", "apples", "appliance", "apply", "apricot",
            ] {
                b.add(w, b"<p>x</p>", KIND_HTML).unwrap();
            }
            b.finish().unwrap();
        }
        let d = Dict::open(&path).unwrap();
        (path, d)
    }

    #[test]
    fn nearby_reads_the_index_around_a_headword() {
        let (_p, d) = words_dict("nearby");
        // Byte order, not collation: "apples" sorts before "applet".
        let (exact, prev, next) = d.nearby("apples", 2, 2);
        assert!(exact);
        assert_eq!(prev, vec!["apple", "apple pie"]);
        assert_eq!(next, vec!["applet", "appliance"]);

        // The first and last keys clamp instead of wrapping or panicking.
        let (_, prev, next) = d.nearby("apple", 5, 2);
        assert!(prev.is_empty());
        assert_eq!(next, vec!["apple pie", "apples"]);
        let (_, _, next) = d.nearby("apricot", 1, 5);
        assert!(next.is_empty());
    }

    #[test]
    fn nearby_on_a_miss_reports_the_insertion_point() {
        let (_p, d) = words_dict("nearby_miss");
        // "aple" is not a headword; the window is where it would sort.
        let (exact, prev, next) = d.nearby("aple", 2, 2);
        assert!(!exact);
        assert!(prev.is_empty());
        assert_eq!(next, vec!["apple", "apple pie"]);
    }

    #[test]
    fn glob_search_handles_stars_and_questions() {
        let (_p, d) = words_dict("glob");
        // "apples" ends in `s`, so `app*e` skips it; "appliance" ends in `e`.
        assert_eq!(d.search_glob("app*e", 10), vec!["apple", "apple pie", "appliance"]);
        assert_eq!(d.search_glob("appl?", 10), vec!["apple", "apply"]);
        assert_eq!(d.search_glob("*ant", 10), Vec::<String>::new());
        assert!(glob_match(b"*", b"anything"));
        assert!(!glob_match(b"a*c", b"abd"));
    }

    #[test]
    fn fuzzy_ranks_the_intended_word_first() {
        let (_p, d) = words_dict("fuzzy");
        // A dropped letter: distance 1 and the longest shared prefix.
        let hits = d.fuzzy("aple", 2, 3);
        assert_eq!(hits[0], "apple", "the dropped-letter candidate must rank first");
        assert!(hits.iter().all(|h| ["apple", "apples", "applet", "apply", "apple pie"].contains(&h.as_str())));
        assert_eq!(d.fuzzy("apply", 2, 1), vec!["apply"]);
        // Nothing within distance 1 of a nonsense string this far from any key.
        assert!(d.fuzzy("zzzzzz", 1, 5).is_empty());
    }
}
