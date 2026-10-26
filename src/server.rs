//! HTTP API (axum): serve lookups across one or more dictionaries, returning
//! definitions as HTML or Markdown, plus per-dictionary resources and metadata.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, Request, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

use crate::lookup::Dict;
use crate::md::{decode_record, html_to_markdown};

/// One loaded dictionary plus its optional resource store.
pub struct DictEntry {
    pub name: String,
    pub dict: Arc<Dict>,
    pub res: Option<Arc<Dict>>,
}

/// A named collection of dictionaries. Lookups without an explicit `dict` search
/// all of them in registration order.
pub struct Registry {
    pub entries: Vec<DictEntry>,
    by_name: HashMap<String, usize>,
}

impl Registry {
    pub fn new(entries: Vec<DictEntry>) -> Registry {
        let mut by_name = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            by_name.entry(e.name.clone()).or_insert(i);
        }
        Registry { entries, by_name }
    }

    pub fn get(&self, name: &str) -> Option<&DictEntry> {
        self.by_name.get(name).map(|&i| &self.entries[i])
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Shared server state.
pub struct AppState {
    pub registry: Arc<Registry>,
    /// Built web UI to serve alongside the API (`serve --web`). `None` leaves
    /// routing exactly as it was: `/` falls through to the resource lookup.
    pub web: Option<crate::web::WebRoot>,
    /// External decoder behind `/transcode`, for codecs no browser can play
    /// (Ogg-Speex `.spx`). `None` makes the route answer 501 and the UI keeps
    /// offering the file as a download instead.
    pub ffmpeg: Option<PathBuf>,
    /// Transcoded clips keyed by `dict\u0000path`. A Speex clip is a few KB in
    /// and ~100 KB of WAV out, so caching beats re-decoding on every replay.
    transcode_cache: Mutex<HashMap<String, Arc<Vec<u8>>>>,
    /// Markdown conversions keyed by `dict\u0000word`. The databases are
    /// immutable and the conversion is the slowest thing this server does
    /// (~150 ms for a 237 KB entry), so a hit is pure win.
    md_cache: Mutex<HashMap<String, Arc<String>>>,
    /// Gzipped bytes for immutable assets, keyed by path: compressing a
    /// 400 KB bundle costs ~10 ms per request otherwise.
    gz_cache: Mutex<HashMap<String, Arc<Vec<u8>>>>,
}

impl AppState {
    pub fn new(
        registry: Arc<Registry>,
        web: Option<crate::web::WebRoot>,
        ffmpeg: Option<PathBuf>,
    ) -> Self {
        Self {
            registry,
            web,
            ffmpeg,
            transcode_cache: Mutex::new(HashMap::new()),
            md_cache: Mutex::new(HashMap::new()),
            gz_cache: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct LookupParams {
    pub word: Option<String>,
    pub format: Option<String>,
    /// Restrict the lookup to one dictionary by name; omit to search all.
    pub dict: Option<String>,
}

pub fn router(state: Arc<AppState>) -> Router {
    let router = Router::new()
        .route("/health", get(health))
        .route("/info", get(info))
        .route("/dicts", get(dicts))
        .route("/lookup", get(lookup))
        .route("/word/{word}", get(lookup_path))
        .route("/suggest", get(suggest))
        .route("/nearby", get(nearby))
        .route("/res/{dict}/{*path}", get(resource))
        // Always registered: with no `--ffmpeg` it answers 501, which is how
        // the UI learns there is nothing to convert with.
        .route("/transcode/{*path}", get(transcode));
    // Only with `--web`. The SPA holds its state in the query string and has no
    // path routes, so these three cannot collide with the dictionary asset paths
    // the fallback below serves.
    let router = if state.web.is_some() {
        router
            .route("/", get(crate::web::index))
            .route("/index.html", get(crate::web::index))
            .route("/assets/{*path}", get(crate::web::asset))
    } else {
        router
    };
    router
        .fallback(fallback_resource)
        .layer(middleware::from_fn_with_state(Arc::clone(&state), compress))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

fn dict_json(e: &DictEntry) -> serde_json::Value {
    let m = e.dict.meta();
    json!({
        "name": e.name,
        "title": m.title,
        "description": m.description,
        "creation_date": m.creation_date,
        "source": m.source,
        "entries": m.num_keys,
        "records": m.num_slots,
        "case_sensitive": m.case_sensitive,
        "has_resources": e.res.is_some(),
    })
}

async fn dicts(State(st): State<Arc<AppState>>) -> Response {
    let list: Vec<_> = st.registry.entries.iter().map(dict_json).collect();
    Json(json!({
        "dictionaries": list,
        // Lets the UI offer "play" for codecs it cannot decode itself.
        "transcode": st.ffmpeg.is_some(),
    }))
    .into_response()
}

async fn info(State(st): State<Arc<AppState>>) -> Response {
    dicts(State(st)).await
}

/// Output format for a definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Html,
    Markdown,
    Json,
}

/// Decide the output format from the `format` param, falling back to `Accept`.
/// `format` accepts `html`, `md`/`markdown`, or `json`; the `Accept` header is
/// only honored for Markdown and JSON (HTML is the default).
fn parse_format(fmt: Option<&str>, headers: &HeaderMap) -> Format {
    if let Some(f) = fmt {
        let f = f.trim().to_ascii_lowercase();
        return match f.as_str() {
            "md" | "markdown" | "text/markdown" | "text/x-markdown" => Format::Markdown,
            "json" | "application/json" => Format::Json,
            _ => Format::Html,
        };
    }
    if let Some(acc) = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()) {
        let acc = acc.to_ascii_lowercase();
        if acc.contains("text/markdown") || acc.contains("text/x-markdown") {
            return Format::Markdown;
        }
        if acc.contains("application/json") {
            return Format::Json;
        }
    }
    Format::Html
}

/// True for a real top-level browser navigation. `curl`, `fetch()` and XHR never
/// send `Sec-Fetch-Dest`, so programmatic clients are unaffected by the redirect
/// below.
fn is_browser_navigation(headers: &HeaderMap) -> bool {
    headers
        .get("sec-fetch-dest")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("document"))
}

/// Percent-encode a query-string value. Everything outside RFC 3986's unreserved
/// set is escaped, which covers the spaces, apostrophes, `#` and CJK that real
/// headwords and dictionary names contain.
fn encode_query_value(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len() * 3);
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0xf) as usize] as char);
        }
    }
    out
}

