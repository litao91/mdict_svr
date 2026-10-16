//! HTTP API (axum): look up a word and return its definition as HTML or
//! Markdown, plus resource and metadata endpoints.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

use crate::lookup::Dict;
use crate::md::html_to_markdown;

/// Shared server state: the word dictionary and (optionally) the resource store.
pub struct AppState {
    pub dict: Arc<Dict>,
    pub res: Option<Arc<Dict>>,
}

#[derive(Debug, Deserialize)]
pub struct LookupParams {
    pub word: Option<String>,
    pub format: Option<String>,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/info", get(info))
        .route("/lookup", get(lookup))
        .route("/word/{word}", get(lookup_path))
        .route("/res/{*path}", get(resource))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn info(State(st): State<Arc<AppState>>) -> Response {
    let m = st.dict.meta();
    Json(json!({
        "title": m.title,
        "description": m.description,
        "creation_date": m.creation_date,
        "source": m.source,
        "entries": m.num_keys,
        "records": m.num_slots,
        "case_sensitive": m.case_sensitive,
        "resources": st.res.as_ref().map(|r| r.num_keys()).unwrap_or(0),
    }))
    .into_response()
}

/// Decide HTML vs Markdown from the `format` param, falling back to `Accept`.
fn want_markdown(fmt: Option<&str>, headers: &HeaderMap) -> bool {
    if let Some(f) = fmt {
        let f = f.trim().to_ascii_lowercase();
        return matches!(f.as_str(), "md" | "markdown" | "text/markdown" | "text/x-markdown");
    }
    if let Some(acc) = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()) {
        let acc = acc.to_ascii_lowercase();
        if acc.contains("text/markdown") || acc.contains("text/x-markdown") {
            return true;
        }
    }
    false
}

async fn respond_definition(st: &Arc<AppState>, word: &str, markdown: bool) -> Response {
    // Build the owned HTML first so no borrow of the mmap is held across an await.
    let html = {
        let records = st.dict.lookup_word(word);
        if records.is_empty() {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "not found", "word": word })),
            )
                .into_response();
        }
        records
            .iter()
            .map(|b| String::from_utf8_lossy(b))
            .collect::<Vec<_>>()
            .join("\n<hr/>\n")
    };

    if markdown {
        let source = html.clone();
        let body = tokio::task::spawn_blocking(move || html_to_markdown(&source))
            .await
            .unwrap_or(html);
        (
            [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
            body,
        )
            .into_response()
    } else {
        (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        )
            .into_response()
    }
}

async fn lookup(
    State(st): State<Arc<AppState>>,
    Query(q): Query<LookupParams>,
    headers: HeaderMap,
) -> Response {
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
    let markdown = want_markdown(q.format.as_deref(), &headers);
    respond_definition(&st, &word, markdown).await
}

async fn lookup_path(
    State(st): State<Arc<AppState>>,
    Path(word): Path<String>,
    Query(q): Query<LookupParams>,
    headers: HeaderMap,
) -> Response {
    let markdown = want_markdown(q.format.as_deref(), &headers);
    respond_definition(&st, &word, markdown).await
}

async fn resource(State(st): State<Arc<AppState>>, Path(path): Path<String>) -> Response {
    let res = match &st.res {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "no resource dictionary loaded" })),
            )
                .into_response()
        }
    };
    match res.lookup_resource(&path) {
        Some(bytes) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref())],
                bytes.to_vec(),
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "resource not found", "path": path })),
        )
            .into_response(),
    }
}
