//! Record text decoding and HTML → Markdown conversion (done at query time).
//!
//! Dictionary HTML is not ordinary prose. Five different markup vocabularies
//! appear across the corpus; line structure usually comes from `<br>` and
//! `display:block` rather than from block tags; records are pretty-printed, so
//! their indentation is not content; and they carry a good deal of web-app
//! chrome that the dictionary's own stylesheet hides. The converter here is
//! written against that data — see `role` for the class vocabulary and `filter`
//! for what gets dropped.

mod emit;
mod escape;
mod filter;
mod node;
mod role;
mod text;

pub use text::decode_record;

use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{parse_document, ParseOpts};
use markup5ever_rcdom::RcDom;

/// MDict stores some entries as a bare `@@@LINK=<target>` redirect instead of
/// HTML — 24,298 of OALD9's records, 29% of that dictionary.
/// `lookup::Dict::lookup_word` resolves them to the target entry; this only
/// handles one that survives (an unresolvable target), so the raw control string
/// never reaches the output.
const LINK_PREFIX: &str = "@@@LINK=";

fn redirect_stub(record: &str) -> Option<String> {
    let trimmed = record.trim();
    // The caller joins multi-slot records with markup, so only treat this as a
    // stub when the control string is the whole record.
    if trimmed.contains('<') {
        return None;
    }
    let target = trimmed.strip_prefix(LINK_PREFIX)?.trim();
    if target.is_empty() {
        return None;
    }
    Some(format!("→ {target}\n"))
}

/// Convert an HTML fragment to Markdown.
pub fn html_to_markdown(html: &str) -> String {
    // Records are stored NUL-terminated; `decode_record` strips that, but the CLI
    // and HTTP paths can hand over anything.
    let html = html.trim_end_matches('\0');
    if let Some(stub) = redirect_stub(html) {
        return stub;
    }
    let dom = parse_document(RcDom::default(), ParseOpts::default()).one(StrTendril::from(html));
    let mut writer = emit::Writer::new(html.len());
    writer.run(&dom.document);
    emit::normalize(&writer.finish())
}

#[cfg(test)]
mod tests;
