//! HTTP API (axum): serve lookups across one or more dictionaries, returning
//! definitions as HTML or Markdown, plus per-dictionary resources and metadata.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

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
}

#[derive(Debug, Deserialize)]
pub struct LookupParams {
    pub word: Option<String>,
    pub format: Option<String>,
    /// Restrict the lookup to one dictionary by name; omit to search all.
    pub dict: Option<String>,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/info", get(info))
        .route("/dicts", get(dicts))
        .route("/lookup", get(lookup))
        .route("/word/{word}", get(lookup_path))
        .route("/res/{dict}/{*path}", get(resource))
        .fallback(fallback_resource)
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
    Json(json!({ "dictionaries": list })).into_response()
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
            let htmls: Vec<String> = matches.iter().map(|(_, h)| h.clone()).collect();
            let fallback = htmls.clone();
            let mds = tokio::task::spawn_blocking(move || {
                htmls
                    .iter()
                    .map(|h| html_to_markdown(h))
                    .collect::<Vec<String>>()
            })
            .await
            .unwrap_or(fallback);

            let body = if multi {
                matches
                    .iter()
                    .zip(mds.iter())
                    .map(|((name, _), md)| format!("## {name}\n\n{md}"))
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
    let format = parse_format(q.format.as_deref(), &headers);
    respond_definition(&st, &word, q.dict.as_deref(), format).await
}

#[derive(Debug, Deserialize)]
pub struct ResourceQuery {
    pub dict: Option<String>,
}

/// Serve a resource by path. With `dict`, use that dictionary's store; without it,
/// search every loaded resource store in registry order (first match wins).
async fn serve_resource(st: &AppState, dict: Option<&str>, path: &str) -> Response {
    let candidates: Vec<&Arc<Dict>> = match dict {
        Some(name) => match st.registry.get(name) {
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "unknown dictionary", "dict": name })),
                )
                    .into_response()
            }
            Some(e) => match &e.res {
                Some(r) => vec![r],
                None => {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(json!({ "error": "dictionary has no resources", "dict": name })),
                    )
                        .into_response()
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
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no resource dictionaries loaded" })),
        )
            .into_response();
    }

    for res in &candidates {
        if let Some(bytes) = res.lookup_resource(path) {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            return ([(header::CONTENT_TYPE, mime.as_ref())], bytes.to_vec()).into_response();
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "resource not found", "path": path })),
    )
        .into_response()
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

#[cfg(test)]
mod tests {
    use super::{parse_format, rewrite_urls, Format};
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
