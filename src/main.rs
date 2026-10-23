use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;

use mdict_svr::build;
use mdict_svr::lookup::Dict;
use mdict_svr::md::{decode_record, html_to_markdown};
use mdict_svr::server::{router, AppState, DictEntry, Registry};
use mdict_svr::web::WebRoot;

#[derive(Parser)]
#[command(
    name = "mdict_svr",
    version,
    about = "Import MDict (.mdx/.mdd) dictionaries into a packed index and serve lookups over HTTP"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Import a .mdx (and optional .mdd resources) into packed .qmd database(s)
    Import {
        /// Source: a .mdx file, or a directory to scan recursively for every
        /// .mdx/.mdd (each X.mdx → X.qmd, its sibling X.mdd → X.res.qmd).
        #[arg(long)]
        mdx: PathBuf,
        /// Companion resource file (.mdd). Single-file mode only.
        #[arg(long)]
        mdd: Option<PathBuf>,
        /// Output .qmd (single-file mode; default: alongside the source). In
        /// directory mode this is the output directory (default: the input dir),
        /// mirroring the source subpaths.
        #[arg(short = 'o', long = "out")]
        out: Option<PathBuf>,
        /// Output for resources (default: <out stem>.res.qmd). Single-file mode only.
        #[arg(long = "res-out")]
        res_out: Option<PathBuf>,
    },
    /// Look up a word offline and print its definition
    Lookup {
        /// The word to look up
        word: String,
        /// Packed database (.qmd) to query
        #[arg(long)]
        db: PathBuf,
        /// Output format: html (default) or md/markdown
        #[arg(long, default_value = "html")]
        format: String,
    },
    /// Serve lookups over HTTP (one or more dictionaries)
    Serve {
        /// Dictionary to serve: `NAME=PATH`, a `PATH` to a .qmd (name defaults to
        /// the file stem), or a `PATH` to a directory (recursively serves every
        /// *.qmd under it; X.qmd is a dictionary, X.res.qmd its resources).
        /// Repeatable.
        #[arg(long = "db", required = true)]
        db: Vec<String>,
        /// Resource database: `NAME=PATH` (attached to dictionary NAME), a bare
        /// `PATH` (attached to the sole dictionary), or a directory (scanned for
        /// *.res.qmd). Repeatable.
        #[arg(long = "res")]
        res: Vec<String>,
        /// Socket address to bind
        #[arg(long, default_value = "127.0.0.1:8080")]
        addr: String,
        /// Directory of built web UI assets to serve alongside the API (the
        /// output of `npm run build` in `web/`). Must contain `index.html`.
        #[arg(long)]
        web: Option<PathBuf>,
        /// ffmpeg binary used by `GET /transcode` to decode audio no browser can
        /// play (Ogg-Speex `.spx`). Without it those files stay download-only.
        #[arg(long)]
        ffmpeg: Option<PathBuf>,
    },
    /// Print database metadata as JSON
    Info {
        /// Packed database (.qmd)
        #[arg(long)]
        db: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Import {
            mdx,
            mdd,
            out,
            res_out,
        } => cmd_import(mdx, mdd, out, res_out).await,
        Command::Lookup { word, db, format } => cmd_lookup(word, db, format),
        Command::Serve { db, res, addr, web, ffmpeg } => cmd_serve(db, res, addr, web, ffmpeg).await,
        Command::Info { db } => cmd_info(db),
    }
}

async fn cmd_import(
    mdx: PathBuf,
    mdd: Option<PathBuf>,
    out: Option<PathBuf>,
    res_out: Option<PathBuf>,
) -> Result<()> {
    if mdx.is_dir() {
        if mdd.is_some() || res_out.is_some() {
            bail!("--mdd/--res-out are for single-file import; omit them when --mdx is a directory");
        }
        let outdir = out.unwrap_or_else(|| mdx.clone());
        return import_dir(&mdx, &outdir).await;
    }

    // Single file: default output is alongside the source, with a .qmd extension.
    let out = out.unwrap_or_else(|| mdx.with_extension("qmd"));
    import_one_dict(&mdx, &out).await?;
    if let Some(mdd) = mdd {
        let res_out = res_out.unwrap_or_else(|| default_res_path(&out));
        import_one_res(&mdd, &res_out).await?;
    } else if res_out.is_some() {
        bail!("--res-out given without --mdd");
    }
    Ok(())
}