/// With `--web`, a person opening `/word/apple` or `/lookup?word=apple` in a
/// browser wants the UI, not a dump of publisher markup with no stylesheet. Send
/// them into the SPA, which fetches the same endpoint itself.
fn redirect_to_ui(word: &str, dict: Option<&str>) -> Response {
    let mut target = format!("/?word={}", encode_query_value(word));
    if let Some(d) = dict {
        target.push_str("&dict=");
        target.push_str(&encode_query_value(d));
    }
    (StatusCode::FOUND, [(header::LOCATION, target)]).into_response()
}

/// Rewrite MDict's internal URL schemes in a definition so a browser can actually
/// fetch them from this server:
///   * `sound://X` and `file://X` → `/res/{dict}/X` (audio/images stored in the .mdd)
///   * `entry://W`               → `/lookup?word=W` (a cross-reference to headword W)
///
/// Scheme-less relative refs (`src="txbapple.jpg"`) are left untouched: a
/// `<base href="/">` is injected into the response and the fallback route serves
/// them from the resource store.
fn rewrite_urls(html: &str, dict: &str) -> String {
    html.replace("sound://", &format!("/res/{dict}/"))
        .replace("file://", &format!("/res/{dict}/"))
        .replace("entry://", "/lookup?word=")
}

/// Collect the raw HTML for `word` from the selected dictionary (or all of them).
/// Returns `(dict_name, html)` pairs in registry order, with URLs rewritten for
/// that dictionary.
fn gather_html(st: &AppState, word: &str, dict: Option<&str>) -> Result<Vec<(String, String)>, Response> {
    let entries: Vec<&DictEntry> = match dict {
        Some(name) => match st.registry.get(name) {
            Some(e) => vec![e],
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "unknown dictionary", "dict": name })),
                )
                    .into_response())
            }
        },
        None => st.registry.entries.iter().collect(),
    };

    let mut out = Vec::new();
    for e in entries {
        let records = e.dict.lookup_word(word);
        if records.is_empty() {
            continue;
        }
        let raw = records
            .iter()
            .map(|b| decode_record(b))
            .collect::<Vec<_>>()
            .join("\n<hr/>\n");
        out.push((e.name.clone(), rewrite_urls(&raw, &e.name)));
    }
    Ok(out)
}

