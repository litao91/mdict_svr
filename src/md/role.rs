//! Mapping an element to what it means in Markdown.
//!
//! Two layers, applied in order: generic HTML semantics, then a small set of
//! class names. The class names come from the three class-based vocabularies in
//! the corpus — OALD9 (Oxford Dictionaries Online), OxEnCh8 (OALD8C) and col
//! (Youdao Collins). The other dictionaries (CALD4, COBUILD8, oxfordPrimary,
//! XinNiujin, Sound) carry no classes at all and are handled entirely by the
//! generic layer, which keys off `<br>` and `display:block`.
//!
//! There is deliberately no "which dictionary is this" detection: the class
//! vocabularies are disjoint, and a classifier would only add a way to be wrong.

use super::filter::{self, LinkKind};
use super::node::El;
use markup5ever_rcdom::NodeData;

/// What the renderer should do with an element.
pub enum Action {
    /// Drop the element and its subtree.
    Skip,
    /// Drop the subtree but leave a space where it was, because the original
    /// separated it from its neighbours with a CSS margin.
    Spacer,
    /// Render children with no wrapper of its own.
    Inline,
    /// Paragraph-level container: flush before and after.
    Block,
    /// Inline emphasis. Suppressed inside a heading.
    Mark(&'static str, &'static str),
    /// Paragraph-level container whose inline content is bolded — OALD9's
    /// `.idm`, which puts the idiom itself in bold on its own line.
    BoldBlock,
    Heading(u8),
    LineBreak,
    Rule,
    Image { src: String, alt: String },
    Link { href: String, audio: bool },
    List { ordered: bool },
    /// A list item. `bullet` forces `- ` even inside an ordered list.
    Item { bullet: bool },
    /// A lead-in label that pulls the following block up onto its own line:
    /// OALD9's `<span class="num">1 </span>` sense number and col's
    /// `<span class="collinsOrder">1</span>`.
    LeadIn { bold: bool },
    /// An inline element that the original separates with CSS margins rather
    /// than with whitespace, so the stored markup has none and the words would
    /// otherwise run together.
    Pad { before: bool, after: bool },
    /// A table row: a block whose cells are joined by `" | "`.
    Row,
    /// A table cell, separated from its siblings by `" | "`.
    Cell,
    /// XinNiujin's `<frac><nu>1</nu><dn>299 792 458</dn></frac>`, which must
    /// render as `1/299 792 458` — transparent rendering would give the
    /// misleading `1299 792 458`.
    Fraction,
}

/// Ancestor scopes that change how a class is read.
#[derive(Default, Clone, Copy)]
pub struct Ctx {
    /// Inside OALD9's `.pron-gs`. `data/OALD9.css:76` declares
    /// `.pron-gs .wrap{display:none}`, so `.wrap` there is the *hidden* copy of
    /// the slash and `.separator` is the visible one. Outside `.pron-gs`,
    /// `.wrap` supplies the real parentheses of `.label-g`.
    pub pron_gs: u32,
}

/// Block-level HTML tags. `<td>`/`<th>`/`<li>` are handled separately.
fn is_block_tag(tag: &str) -> bool {
    matches!(
        tag,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "body"
            | "center"
            | "colgroup"
            | "dd"
            | "details"
            | "dialog"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hgroup"
            | "html"
            | "legend"
            | "main"
            | "nav"
            | "p"
            | "pre"
            | "section"
            | "summary"
            | "table"
            | "tbody"
            | "tfoot"
            | "thead"
            | "tr"
    )
}

/// Classes that must become paragraph-level even though they are on a `<span>`.
///
/// OxEnCh8 builds entire entries out of `<span>`s, so without this its senses,
/// examples and sections would run together into one paragraph. Kept
/// deliberately narrow: the generic layer already makes every `<div>` and `<p>`
/// a block, and turning an *inline* element into a block fragments text that
/// belongs on one line. Notably `.def`, `.d`, `.gram` and `.label-g` stay
/// inline — OALD9's Word Origin is
/// `<span class="def"><span class="lang">Old English</span></span> æppel, of ...`,
/// where blocking `.def` would split "Old English" from the etymology it belongs
/// to. The surrounding blocks flush it instead.
const BLOCK_CLASSES: &[&str] = &[
    // OALD9
    "entry",
    "h-g",
    "top-container",
    "top-g",
    "webtop-g",
    "ei-g",
    "sn-gs",
    "sn-g",
    "idm-gs",
    "idm-g",
    "res-g",
    "xr-gs",
    // OALD9's Word Family box: adjacent `.wfw-g` rows with no whitespace
    // between them, which would otherwise run together into
    // "able adjective (≠unable)ably adverbability noun".
    "wfw-g",
    "unbox",
    "collapse",
    "body",
    "vp-gs",
    "vp-g",
    "pv-gs",
    "pv-g",
    // OxEnCh8 (spans throughout)
    "oald8c",
    "def-g",
    "ids-g",
    "id-g",
    "sense-g",
    "block-g",
    "infl",
    "para",
    "und",
    "help",
    "chn",
    "tx",
    "table",
    // col (Youdao Collins)
    "tab-content",
    "trans-container",
    "trans-content",
    "collapse-content",
    "wt-container",
    "collinsMajorTrans",
    "exampleLists",
    "examples",
];

