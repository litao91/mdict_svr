//! Deciding which parts of a record are content and which are web-app chrome.

use super::node::El;

/// Tags whose subtree is never content.
///
/// Dropped globally, not only under `<head>`: col repeats a full
/// `<head><link><script src><meta></head>` block mid-record after an `<hr/>`, and
/// html5ever ignores a `<head>` start tag in body while still inserting its
/// children there. OALD9 records *end* with `<link rel=stylesheet href=...>`
/// inside the body.
pub fn dropped_tag(tag: &str) -> bool {
    matches!(
        tag,
        "script"
            | "style"
            | "head"
            | "meta"
            | "link"
            | "base"
            | "title"
            | "noscript"
            | "iframe"
            | "object"
            | "embed"
            | "template"
            | "canvas"
            | "svg"
            | "button"
            | "input"
            | "select"
            | "textarea"
            | "option"
            | "frame"
            | "frameset"
            | "applet"
            | "param"
    )
}

/// Web-app chrome observed in the corpus. Each is either `display:none` in the
/// dictionary's own stylesheet or a control that does nothing outside a browser:
/// `data/OALD9.css:171-174` hides `.responsive_display_inline_on_smartphone`
/// ("jump to other results"), `div[class="btn icon-left-colapse wl-add"]`
/// ("Add to my wordlist"), `.pron-link` ("Check pronunciation:") and
/// `.ox-enlarge-label` ("enlarge image"). OxEnCh8 adds a 简 toggle button and
/// ★/◆/☞/► decoration glyphs whose meaning is already carried by structure (see
/// `role::is_glyph_only`); col adds an in-page table of contents.
const CHROME_CLASSES: &[&str] = &[
    "oxford3000",
    "clear",
    "btn",
    "wl-add",
    "link-right",
    "pron-link",
    "ox-enlarge-label",
    "icon-left-colapse",
    "star",
    "do-detail",
    "search-js",
    "simplized-btn",
    "wordGroup",
    "responsive_display_inline_on_smartphone",
];

/// `hide="y"` (OALD9, 13,555x) or an inline `display:none`/`visibility:hidden`
/// (the lightbox's full-size `<img>`, OxEnCh8's `style="display:none;"`).
///
/// This is deliberately NOT a CSS engine and must not become one.
/// `data/OALD9.css:138` declares `.collapse .body{display:none}` because the real
/// app reveals those panels with a JS class toggle that no stored record
/// carries. Honoring the stylesheet would silently delete every "Word Origin",
/// "Extra examples" and "Collocations" body — the most valuable text in the
/// dictionary. Collapsed panels are expanded instead.
pub fn is_hidden(el: &El) -> bool {
    if el.attr("hide").is_some_and(is_yes) {
        return true;
    }
    el.attr("style").is_some_and(|style| {
        declarations(style).any(|(prop, val)| {
            (prop.eq_ignore_ascii_case("display") && first_token(val).eq_ignore_ascii_case("none"))
                || (prop.eq_ignore_ascii_case("visibility")
                    && first_token(val).eq_ignore_ascii_case("hidden"))
        })
    })
}

/// True when an element is explicitly block-level: either a `display` attribute
/// (OxEnCh8 writes `<span level="3" display="block" class="block-g">`) or an
/// inline style (CALD4 builds its whole entry out of
/// `<span style="margin-left:20px;display:block;">`).
pub fn is_block_display(el: &El) -> bool {
    const BLOCK: &[&str] = &["block", "list-item", "table", "flex", "grid"];
    if el.attr("display").is_some_and(|d| {
        d.split_ascii_whitespace()
            .any(|t| BLOCK.iter().any(|b| t.eq_ignore_ascii_case(b)))
    }) {
        return true;
    }
    el.attr("style").is_some_and(|style| {
        declarations(style).any(|(prop, val)| {
            prop.eq_ignore_ascii_case("display")
                && BLOCK.iter().any(|b| first_token(val).eq_ignore_ascii_case(b))
        })
    })
}

/// Split a `style` attribute into `(property, value)` pairs.
fn declarations(style: &str) -> impl Iterator<Item = (&str, &str)> {
    style.split(';').filter_map(|decl| {
        let (prop, val) = decl.split_once(':')?;
        Some((prop.trim(), val.trim()))
    })
}

/// First token of a declaration value, ignoring `!important` and whitespace.
fn first_token(val: &str) -> &str {
    val.split(|c: char| c == '!' || c.is_whitespace())
        .next()
        .unwrap_or("")
}

