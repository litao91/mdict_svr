use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;

use mdict_svr::build;
use mdict_svr::lookup::Dict;
use mdict_svr::md::html_to_markdown;
use mdict_svr::server::{router, AppState};

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
        /// Source word dictionary (.mdx)
        #[arg(long)]
        mdx: PathBuf,
        /// Optional companion resource file (.mdd)
        #[arg(long)]
        mdd: Option<PathBuf>,
        /// Output database for the words
        #[arg(short = 'o', long = "out")]
        out: PathBuf,
        /// Output database for resources (default: <out stem>.res.qmd)
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
    /// Serve lookups over HTTP
    Serve {
        /// Packed word database (.qmd)
        #[arg(long)]
        db: PathBuf,
        /// Optional packed resource database (.qmd) for /res
        #[arg(long)]
        res: Option<PathBuf>,
        /// Socket address to bind
        #[arg(long, default_value = "127.0.0.1:8080")]
        addr: String,
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
        Command::Serve { db, res, addr } => cmd_serve(db, res, addr).await,
        Command::Info { db } => cmd_info(db),
    }
}

async fn cmd_import(
    mdx: PathBuf,
    mdd: Option<PathBuf>,
    out: PathBuf,
    res_out: Option<PathBuf>,
) -> Result<()> {
    let dict_mdx = mdx.clone();
    let dict_out = out.clone();
    let report = tokio::task::spawn_blocking(move || {
        build::import_dictionary(&dict_mdx, &dict_out)
    })
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

    if let Some(mdd) = mdd {
        let res_out = res_out.unwrap_or_else(|| default_res_path(&out));
        let r_mdd = mdd.clone();
        let r_out = res_out.clone();
        let rreport = tokio::task::spawn_blocking(move || {
            build::import_resources(&r_mdd, &r_out)
        })
        .await?
        .with_context(|| format!("importing {}", mdd.display()))?;
        println!(
            "imported resources: entries={} -> keys={} slots={} text_bytes={}",
            rreport.entries, rreport.stats.num_keys, rreport.stats.num_slots, rreport.stats.text_bytes
        );
        println!("wrote {}", res_out.display());
    } else if res_out.is_some() {
        bail!("--res-out given without --mdd");
    }
    Ok(())
}

fn default_res_path(out: &PathBuf) -> PathBuf {
    let mut p = out.clone();
    let stem = out.file_stem().map(|s| s.to_string_lossy().into_owned());
    if let Some(stem) = stem {
        p.set_file_name(format!("{stem}.res.qmd"));
    }
    p
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
        .map(|b| String::from_utf8_lossy(b))
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

async fn cmd_serve(db: PathBuf, res: Option<PathBuf>, addr: String) -> Result<()> {
    let dict = Arc::new(Dict::open(&db).with_context(|| format!("opening {}", db.display()))?);
    let res = match res {
        Some(p) => Some(Arc::new(
            Dict::open(&p).with_context(|| format!("opening {}", p.display()))?,
        )),
        None => None,
    };
    let meta = dict.meta();
    let state = Arc::new(AppState { dict, res });
    let app = router(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    println!(
        "serving {:?} ({} entries) on http://{}",
        meta.title,
        meta.num_keys,
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