/// Import every `.mdx`/`.mdd` under `root`, writing `.qmd` files under `outdir`
/// and mirroring the source subpaths: `X.mdx` → `X.qmd`, a sibling `X.mdd` →
/// `X.res.qmd`, and a lone `X.mdd` → `X.res.qmd`.
async fn import_dir(root: &Path, outdir: &Path) -> Result<()> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_dict_files(root, &mut files)?;
    files.sort();
    if files.is_empty() {
        bail!("no .mdx/.mdd files found under {}", root.display());
    }

    // Index .mdd files by (parent dir, lowercase stem) to pair with a sibling .mdx.
    let mut mdd_by_stem: std::collections::HashMap<(PathBuf, String), PathBuf> =
        std::collections::HashMap::new();
    for f in &files {
        if eq_ext(f, "mdd") {
            if let (Some(parent), Some(stem)) = (f.parent(), f.file_stem()) {
                mdd_by_stem.insert(
                    (parent.to_path_buf(), stem.to_string_lossy().to_lowercase()),
                    f.clone(),
                );
            }
        }
    }

    let mut paired_mdd: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut count = 0usize;

    for f in &files {
        if !eq_ext(f, "mdx") {
            continue;
        }
        let out = out_path(root, outdir, f, "qmd");
        ensure_parent(&out)?;
        import_one_dict(f, &out).await?;
        count += 1;

        if let (Some(parent), Some(stem)) = (f.parent(), f.file_stem()) {
            let key = (parent.to_path_buf(), stem.to_string_lossy().to_lowercase());
            if let Some(mdd) = mdd_by_stem.get(&key) {
                let res_out = out_path(root, outdir, mdd, "res.qmd");
                ensure_parent(&res_out)?;
                import_one_res(mdd, &res_out).await?;
                paired_mdd.insert(mdd.clone());
            }
        }
    }

    // Lone .mdd files: resources with no matching word dictionary.
    for f in &files {
        if eq_ext(f, "mdd") && !paired_mdd.contains(f) {
            let res_out = out_path(root, outdir, f, "res.qmd");
            ensure_parent(&res_out)?;
            import_one_res(f, &res_out).await?;
            count += 1;
        }
    }

    println!(
        "imported {count} dictionar{} from {} into {}",
        if count == 1 { "y" } else { "ies" },
        root.display(),
        outdir.display()
    );
    Ok(())
}

/// Recursively collect `*.mdx` and `*.mdd` under `dir`.
fn collect_dict_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_dict_files(&path, out)?;
        } else if eq_ext(&path, "mdx") || eq_ext(&path, "mdd") {
            out.push(path);
        }
    }
    Ok(())
}

/// Case-insensitive file-extension check.
fn eq_ext(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case(ext))
        .unwrap_or(false)
}