async fn respond_definition(st: &Arc<AppState>, word: &str, dict: Option<&str>, format: Format) -> Response {
    let matches = match gather_html(st, word, dict) {
        Ok(m) => m,
        Err(resp) => return resp,
    };
    if matches.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "not found", "word": word })),
        )
            .into_response();
    }

    let names: Vec<&str> = matches.iter().map(|(n, _)| n.as_str()).collect();
    let xdict = names.join(",");
    let xdict_header = header::HeaderName::from_static("x-dictionary");
    let multi = matches.len() > 1;

    // The combined HTML body, identical for the `html` and `json` formats:
    // <base href="/"> makes the definition's relative asset refs (e.g.
    // src="txbapple.jpg") resolve at the origin root regardless of whether the
    // page was served from /lookup or /word/<w>; the fallback route then serves
    // them from the resource store.
    let html_body = {
        let inner = if multi {
            matches
                .iter()
                .map(|(name, html)| {
                    format!(
                        "<section class=\"dict\" data-dict=\"{name}\"><h3>{name}</h3>{html}</section>"
                    )
                })
                .collect::<Vec<_>>()
                .join("\n<hr/>\n")
        } else {
            matches.first().map(|(_, h)| h.clone()).unwrap_or_default()
        };
        format!("<base href=\"/\">\n{inner}")
    };

    match format {
        Format::Json => (
            [(xdict_header, xdict.as_str())],
            Json(json!({
                "word": word,
                "format": "html",
                "dictionaries": names,
                "html": html_body,
            })),
        )
            .into_response(),
        Format::Markdown => {
            // Convert each dictionary's HTML off the async worker, then combine.
            // Conversions are cached: the stores are immutable, and hover
            // previews plus the copy button ask for the same word repeatedly.
            let mut mds: Vec<Option<Arc<String>>> = vec![None; matches.len()];
            let mut pending: Vec<(usize, String, String)> = Vec::new();
            {
                let cache = st.md_cache.lock().await;
                for (i, (name, _)) in matches.iter().enumerate() {
                    let key = format!("{name}\u{0}{word}");
                    match cache.get(&key) {
                        Some(md) => mds[i] = Some(md.clone()),
                        None => pending.push((i, key, matches[i].1.clone())),
                    }
                }
            }
            if !pending.is_empty() {
                let converted = tokio::task::spawn_blocking(move || {
                    pending
                        .into_iter()
                        .map(|(i, key, html)| (i, key, html_to_markdown(&html)))
                        .collect::<Vec<_>>()
                })
                .await
                .unwrap_or_default();
                let mut cache = st.md_cache.lock().await;
                for (i, key, md) in converted {
                    if cache.len() >= MD_CACHE_CAP {
                        cache.clear();
                    }
                    let shared = Arc::new(md);
                    cache.insert(key, shared.clone());
                    mds[i] = Some(shared);
                }
            }
            let mds: Vec<String> = mds
                .into_iter()
                .map(|md| md.map(|m| (*m).clone()).unwrap_or_default())
                .collect();

            let body = if multi {
                matches
                    .iter()
                    .zip(mds.iter())
                    // `#`, one level above the `##` the converter gives an entry's
                    // headword, so the dictionary name stays the outer heading.
                    .map(|((name, _), md)| format!("# {name}\n\n{md}"))
                    .collect::<Vec<_>>()
                    .join("\n\n---\n\n")
            } else {
                mds.into_iter().next().unwrap_or_default()
            };
            (
                [
                    (header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
                    (xdict_header, xdict.as_str()),
                ],
                body,
            )
                .into_response()
        }
        Format::Html => (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (xdict_header, xdict.as_str()),
            ],
            html_body,
        )
            .into_response(),
    }
}

async fn lookup(
    State(st): State<Arc<AppState>>,
    Query(q): Query<LookupParams>,
    headers: HeaderMap,
) -> Response {
    if st.registry.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"no dictionaries loaded"}))).into_response();
    }
    let word = match q.word {
        Some(w) if !w.trim().is_empty() => w,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "missing 'word' query parameter" })),
            )
                .into_response()
        }
    };
    if st.web.is_some() && q.format.is_none() && is_browser_navigation(&headers) {
        return redirect_to_ui(&word, q.dict.as_deref());
    }
    let format = parse_format(q.format.as_deref(), &headers);
    respond_definition(&st, &word, q.dict.as_deref(), format).await
}