pub fn action(el: &El, ctx: &Ctx) -> Action {
    let tag = el.tag;

    if filter::dropped_tag(tag) || filter::is_hidden(el) || filter::is_chrome(el) {
        return Action::Skip;
    }

    match tag {
        "br" => return Action::LineBreak,
        "hr" => return Action::Rule,
        "img" => return image(el),
        "a" => return anchor(el),
        "b" | "strong" => return Action::Mark("**", "**"),
        "i" | "em" => return Action::Mark("*", "*"),
        "s" | "del" | "strike" => return Action::Mark("~~", "~~"),
        // No Markdown equivalent, and `~x~` is strikethrough in GFM — so
        // `<sub>`/`<sup>` degrade to plain text rather than to wrong semantics.
        // CALD4's `<u>Thesaurus</u><sup>+</sup>` is a section label.
        "u" | "ins" | "sub" | "sup" | "font" | "span" | "small" | "big" | "tt" | "abbr"
        | "acronym" | "cite" | "var" | "kbd" | "samp" | "label" | "nobr" | "time" | "mark"
        | "data" | "output" | "bdi" | "bdo" | "wbr" | "ruby" | "rt" | "rp" => {}
        "h1" => return Action::Heading(1),
        "h2" => return Action::Heading(2),
        "h3" => return Action::Heading(3),
        "h4" => return Action::Heading(4),
        "h5" => return Action::Heading(5),
        "h6" => return Action::Heading(6),
        "ul" | "ol" => {
            // OALD9 wraps whole entries in `<ol class="h-g">` / `<ol class="idm-g">`
            // whose children are `div`/`span`, not `li`. Numbering those would
            // emit a run of empty `1.` `2.` markers.
            return if el.has_child_element("li") {
                Action::List { ordered: tag == "ol" }
            } else {
                Action::Block
            };
        }
        "li" => {
            // OALD9 marks each sense `<li class="sn-g">` and numbers it with a
            // `<span class="num">`, so a bullet would be redundant.
            return if el.has_class("sn-g") {
                Action::Block
            } else {
                Action::Item { bullet: false }
            };
        }
        "tr" => return Action::Row,
        "td" | "th" => return Action::Cell,
        "frac" => return Action::Fraction,
        _ => {}
    }

    class_action(el, ctx).unwrap_or_else(|| {
        if is_block_tag(tag) || filter::is_block_display(el) {
            Action::Block
        } else {
            Action::Inline
        }
    })
}