/// Map a source file under `root` to its output path under `outdir`, replacing
/// the extension with `new_ext` (`"qmd"` or `"res.qmd"`) and preserving subpaths.
fn out_path(root: &Path, outdir: &Path, src: &Path, new_ext: &str) -> PathBuf {
    let rel = src.strip_prefix(root).unwrap_or(src);
    let stem = rel
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = rel.parent().unwrap_or(Path::new(""));
    outdir.join(parent).join(format!("{stem}.{new_ext}"))
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

async fn import_one_dict(mdx: &Path, out: &Path) -> Result<()> {
    let m = mdx.to_path_buf();
    let o = out.to_path_buf();
    let report = tokio::task::spawn_blocking(move || build::import_dictionary(&m, &o))
        .await?
        .with_context(|| format!("importing {}", mdx.display()))?;
    println!(
        "imported dictionary: title={:?} version={} encoding={} encrypted={} entries={} \
         -> keys={} slots={} text_bytes={}",
        report.title,
        report.version,
        report.encoding,
        report.encrypted,
        report.entries,
        report.stats.num_keys,
        report.stats.num_slots,
        report.stats.text_bytes,
    );
    println!("wrote {}", out.display());
    Ok(())
}

async fn import_one_res(mdd: &Path, res_out: &Path) -> Result<()> {
    let m = mdd.to_path_buf();
    let o = res_out.to_path_buf();
    let report = tokio::task::spawn_blocking(move || build::import_resources(&m, &o))
        .await?
        .with_context(|| format!("importing {}", mdd.display()))?;
    println!(
        "imported resources: entries={} -> keys={} slots={} text_bytes={}",
        report.entries, report.stats.num_keys, report.stats.num_slots, report.stats.text_bytes
    );
    println!("wrote {}", res_out.display());
    Ok(())
}

fn default_res_path(out: &Path) -> PathBuf {
    match out.file_stem() {
        Some(stem) => out.with_file_name(format!("{}.res.qmd", stem.to_string_lossy())),
        None => out.with_extension("res.qmd"),
    }
}

fn cmd_lookup(word: String, db: PathBuf, format: String) -> Result<()> {
    let dict = Dict::open(&db).with_context(|| format!("opening {}", db.display()))?;
    let records = dict.lookup_word(&word);
    if records.is_empty() {
        eprintln!("not found: {word}");
        std::process::exit(2);
    }
    let html: String = records
        .iter()
        .map(|b| decode_record(b))
        .collect::<Vec<_>>()
        .join("\n<hr/>\n");
    let markdown = matches!(
        format.trim().to_ascii_lowercase().as_str(),
        "md" | "markdown" | "text/markdown"
    );
    let out = if markdown { html_to_markdown(&html) } else { html };
    println!("{out}");
    Ok(())
}

fn cmd_info(db: PathBuf) -> Result<()> {
    let dict = Dict::open(&db).with_context(|| format!("opening {}", db.display()))?;
    let m = dict.meta();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "path": db.display().to_string(),
            "title": m.title,
            "description": m.description,
            "creation_date": m.creation_date,
            "source": m.source,
            "is_resource": m.is_resource,
            "case_sensitive": m.case_sensitive,
            "entries": m.num_keys,
            "records": m.num_slots,
        }))?
    );
    Ok(())
}

/// Split a `NAME=PATH` or bare `PATH` spec into (optional explicit name, path).
fn split_spec(spec: &str) -> (Option<String>, PathBuf) {
    match spec.split_once('=') {
        Some((name, path)) if !name.trim().is_empty() => {
            (Some(name.trim().to_string()), PathBuf::from(path))
        }
        _ => (None, PathBuf::from(spec)),
    }
}

/// Default name for a path: its file stem (so `foo.qmd` → `foo`,
/// `foo.res.qmd` → `foo.res`).
fn stem_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// A resource store to attach, and the dictionary it belongs to (`None` = the
/// sole dictionary). `explicit` marks ones given on the command line, whose
/// target must exist; discovered ones are skipped silently if unmatched.
struct ResSpec {
    dict: Option<String>,
    path: PathBuf,
    explicit: bool,
}

/// Recursively collect every `*.qmd` path under `dir`.
fn collect_qmd(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_qmd(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("qmd") {
            out.push(path);
        }
    }
    Ok(())
}

/// Name from the path relative to `root`, minus the `.qmd`/`.res.qmd` suffix,
/// with separators replaced by `-`. Used to disambiguate duplicate stems.
fn rel_name(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let mut s = rel.to_string_lossy().into_owned();
    for suffix in [".res.qmd", ".qmd"] {
        if let Some(trimmed) = s.strip_suffix(suffix) {
            s = trimmed.to_string();
            break;
        }
    }
    s.replace(std::path::MAIN_SEPARATOR, "-").replace('/', "-")
}

/// Scan a directory for dictionaries: `X.qmd` is a word dictionary named `X`,
/// `X.res.qmd` is the resource store for dictionary `X`. Word-dictionary names
/// are the file stem, disambiguated by relative path on collision. Results are
/// sorted by path for a deterministic load order.
fn scan_qmd_dir(root: &Path) -> Result<(Vec<(String, PathBuf)>, Vec<ResSpec>)> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_qmd(root, &mut files)?;
    files.sort();

    let mut dicts: Vec<(String, PathBuf)> = Vec::new();
    let mut resources: Vec<ResSpec> = Vec::new();
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();

    for path in files {
        let stem = stem_name(&path); // "foo" or "foo.res"
        match stem.strip_suffix(".res") {
            Some(base) => resources.push(ResSpec {
                dict: Some(base.to_string()),
                path,
                explicit: false,
            }),
            None => {
                let mut name = stem.clone();
                if used.contains(&name) {
                    name = rel_name(root, &path);
                }
                used.insert(name.clone());
                dicts.push((name, path));
            }
        }
    }
    Ok((dicts, resources))
}