async fn lookup_path(
    State(st): State<Arc<AppState>>,
    Path(word): Path<String>,
    Query(q): Query<LookupParams>,
    headers: HeaderMap,
) -> Response {
    if st.registry.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"no dictionaries loaded"}))).into_response();
    }
    if st.web.is_some() && q.format.is_none() && is_browser_navigation(&headers) {
        return redirect_to_ui(&word, q.dict.as_deref());
    }
    let format = parse_format(q.format.as_deref(), &headers);
    respond_definition(&st, &word, q.dict.as_deref(), format).await
}

const DEFAULT_SUGGEST_LIMIT: usize = 20;
const MAX_SUGGEST_LIMIT: usize = 100;

#[derive(Debug, Deserialize)]
pub struct SuggestParams {
    /// The partial input to complete. `q` is canonical; `word`/`prefix` are
    /// accepted as aliases.
    pub q: Option<String>,
    pub word: Option<String>,
    pub prefix: Option<String>,
    /// Restrict to one dictionary by name; omit to search all.
    pub dict: Option<String>,
    pub limit: Option<usize>,
    /// Opt in to the typo-tolerant pass. It scans every key in the index, so
    /// as-you-type callers leave it off and request it once the input settles.
    pub fuzzy: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct NearbyParams {
    pub word: Option<String>,
    pub dict: Option<String>,
    pub before: Option<usize>,
    pub after: Option<usize>,
}

/// Autocomplete endpoint: given a partial input, return headwords that begin with
/// it. Prefix scanning is a cheap binary search + short walk over the mmap'd,
/// byte-sorted key index, so this runs inline on the async worker.
async fn suggest(State(st): State<Arc<AppState>>, Query(q): Query<SuggestParams>) -> Response {
    if st.registry.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"no dictionaries loaded"}))).into_response();
    }
    let partial = q
        .q
        .or(q.word)
        .or(q.prefix)
        .unwrap_or_default();
    // A missing param and an empty one both mean "no input"; return the
    // alphabetically first headwords rather than an error, which is friendlier
    // for an as-you-type UI that starts from "".
    let limit = q.limit.map(|l| l.clamp(1, MAX_SUGGEST_LIMIT)).unwrap_or(DEFAULT_SUGGEST_LIMIT);

    let entries: Vec<&DictEntry> = match q.dict.as_deref() {
        Some(name) => match st.registry.get(name) {
            Some(e) => vec![e],
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "unknown dictionary", "dict": name })),
                )
                    .into_response()
            }
        },
        None => st.registry.entries.iter().collect(),
    };

    // Merge across dictionaries, dedupe, and keep the list globally sorted.
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    let wildcard = partial.contains('*') || partial.contains('?');
    let mut mode = if wildcard { "wildcard" } else { "prefix" };
    // The typo-tolerant additions, reported separately so a client can label
    // them "did you mean" instead of mixing them into the prefix list.
    let mut fuzzy_only: Vec<String> = Vec::new();

    // Prefix is a binary search plus a short walk and runs inline. Wildcards and
    // the typo fallback scan every key in the index, so they go to a blocking
    // task rather than parking an async worker.
    if wildcard {
        let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
        let registry = Arc::clone(&st.registry);
        let pattern = partial.clone();
        out = tokio::task::spawn_blocking(move || {
            scan(&registry, &names, &pattern, limit, true, |d, q, n| d.search_glob(q, n))
        })
        .await
        .unwrap_or_default();
    } else {
        for e in &entries {
            for s in e.dict.suggest(&partial, limit) {
                if seen.insert(s.clone()) {
                    out.push(s);
                }
                if out.len() >= limit {
                    break;
                }
            }
            if out.len() >= limit {
                break;
            }
        }
        out.sort();
        out.truncate(limit);

        // A typo is otherwise a dead end: "aple" prefix-matches "aplenty" and
        // the reader never sees "apple". Only for inputs long enough that
        // distance 2 is meaningful, and skipped when the input is already a
        // headword somewhere.
        let has_exact = entries.iter().any(|e| e.dict.nearby(&partial, 0, 0).0);
        if q.fuzzy.unwrap_or(false) && partial.chars().count() >= 4 && !has_exact {
            let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
            let registry = Arc::clone(&st.registry);
            let word = partial.clone();
            let found = tokio::task::spawn_blocking(move || {
                scan(&registry, &names, &word, limit, true, |d, q, n| d.fuzzy(q, 2, n))
            })
            .await
            .unwrap_or_default();
            if !found.is_empty() {
                mode = if out.is_empty() { "fuzzy" } else { "prefix+fuzzy" };
                for s in &found {
                    if seen.insert(s.clone()) {
                        out.push(s.clone());
                    }
                }
                out.truncate(limit);
                fuzzy_only = found;
            }
        }
    }

    (
        [(
            header::HeaderName::from_static("x-dictionary"),
            entries
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>()
                .join(","),
        )],
        Json(json!({
            "query": partial,
            "count": out.len(),
            "mode": mode,
            "fuzzy": fuzzy_only,
            "suggestions": out,
        })),
    )
        .into_response()
}