/// Class-driven roles. `None` means "fall through to the generic rule".
fn class_action(el: &El, ctx: &Ctx) -> Option<Action> {
    // OALD9 duplicates every pronunciation slash: `.separator` and `.wrap` each
    // hold one `/`. `data/OALD9.css:76` hides `.pron-gs .wrap`, so `.separator`
    // is the visible copy. Scoped to `.pron-gs` because `.label-g > .wrap`
    // supplies the real parentheses around "(saying, especially North American
    // English)".
    if ctx.pron_gs > 0 && el.has_class("wrap") {
        return Some(Action::Skip);
    }
    // `data/OALD9.css:38` — `.bre`/`.name` repeat the BrE/NAmE label that the
    // sibling `.prefix` already supplies, giving "BrE BrE//ˈæpl//".
    if el.has_any_class(&["bre", "name"]) {
        return Some(Action::Skip);
    }
    // OxEnCh8 decorates its entries with glyphs — ★ marks an Oxford-3000 core
    // word, ◆ an example, ☞ a cross-reference, ► a note. The structure already
    // carries that meaning, so the glyph goes. It becomes a spacer rather than
    // being dropped outright because the CSS margin around it was doing real
    // work: `<span class="z_n">1.</span><span class="symbols-small_coresym">★</span><span class="gr">[intransitive]</span>`
    // would otherwise glue into "1.[intransitive]". The `symbols-*` spans that
    // hold a word (SYN, OPP, HELP, NOTE) are kept and padded below.
    if el.classes().any(|c| c.starts_with("symbols-")) && is_glyph_only(el) {
        return Some(Action::Spacer);
    }
    // OxEnCh8's headword. Its `.h` sibling carries the same word with syllable
    // dots ("ba·nana" for `.no-dot-h`'s "banana"), so one of them has to go;
    // OALD9's `.h` is on a real `<h2>`, which the tag rule above already caught.
    if el.has_class("no-dot-h") {
        return Some(Action::Heading(2));
    }
    if el.has_class("h") {
        return Some(Action::Skip);
    }

    for class in el.classes() {
        let action = match class {
            // Section headings: OALD9's `.heading` ("Word Origin", "Extra
            // examples", "Idioms"), OxEnCh8's `.unbox > .title` and `.revout`
            // ("IDIOMS", "PHRASAL VERBS"), col's `.title.trans-tip`
            // ("VERB USES"). The renderer downgrades these to plain text when
            // already inside a heading, so col's
            // `<h4><span class="title">apple</span>` does not nest.
            "heading" | "title" | "revout" => Action::Heading(3),
            // The idiom or phrasal verb itself, rendered bold on its own line.
            "idm" | "id" => Action::BoldBlock,
            // OALD9's `<span class="num">1 </span>` sense number. The stylesheet
            // hides it (`data/OALD9.css:76`) and regenerates it with a CSS
            // counter (line 95), which Markdown has no equivalent for — the
            // stored text is the real number, so keep it.
            "num" => Action::LeadIn { bold: false },
            "collinsOrder" => Action::LeadIn { bold: true },
            "x-gs" => Action::List { ordered: false },
            "x-g" => Action::Item { bullet: true },
            "tr" => Action::Row,
            "td" | "th" => Action::Cell,
            // OxEnCh8 separates these with margins, not whitespace: without
            // padding its pronunciation renders as
            // `/ˈæpl[audio](…);NAmEˈæpl[audio](…)/`, its labels glue onto the
            // definition (`(especially NAmE)a child usually behaves…`) and its
            // grammar frames glue onto their neighbours
            // (`[transitive]take sb/sth`, `noun[uncountable]`,
            // `go through with(= not to have)the operation`,
            // `(figurative)Her life had always run smoothly`).
            "z_phon-us" | "d" | "ud" | "cf" | "gr" | "gl" | "label-g" => {
                Action::Pad { before: true, after: true }
            }
            // `.z` is a punctuation separator sitting flush against the word
            // before it (`<a>verb</a><span class="z">,</span>`) with the gap
            // supplied by CSS on the right, so pad only after. Padding both
            // sides would turn OxEnCh8's cross-reference list into
            // `[crab apple] , [eating apple]`.
            "z" => Action::Pad { before: false, after: true },
            // The surviving `symbols-*` classes are word labels (SYN, OPP, HELP,
            // NOTE) sitting flush against the text they introduce.
            _ if class.starts_with("symbols-") => Action::Pad { before: true, after: true },
            _ => {
                if BLOCK_CLASSES.contains(&class) {
                    Action::Block
                } else {
                    continue;
                }
            }
        };
        return Some(action);
    }
    None
}

/// True when an element's own text is nothing but punctuation or symbols.
///
/// Only ever called on `symbols-*` spans, which in OxEnCh8 are single-character
/// leaves (★ ◆ ☞ ►). The 64-byte bound keeps this O(1) even if some dictionary
/// puts a real subtree behind a `symbols-` class, in which case it is treated as
/// content and kept.
fn is_glyph_only(el: &El) -> bool {
    let mut text = String::new();
    for c in el.node.children.borrow().iter() {
        if let NodeData::Text { contents } = &c.data {
            text.push_str(contents.borrow().as_ref());
            if text.len() > 64 {
                return false;
            }
        }
    }
    let trimmed = text.trim();
    !trimmed.is_empty() && !trimmed.chars().any(char::is_alphanumeric)
}

fn image(el: &El) -> Action {
    let src = el.attr("src").unwrap_or("").trim();
    if src.is_empty() {
        return Action::Skip;
    }
    Action::Image {
        alt: image_alt(el.attr("alt").unwrap_or(""), src),
        src: src.to_owned(),
    }
}

/// Prefer the `alt` text, but dictionaries routinely set it to the filename
/// (`alt="txbapple.jpg"`, `alt="pic/fruit_comp.jpg"`) or leave it empty, so fall
/// back to the file stem in those cases.
fn image_alt(alt: &str, src: &str) -> String {
    const IMAGE_EXT: &[&str] = &[".jpg", ".jpeg", ".png", ".gif", ".bmp", ".svg", ".webp"];
    let alt = alt.trim();
    if !alt.is_empty() && !IMAGE_EXT.iter().any(|e| alt.to_ascii_lowercase().ends_with(e)) {
        return alt.to_owned();
    }
    let base = src.rsplit(['/', '\\']).next().unwrap_or(src);
    base.rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(base)
        .to_owned()
}

fn anchor(el: &El) -> Action {
    match filter::link(el) {
        LinkKind::Drop => Action::Skip,
        LinkKind::Unwrap => Action::Inline,
        LinkKind::Audio(href) => Action::Link { href, audio: true },
        LinkKind::Href(href) => Action::Link { href, audio: false },
    }
}
