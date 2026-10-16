//! Import: read an MDict file and pack it into a `qmd` container.

use std::path::Path;

use crate::error::Result;
use crate::lookup::normalize_key;
use crate::mdx::Mdx;
use crate::qmd::{BuildStats, Meta, QmdBuilder, KIND_HTML, KIND_RESOURCE};

/// Summary of an import, for CLI reporting.
#[derive(Debug)]
pub struct ImportReport {
    pub title: String,
    pub version: f32,
    pub encoding: String,
    pub encrypted: u8,
    pub entries: u64,
    pub stats: BuildStats,
}

fn encoding_name(m: &Mdx) -> String {
    format!("{:?}", m.header.key_encoding)
}

fn source_name<P: AsRef<Path>>(p: P) -> String {
    p.as_ref()
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Import a word dictionary (`.mdx`) into `out`.
pub fn import_dictionary<P, Q>(mdx: P, out: Q) -> Result<ImportReport>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let m = Mdx::open(&mdx)?;
    let case_sensitive = m.header.key_case_sensitive;
    let meta = Meta {
        title: m.header.title.clone(),
        description: m.header.description.clone(),
        creation_date: m.header.creation_date.clone(),
        source: source_name(&mdx),
        case_sensitive,
        ..Default::default()
    };
    let report_head = ImportReport {
        title: m.header.title.clone(),
        version: m.header.version,
        encoding: encoding_name(&m),
        encrypted: m.header.encrypted,
        entries: m.num_entries,
        stats: BuildStats {
            num_keys: 0,
            num_slots: 0,
            text_bytes: 0,
        },
    };

    let mut builder = QmdBuilder::create(out, false, meta)?;
    let mut added = 0u64;
    m.visit_records(|key, data| {
        let norm = normalize_key(key, case_sensitive, false);
        if norm.is_empty() {
            return Ok(());
        }
        builder.add(&norm, data, KIND_HTML)?;
        added += 1;
        Ok(())
    })?;
    let stats = builder.finish()?;
    tracing::info!(entries = added, "imported dictionary records");
    Ok(ImportReport {
        stats,
        ..report_head
    })
}

/// Import a resource file (`.mdd`) into `out`.
pub fn import_resources<P, Q>(mdd: P, out: Q) -> Result<ImportReport>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let m = Mdx::open(&mdd)?;
    let meta = Meta {
        title: m.header.title.clone(),
        description: m.header.description.clone(),
        creation_date: m.header.creation_date.clone(),
        source: source_name(&mdd),
        case_sensitive: false,
        ..Default::default()
    };
    let report_head = ImportReport {
        title: m.header.title.clone(),
        version: m.header.version,
        encoding: encoding_name(&m),
        encrypted: m.header.encrypted,
        entries: m.num_entries,
        stats: BuildStats {
            num_keys: 0,
            num_slots: 0,
            text_bytes: 0,
        },
    };

    let mut builder = QmdBuilder::create(out, true, meta)?;
    let mut added = 0u64;
    m.visit_records(|key, data| {
        let norm = normalize_key(key, false, true);
        if norm.is_empty() {
            return Ok(());
        }
        builder.add(&norm, data, KIND_RESOURCE)?;
        added += 1;
        Ok(())
    })?;
    let stats = builder.finish()?;
    tracing::info!(entries = added, "imported resource records");
    Ok(ImportReport {
        stats,
        ..report_head
    })
}