/// Run one whole-index search mode over the named dictionaries and merge the
/// results, deduped and globally sorted (or distance-ordered by the closure).
fn scan(
    registry: &Registry,
    names: &[String],
    query: &str,
    limit: usize,
    preserve_order: bool,
    run: impl Fn(&crate::lookup::Dict, &str, usize) -> Vec<String>,
) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for name in names {
        let Some(e) = registry.get(name) else { continue };
        for s in run(&e.dict, query, limit) {
            if seen.insert(s.clone()) {
                out.push(s);
            }
        }
        if out.len() >= limit {
            break;
        }
    }
    // Wildcards keep index order; fuzzy results arrive closest-match-first and
    // sorting them alphabetically would bury the word the reader meant.
    if !preserve_order {
        out.sort();
    }
    out.truncate(limit);
    out
}

/// The headwords sitting either side of `word` in a dictionary's byte-sorted
/// index. `/suggest` cannot answer this — it only scans forward from a prefix —
/// and "the words around this one" is what a paper dictionary gives you when
/// your thumb lands on the wrong page.
async fn nearby(State(st): State<Arc<AppState>>, Query(p): Query<NearbyParams>) -> Response {
    let word = p.word.clone().unwrap_or_default();
    if word.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing word parameter" })),
        )
            .into_response();
    }
    let before = p.before.unwrap_or(5).clamp(0, 25);
    let after = p.after.unwrap_or(5).clamp(0, 25);

    let all: Vec<&DictEntry> = st.registry.entries.iter().filter(|e| !e.dict.is_resource()).collect();
    let chosen: &DictEntry = match p.dict.as_deref() {
        Some(name) => match st.registry.get(name) {
            Some(e) => e,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "unknown dictionary", "dict": name })),
                )
                    .into_response()
            }
        },
        // Prefer a dictionary that actually has the word, so the window sits in
        // the index the reader is reading; fall back to the first loaded one.
        None => match all.iter().find(|e| e.dict.nearby(&word, 0, 0).0) {
            Some(e) => e,
            None => match all.first() {
                Some(e) => e,
                None => {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({ "error": "no dictionaries loaded" })),
                    )
                        .into_response()
                }
            },
        },
    };

    let (exact, prev, next) = chosen.dict.nearby(&word, before, after);
    Json(json!({
        "word": word,
        "dict": chosen.name,
        "exact": exact,
        "prev": prev,
        "next": next,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct ResourceQuery {
    pub dict: Option<String>,
}

/// Serve a resource by path. With `dict`, use that dictionary's store; without it,
/// search every loaded resource store in registry order (first match wins).
///
/// `crate::web` calls this too, so an SPA route that matches no built file still
/// resolves a dictionary asset keyed the same way.
pub(crate) async fn serve_resource(st: &AppState, dict: Option<&str>, path: &str) -> Response {
    match resource_bytes(st, dict, path) {
        Ok(bytes) => {
            // Speex and friends cannot be played by any browser, so with a
            // decoder configured the conversion happens here, at the URL the
            // definition already points at. Without one, or if the decode
            // fails, the stored bytes go out untouched and the UI explains.
            if st.ffmpeg.is_some() && undecodable_audio(path) {
                let key = format!("res\u{0}{}\u{0}{}", dict.unwrap_or(""), path);
                if let Ok(wav) = convert_audio(st, &key, bytes.clone()).await {
                    return wav_response(wav);
                }
            }
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            // A .qmd store never changes, so neither do the bytes it serves.
            (
                [
                    (header::CONTENT_TYPE, mime.as_ref()),
                    (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                ],
                bytes,
            )
                .into_response()
        }
        Err(response) => response,
    }
}

/// The stored bytes behind a resource reference, or the 404 that explains why
/// there are none. Shared by `/res`, the asset fallback and `/transcode`.
fn resource_bytes(st: &AppState, dict: Option<&str>, path: &str) -> Result<Vec<u8>, Response> {
    let candidates: Vec<&Arc<Dict>> = match dict {
        Some(name) => match st.registry.get(name) {
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "unknown dictionary", "dict": name })),
                )
                    .into_response())
            }
            Some(e) => match &e.res {
                Some(r) => vec![r],
                None => {
                    return Err((
                        StatusCode::NOT_FOUND,
                        Json(json!({ "error": "dictionary has no resources", "dict": name })),
                    )
                        .into_response())
                }
            },
        },
        None => st
            .registry
            .entries
            .iter()
            .filter_map(|e| e.res.as_ref())
            .collect(),
    };

    if candidates.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no resource dictionaries loaded" })),
        )
            .into_response());
    }

    for res in &candidates {
        if let Some(bytes) = res.lookup_resource(path) {
            return Ok(bytes.to_vec());
        }
    }
    Err((
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "resource not found", "path": path })),
    )
        .into_response())
}