fn is_yes(val: &str) -> bool {
    val.split_whitespace().any(|t| t.eq_ignore_ascii_case("y"))
}

pub fn is_chrome(el: &El) -> bool {
    el.has_any_class(CHROME_CLASSES) || is_jump_anchor(el)
}

/// OALD9 tags its in-entry navigation with `nattr="head-jump-*"`:
/// `head-jump-entry-flag`/`head-jump-entry` are the empty `<a name="HASH_word">`
/// jump targets that open every record, `head-jump-entry-content` is the
/// part-of-speech link in the headword bar (which duplicates the `.pos-g` label
/// right below it, giving "noun noun"), and `head-jump-pos` is the
/// "noun | verb" bar at the top of a multi-part-of-speech entry.
///
/// `nattr` is OALD9's marker for "not a real navigational href" — its other
/// value, `bk-not-ref-href`, appears on every piece of web-app chrome and those
/// carry no `href` at all, so they are already unwrapped by `link`.
fn is_jump_anchor(el: &El) -> bool {
    el.attr("nattr")
        .is_some_and(|n| n.starts_with("head-jump"))
}

/// What to do with an `<a>`.
pub enum LinkKind {
    /// Emit the label text with no link around it.
    Unwrap,
    /// Drop the element and its subtree.
    Drop,
    /// A pronunciation/recording link; rendered as `[audio](href)`.
    Audio(String),
    Href(String),
}

const AUDIO_EXT: &[&str] = &[".mp3", ".ogg", ".spx", ".wav", ".wma", ".m4a"];

/// Classify an `<a>`.
///
/// The "no usable href → unwrap" rule is what removes essentially all OALD9
/// chrome in one stroke: every piece of it uses `nattr="bk-not-ref-href"` plus an
/// `ohref=` attribute instead of a real `href`, while every content link
/// (`a.Ref`, `a.sound`, `a.fayin`) has one.
pub fn link(el: &El) -> LinkKind {
    let Some(href) = el.attr("href").map(str::trim).filter(|h| !h.is_empty()) else {
        return LinkKind::Unwrap;
    };
    let lower = href.to_ascii_lowercase();
    if lower.starts_with("javascript:") {
        return LinkKind::Drop;
    }
    // A bare fragment, or a self-link to a fragment of the current entry. The
    // part before `#` is empty, or ends in `=`, `?`, `&` or `://` — which covers
    // both `entry://#HASH_word` (CLI) and `/lookup?word=#HASH_word` (server,
    // after `rewrite_urls`).
    if let Some(i) = href.find('#') {
        let base = &href[..i];
        if base.is_empty()
            || base.ends_with('=')
            || base.ends_with('?')
            || base.ends_with('&')
            || base.ends_with("://")
        {
            return LinkKind::Unwrap;
        }
    }
    // Root-relative paths that this server does not route: `/topic/...`,
    // `/mywordlist/`, `/definition/...`, `/pronunciation/...`, `/media/...`.
    // Only `/res/<dict>/...` and `/lookup...` resolve here.
    if href.starts_with('/')
        && !href.starts_with("/res/")
        && !href.starts_with("/lookup")
    {
        return LinkKind::Unwrap;
    }
    if lower.starts_with("chrome://") {
        return LinkKind::Unwrap;
    }
    if is_audio(el, &lower) {
        return LinkKind::Audio(href.to_owned());
    }
    LinkKind::Href(href.to_owned())
}

/// A link that plays a recording rather than navigating: OALD9's
/// `<a class="sound audio_play_button" type="sound" href="sound://...">`,
/// OxEnCh8's `<a class="fayin" type="sound">`, CALD4's
/// `<a href="sound://ukappen014.spx"><img src="snd_uk.png"></a>`, and the
/// audio-only stubs that make up the whole `Sound`/`Sound-en_GB` dictionaries.
fn is_audio(el: &El, lower_href: &str) -> bool {
    if lower_href.starts_with("sound://") || lower_href.starts_with("sounds://") {
        return true;
    }
    if el.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("sound")) {
        return true;
    }
    if el.has_any_class(&["sound", "fayin", "audio_play_button", "pron-link-audio"]) {
        return true;
    }
    let path = lower_href.split(['?', '#']).next().unwrap_or(lower_href);
    AUDIO_EXT.iter().any(|ext| path.ends_with(ext))
}