async fn cmd_serve(
    db: Vec<String>,
    res: Vec<String>,
    addr: String,
    web: Option<PathBuf>,
    ffmpeg: Option<PathBuf>,
) -> Result<()> {
    // Validate up front so a typo'd path fails before the dictionaries load.
    let web = match &web {
        Some(dir) => Some(
            WebRoot::open(dir)
                .with_context(|| format!("opening web assets {}", dir.display()))?,
        ),
        None => None,
    };
    let ffmpeg = match ffmpeg {
        Some(path) => {
            let canon = path
                .canonicalize()
                .with_context(|| format!("resolving ffmpeg {}", path.display()))?;
            if !canon.is_file() {
                bail!("--ffmpeg {}: not a file", canon.display());
            }
            println!("transcoding undecodable audio with {}", canon.display());
            Some(canon)
        }
        None => None,
    };

    let mut dict_specs: Vec<(String, PathBuf)> = Vec::new();
    let mut res_specs: Vec<ResSpec> = Vec::new();

    // Word dictionaries; a directory also yields the resources discovered in it.
    for spec in &db {
        let (name_opt, path) = split_spec(spec);
        if path.is_dir() {
            if name_opt.is_some() {
                bail!("--db NAME=<directory>: cannot name a whole directory; use --db <directory>");
            }
            let (dicts, resources) =
                scan_qmd_dir(&path).with_context(|| format!("scanning {}", path.display()))?;
            println!(
                "scanned {}: {} dictionaries, {} resource stores",
                path.display(),
                dicts.len(),
                resources.len()
            );
            dict_specs.extend(dicts);
            res_specs.extend(resources);
        } else {
            let name = name_opt.unwrap_or_else(|| stem_name(&path));
            dict_specs.push((name, path));
        }
    }

    // Explicit resource stores (a directory is scanned for *.res.qmd).
    for spec in &res {
        let (name_opt, path) = split_spec(spec);
        if path.is_dir() {
            let (_, resources) =
                scan_qmd_dir(&path).with_context(|| format!("scanning {}", path.display()))?;
            res_specs.extend(resources);
        } else {
            res_specs.push(ResSpec {
                dict: name_opt,
                path,
                explicit: true,
            });
        }
    }

    // Open every word dictionary.
    let mut entries: Vec<DictEntry> = Vec::with_capacity(dict_specs.len());
    for (name, path) in &dict_specs {
        if entries.iter().any(|e| &e.name == name) {
            bail!("duplicate dictionary name {name:?}");
        }
        let dict = Arc::new(
            Dict::open(path)
                .with_context(|| format!("opening dictionary {name}: {}", path.display()))?,
        );
        println!(
            "loaded dictionary {name:?} ({} entries) from {}",
            dict.num_keys(),
            path.display()
        );
        entries.push(DictEntry {
            name: name.clone(),
            dict,
            res: None,
        });
    }
    if entries.is_empty() {
        bail!("no dictionaries found");
    }

    // Attach resource stores to their dictionaries.
    for rs in &res_specs {
        let res_dict = Arc::new(
            Dict::open(&rs.path)
                .with_context(|| format!("opening resources {}", rs.path.display()))?,
        );
        let target = match &rs.dict {
            Some(name) => entries.iter_mut().find(|e| &e.name == name),
            None => {
                if entries.len() == 1 {
                    Some(&mut entries[0])
                } else {
                    None
                }
            }
        };
        match target {
            Some(t) => {
                println!("attached resources to {:?} from {}", t.name, rs.path.display());
                t.res = Some(res_dict);
            }
            None => match (&rs.dict, rs.explicit) {
                (None, _) => bail!(
                    "--res {} needs a NAME= prefix: multiple dictionaries loaded",
                    rs.path.display()
                ),
                (Some(name), true) => {
                    bail!("--res {name} matches no loaded dictionary ({})", rs.path.display())
                }
                (Some(_), false) => tracing::warn!(
                    "resource store {} matches no loaded dictionary; skipped",
                    rs.path.display()
                ),
            },
        }
    }

    let total = entries.len();
    if let Some(root) = &web {
        println!("serving web UI from {}", root.dir().display());
    }
    let state = Arc::new(AppState::new(Arc::new(Registry::new(entries)), web, ffmpeg));
    let app = router(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    println!(
        "serving {total} dictionar{} on http://{}",
        if total == 1 { "y" } else { "ies" },
        listener.local_addr()?
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    println!("\nshutting down");
}