/// Pronunciation clips are a few KB; anything bigger is not audio this route
/// should be re-encoding on demand.
const MAX_TRANSCODE_INPUT: usize = 8 << 20;

/// Codecs no browser ships a decoder for. `.spx` is the one that matters:
/// CALD4, `Sound` and `Sound-en_GB` store every pronunciation in Ogg-Speex, and
/// `mime_guess` would happily serve it as `audio/ogg` for `<audio>` to fail at
/// decode time with a 200 status.
pub fn undecodable_audio(path: &str) -> bool {
    let ext = path.rsplit('.').next().unwrap_or("");
    let ext = if ext.len() <= 4 { ext.to_ascii_lowercase() } else { String::new() };
    matches!(ext.as_str(), "spx" | "wma" | "amr" | "mid")
}

enum ConvertError {
    NoDecoder,
    TooLarge(usize),
    Undecodable(String),
}

/// Decode stored audio into PCM WAV, memoized. Shared by `/res` (which converts
/// transparently and falls back to the raw bytes on failure) and `/transcode`
/// (which reports the failure).
async fn convert_audio(st: &AppState, key: &str, input: Vec<u8>) -> Result<Arc<Vec<u8>>, ConvertError> {
    let Some(ffmpeg) = st.ffmpeg.clone() else {
        return Err(ConvertError::NoDecoder);
    };
    if let Some(cached) = st.transcode_cache.lock().await.get(key) {
        return Ok(cached.clone());
    }
    if input.len() > MAX_TRANSCODE_INPUT {
        return Err(ConvertError::TooLarge(input.len()));
    }
    let key_owned = key.to_owned();
    let decoded = tokio::task::spawn_blocking(move || run_ffmpeg(&ffmpeg, &input))
        .await
        .unwrap_or_else(|e| Err(format!("transcode task failed: {e}")));
    match decoded {
        Ok(wav) => {
            let wav = Arc::new(wav);
            st.transcode_cache
                .lock()
                .await
                .insert(key_owned, wav.clone());
            Ok(wav)
        }
        Err(detail) => Err(ConvertError::Undecodable(detail)),
    }
}

fn wav_response(wav: Arc<Vec<u8>>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "audio/wav"),
            // The input is immutable, so the output is too.
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        (*wav).clone(),
    )
        .into_response()
}

