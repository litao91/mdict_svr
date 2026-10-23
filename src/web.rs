//! Serving the built web UI from the same origin as the API.
//!
//! The SPA keeps all of its state in the query string (`/?word=apple&dict=OALD9`)
//! and deliberately has no path-based routes of its own. That is what keeps it out
//! of the way of dictionary assets: definitions reference files at arbitrary root
//! paths (`/media/english/thumb/f/fru/fruit/fruit.jpg`, `/txbapple.jpg`), and the
//! resource fallback in `server.rs` has to keep serving those.
//!
//! Even so, a dictionary could in principle store a resource keyed `index.html` or
//! under `assets/`, so both handlers fall through to the resource lookup on a miss
//! rather than relying on route priority.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::{Path as PathParam, State},
    http::{header, Uri},
    response::{IntoResponse, Response},
};

use crate::error::{Error, Result};
use crate::server::{serve_resource, AppState};

/// Hashed build output never changes under a given name, so it can be cached
/// forever; the shell must not be, or a rebuild is invisible without a hard
/// refresh.
const CACHE_IMMUTABLE: &str = "public, max-age=31536000, immutable";
const CACHE_NO_CACHE: &str = "no-cache";

/// A directory of built frontend assets, canonicalized once at startup so the
/// per-request containment check compares against a stable root.
#[derive(Clone, Debug)]
pub struct WebRoot {
    dir: PathBuf,
}

impl WebRoot {
    /// Canonicalize `dir` and require an `index.html` inside it.
    pub fn open(dir: impl AsRef<Path>) -> Result<WebRoot> {
        let dir = dir.as_ref().canonicalize()?;
        if !dir.is_dir() {
            return Err(Error::Other(format!("{} is not a directory", dir.display())));
        }
        if !dir.join("index.html").is_file() {
            return Err(Error::Other(format!(
                "{} has no index.html — run `npm run build` in web/ first",
                dir.display()
            )));
        }
        Ok(WebRoot { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// Resolve `rel` under `root`, or `None` if it would escape.
///
/// The check is structural rather than a substring search for `..`: a substring
/// test false-positives on a legitimate `my..file.css`, and it is weaker than what
/// axum already gives us — `Path<String>` is percent-decoded, so `%2e%2e%2f`
/// arrives as `../` and is rejected here like any other parent component.
pub fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.strip_prefix('/').unwrap_or(rel);
    if rel.is_empty() || rel.ends_with('/') {
        return None;
    }
    let path = Path::new(rel);
    if path.is_absolute() {
        return None;
    }
    // CurDir, ParentDir, RootDir and Prefix all fail this test.
    if !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return None;
    }
    // `canonicalize` also proves existence and resolves symlinks, so a link
    // inside the build directory cannot point out of it.
    let canon = root.join(path).canonicalize().ok()?;
    canon.starts_with(root).then_some(canon)
}

/// `GET /` and `GET /index.html` — the SPA shell.
pub async fn index(State(st): State<Arc<AppState>>, uri: Uri) -> Response {
    let Some(root) = st.web.as_ref() else {
        return serve_resource(&st, None, uri.path()).await;
    };
    match read(&root.dir().join("index.html")).await {
        // `mime_guess` reports text/html without a charset.
        Some(bytes) => respond(bytes, "text/html; charset=utf-8", CACHE_NO_CACHE),
        None => serve_resource(&st, None, uri.path()).await,
    }
}

/// `GET /assets/{*path}` — Vite's hashed build output.
pub async fn asset(
    State(st): State<Arc<AppState>>,
    PathParam(path): PathParam<String>,
    uri: Uri,
) -> Response {
    let Some(root) = st.web.as_ref() else {
        return serve_resource(&st, None, uri.path()).await;
    };
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    match safe_join(root.dir(), &format!("assets/{path}")) {
        Some(file) => match read(&file).await {
            Some(bytes) => respond(bytes, mime.as_ref(), CACHE_IMMUTABLE),
            None => serve_resource(&st, None, uri.path()).await,
        },
        None => serve_resource(&st, None, uri.path()).await,
    }
}

async fn read(path: &Path) -> Option<Vec<u8>> {
    tokio::fs::read(path).await.ok()
}

fn respond(bytes: Vec<u8>, content_type: &str, cache_control: &str) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, cache_control),
        ],
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A build directory laid out the way Vite leaves it.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        /// `name` keeps each test's directory distinct: they run in parallel and
        /// would otherwise delete the tree out from under one another.
        fn new(name: &str) -> Fixture {
            let root = std::env::temp_dir().join(format!("web_root_{}_{}", std::process::id(), name));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("assets/nested")).unwrap();
            std::fs::write(root.join("index.html"), b"<html>").unwrap();
            std::fs::write(root.join("assets/index-abc123.js"), b"console.log(1)").unwrap();
            std::fs::write(root.join("assets/nested/a.css"), b"body{}").unwrap();
            // A legitimate name that a substring check for ".." would reject.
            std::fs::write(root.join("assets/my..file.css"), b"i{}").unwrap();
            Fixture { root }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn resolves_real_assets() {
        let f = Fixture::new("resolves");
        let root = f.root.canonicalize().unwrap();
        assert!(safe_join(&root, "index.html").is_some());
        assert!(safe_join(&root, "/index.html").is_some());
        assert!(safe_join(&root, "assets/index-abc123.js").is_some());
        assert!(safe_join(&root, "assets/nested/a.css").is_some());
        // A double dot that is part of a filename, not a traversal.
        assert!(safe_join(&root, "assets/my..file.css").is_some());
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        let f = Fixture::new("traversal");
        let root = f.root.canonicalize().unwrap();
        for bad in [
            "..",
            "../etc/passwd",
            "assets/../../etc/passwd",
            "assets/../../../etc/passwd",
            "./index.html",
            "assets/",
            "",
            "/",
        ] {
            assert!(safe_join(&root, bad).is_none(), "accepted {bad:?}");
        }
        // Absolute paths, including a Windows-style prefix.
        assert!(safe_join(&root, "/etc/passwd").is_none());
        assert!(safe_join(&root, "C:\\Windows\\win.ini").is_none());
    }

    #[test]
    fn rejects_missing_files() {
        let f = Fixture::new("missing");
        let root = f.root.canonicalize().unwrap();
        // `canonicalize` doubles as the existence check, so a well-formed but
        // absent path is a miss and the handler falls through to the resources.
        assert!(safe_join(&root, "assets/nope.js").is_none());
    }

    #[test]
    fn rejects_a_symlink_pointing_outside_the_root() {
        let f = Fixture::new("symlink");
        let root = f.root.canonicalize().unwrap();
        let outside = std::env::temp_dir().join(format!("web_outside_{}_symlink", std::process::id()));
        std::fs::write(&outside, b"secret").unwrap();
        let link = root.join("assets/escape.txt");
        if std::os::unix::fs::symlink(&outside, &link).is_ok() {
            assert!(safe_join(&root, "assets/escape.txt").is_none());
        }
        let _ = std::fs::remove_file(&outside);
    }

    #[test]
    fn open_requires_an_index_html() {
        let f = Fixture::new("open");
        assert!(WebRoot::open(&f.root).is_ok());
        let empty = std::env::temp_dir().join(format!("web_empty_{}_open", std::process::id()));
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(&empty).unwrap();
        let err = WebRoot::open(&empty).unwrap_err().to_string();
        assert!(err.contains("index.html"), "{err}");
        let _ = std::fs::remove_dir_all(&empty);
    }
}