/// Decode a stored audio file no browser can play — in practice Ogg-Speex
/// `.spx`, which CALD4 and the Sound libraries use for every pronunciation.
/// `/res` already converts these transparently when a decoder is configured;
/// this route is the explicit form, for clients that want the conversion
/// named in the URL and the failures reported rather than fallen back from.
///
/// The path is resolved through the resource stores, never the filesystem, so
/// there is nothing to traverse.
async fn transcode(
    State(st): State<Arc<AppState>>,
    Path(path): Path<String>,
    Query(q): Query<ResourceQuery>,
) -> Response {
    if path.is_empty() || path.ends_with('/') {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "resource not found", "path": path })),
        )
            .into_response();
    }
    let key = format!("{}\u{0}{}", q.dict.as_deref().unwrap_or(""), path);
    let input = match resource_bytes(&st, q.dict.as_deref(), &path) {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };
    match convert_audio(&st, &key, input).await {
        Ok(wav) => wav_response(wav),
        Err(ConvertError::NoDecoder) => (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({ "error": "transcoding not configured" })),
        )
            .into_response(),
        Err(ConvertError::TooLarge(bytes)) => (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "error": "audio too large to transcode", "bytes": bytes })),
        )
            .into_response(),
        Err(ConvertError::Undecodable(detail)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "undecodable audio", "detail": detail })),
        )
            .into_response(),
    }
}

fn run_ffmpeg(ffmpeg: &std::path::Path, input: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            "pipe:0",
            "-f",
            "wav",
            "-c:a",
            "pcm_s16le",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start {}: {e}", ffmpeg.display()))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "ffmpeg stdin unavailable".to_owned())?;
    stdin.write_all(input).map_err(|e| e.to_string())?;
    drop(stdin);
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(stderr.trim().chars().take(300).collect());
    }
    if out.stdout.is_empty() {
        return Err("ffmpeg produced no output".to_owned());
    }
    Ok(out.stdout)
}

async fn resource(
    State(st): State<Arc<AppState>>,
    Path((dict, path)): Path<(String, String)>,
) -> Response {
    serve_resource(&st, Some(&dict), &path).await
}

/// Fallback for any unmatched path: definitions reference assets by root-relative
/// path (e.g. `/media/.../fruit.jpg`, `/snd_uk.png`), which the browser requests
/// from this origin. Serve those from the resource store(s). `?dict=` scopes it.
async fn fallback_resource(
    State(st): State<Arc<AppState>>,
    uri: Uri,
    Query(q): Query<ResourceQuery>,
) -> Response {
    serve_resource(&st, q.dict.as_deref(), uri.path()).await
}

/// Entries in the Markdown cache; cleared wholesale rather than tracked per
/// key, since a dictionary set is fixed for the life of the process.
const MD_CACHE_CAP: usize = 1024;
const GZ_CACHE_CAP: usize = 256;
/// Not worth the latency or the header churn.
const GZ_MIN_BYTES: usize = 1024;
const GZ_MAX_BYTES: usize = 16 << 20;

fn gz_encode(raw: &[u8], best: bool) -> Option<Vec<u8>> {
    use std::io::Write;
    // Immutable assets are compressed once and cached, so they earn the slow
    // level; dynamic responses pay per request and take the fast one.
    let level = if best {
        flate2::Compression::best()
    } else {
        flate2::Compression::fast()
    };
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), level);
    enc.write_all(raw).ok()?;
    enc.finish().ok()
}

fn is_compressible(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || content_type.contains("json")
        || content_type.contains("javascript")
        || content_type.contains("markdown")
        || content_type.contains("xml")
}

/// Gzip every compressible response. Dictionary HTML runs to 237 KB and the
/// bundle to 400 KB; on the wire that is the difference between a fast lookup
/// and a slow one, and nothing else in this binary spoke `Content-Encoding`.
async fn compress(State(st): State<Arc<AppState>>, req: Request<Body>, next: Next) -> Response {
    let wants_gzip = req
        .headers()
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("gzip"));
    let path = req.uri().path().to_owned();
    let res = next.run(req).await;
    if !wants_gzip {
        return res;
    }
    let content_type = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    if !is_compressible(&content_type) || res.headers().contains_key(header::CONTENT_ENCODING) {
        return res;
    }
    let immutable = res
        .headers()
        .get(header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("immutable"));
    if immutable {
        if let Some(gz) = st.gz_cache.lock().await.get(&path) {
            let (parts, _) = res.into_parts();
            return gz_response(parts, gz.as_ref().clone());
        }
    }
    let (parts, body) = res.into_parts();
    let declared = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());
    if declared.is_some_and(|n| n > GZ_MAX_BYTES) {
        return Response::from_parts(parts, Body::empty());
    }
    let Ok(raw) = axum::body::to_bytes(body, GZ_MAX_BYTES).await else {
        return Response::from_parts(parts, Body::empty());
    };
    if raw.len() < GZ_MIN_BYTES {
        return Response::from_parts(parts, Body::from(raw));
    }
    // `Bytes::clone` is a refcount bump, so the fallback path keeps the body
    // without a second copy while the encoder thread borrows it.
    let shared = raw.clone();
    let encoded = tokio::task::spawn_blocking(move || gz_encode(&shared, immutable))
        .await
        .ok()
        .flatten();
    let Some(gz) = encoded else {
        return Response::from_parts(parts, Body::from(raw));
    };
    if gz.len() >= raw.len() {
        return Response::from_parts(parts, Body::from(raw));
    }
    if immutable {
        let mut cache = st.gz_cache.lock().await;
        if cache.len() >= GZ_CACHE_CAP {
            cache.clear();
        }
        cache.insert(path, Arc::new(gz.clone()));
    }
    gz_response(parts, gz)
}

/// Swap in a gzipped body and the headers that make it decodable. The stale
/// `Content-Length` has to go or the client waits for the uncompressed size.
fn gz_response(mut parts: axum::http::response::Parts, gz: Vec<u8>) -> Response {
    parts.headers.remove(header::CONTENT_LENGTH);
    parts
        .headers
        .insert(header::CONTENT_ENCODING, header::HeaderValue::from_static("gzip"));
    parts
        .headers
        .insert(header::VARY, header::HeaderValue::from_static("accept-encoding"));
    Response::from_parts(parts, Body::from(gz))
}

#[cfg(test)]
mod tests {
    use super::{gz_encode, is_compressible, parse_format, rewrite_urls, Format};

    #[test]
    fn gzip_round_trips() {
        let raw = b"apple apple apple apple apple apple apple apple".to_vec();
        let gz = gz_encode(&raw, false).unwrap();
        assert!(gz.len() < raw.len());
        let mut dec = flate2::read::GzDecoder::new(&gz[..]);
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut dec, &mut out).unwrap();
        assert_eq!(out, raw);
    }

    #[test]
    fn only_text_like_types_are_compressed() {
        for t in [
            "text/html; charset=utf-8",
            "application/json",
            "text/javascript",
            "text/markdown; charset=utf-8",
        ] {
            assert!(is_compressible(t), "{t}");
        }
        for t in ["audio/mpeg", "audio/wav", "image/jpeg", "application/octet-stream"] {
            assert!(!is_compressible(t), "{t}");
        }
    }

    use axum::http::{header, HeaderMap, HeaderValue};

    #[test]
    fn rewrites_mdict_url_schemes() {
        let html = r#"<a href="sound://a.mp3">s</a><a href="entry://Types of fruit">e</a><img src="file://c.png">"#;
        let out = rewrite_urls(html, "ox");
        assert!(out.contains(r#"href="/res/ox/a.mp3""#));
        assert!(out.contains(r#"href="/lookup?word=Types of fruit""#));
        assert!(out.contains(r#"src="/res/ox/c.png""#));
        assert!(!out.contains("sound://"));
        assert!(!out.contains("entry://"));
        assert!(!out.contains("file://"));
    }

    #[test]
    fn format_from_param() {
        let h = HeaderMap::new();
        assert_eq!(parse_format(Some("json"), &h), Format::Json);
        assert_eq!(parse_format(Some("JSON"), &h), Format::Json);
        assert_eq!(parse_format(Some("md"), &h), Format::Markdown);
        assert_eq!(parse_format(Some("html"), &h), Format::Html);
        assert_eq!(parse_format(Some("nonsense"), &h), Format::Html);
        assert_eq!(parse_format(None, &h), Format::Html);
    }

    #[test]
    fn format_from_accept() {
        let mut h = HeaderMap::new();
        h.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        assert_eq!(parse_format(None, &h), Format::Json);
        // An explicit param wins over the Accept header.
        assert_eq!(parse_format(Some("md"), &h), Format::Markdown);
        // A browser's wildcard Accept stays HTML.
        let mut h2 = HeaderMap::new();
        h2.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml,*/*;q=0.8"),
        );
        assert_eq!(parse_format(None, &h2), Format::Html);
    }
}
