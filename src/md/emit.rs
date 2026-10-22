//! The DOM walk that produces Markdown.
//!
//! One pass, one shared inline buffer. Block-level elements flush that buffer as
//! a paragraph; inline elements append to it. This is the piece that fixes the
//! two worst failures of the previous converter: dictionary records are
//! pretty-printed HTML whose indentation it mistook for indented code blocks,
//! and their line structure comes from `<br>` and `display:block` rather than
//! from block tags, which it collapsed into one run-on paragraph.

use markup5ever_rcdom::{Handle, NodeData};

use super::escape;
use super::node::El;
use super::role::{Action, Ctx};

/// Recursion cap. The deepest nesting measured in the corpus is 15 (OALD9
/// `hand`), so this is only insurance against a pathological dictionary.
const MAX_DEPTH: u32 = 128;

#[derive(Clone, Copy)]
struct Mark {
    token: &'static str,
    close: &'static str,
    /// Length of `buf` just after the opening token was written.
    len_at_open: usize,
    /// Whether the opening token is currently present in `buf`. A flush closes
    /// open marks and clears this; the next atom re-opens them.
    in_buf: bool,
}

enum ListState {
    Unordered,
    Ordered(usize),
}

/// One entry per open element, so `close` can undo what `open` did.
enum Frame {
    Inline,
    Block,
    Pad { after: bool },
    Mark { pushed: bool },
    BoldBlock { pushed: bool },
    Heading { saved: u8 },
    Item { saved_indent: String, saved_prefix: Option<String>, saved_blocks: usize },
    List,
    Row,
    Cell,
    Link {
        saved: String,
        saved_dirty: bool,
        saved_link_text: bool,
        href: String,
        audio: bool,
    },
}

pub struct Writer {
    out: String,
    /// The inline run currently being accumulated.
    buf: String,
    /// True once `buf` holds real content. Marks alone do not count, which is
    /// what makes `<div><b></b></div>` render as nothing instead of `****`.
    dirty: bool,
    need_space: bool,
    need_br: bool,
    marks: Vec<Mark>,
    frames: Vec<Frame>,
    lists: Vec<ListState>,
    /// Continuation indent for the current line, and the prefix for the first
    /// line of the next block (a list marker, when inside an item).
    indent: String,
    first_prefix: Option<String>,
    /// Set by a `LeadIn`; makes the next block stay on the same line.
    join_once: bool,
    cell_index: usize,
    heading: u8,
    /// Running count of blocks written to `out`, so an item can tell whether it
    /// produced exactly one and be joined tightly to the next.
    blocks_emitted: usize,
    /// Whether real text (not just an icon `<img>`) has been emitted inside the
    /// link currently being rendered.
    link_text: bool,
}

impl Writer {
    pub fn new(capacity: usize) -> Writer {
        Writer {
            out: String::with_capacity(capacity / 2),
            buf: String::new(),
            dirty: false,
            need_space: false,
            need_br: false,
            marks: Vec::new(),
            frames: Vec::new(),
            lists: Vec::new(),
            indent: String::new(),
            first_prefix: None,
            join_once: false,
            cell_index: 0,
            heading: 0,
            blocks_emitted: 0,
            link_text: false,
        }
    }

    pub fn run(&mut self, doc: &Handle) {
        self.run_children(doc, &Ctx::default(), 0);
    }

    pub fn finish(mut self) -> String {
        self.flush();
        self.out
    }

    fn run_children(&mut self, h: &Handle, ctx: &Ctx, depth: u32) {
        // Clone the child handles (Rc clones) rather than holding the RefCell
        // borrow across the recursion.
        let children: Vec<Handle> = h.children.borrow().iter().cloned().collect();
        for c in children {
            self.run_node(&c, ctx, depth);
        }
    }

    fn run_node(&mut self, h: &Handle, ctx: &Ctx, depth: u32) {
        match &h.data {
            NodeData::Text { contents } => {
                let borrowed = contents.borrow();
                self.text(&borrowed);
            }
            // Comments (`<!-- End of DIV ... -->`, 848,851 of them in OALD9),
            // doctypes and processing instructions carry no content.
            NodeData::Element { .. } => self.run_element(h, ctx, depth),
            _ => {}
        }
    }

    fn run_element(&mut self, h: &Handle, ctx: &Ctx, depth: u32) {
        if depth >= MAX_DEPTH {
            return;
        }
        let (action, child_ctx) = {
            // Scoped so the attribute borrow is dropped before recursing.
            let Some(el) = El::new(h) else { return };
            (super::role::action(&el, ctx), next_ctx(&el, ctx))
        };
        match action {
            Action::Skip => {}
            // One flag is enough: it separates whatever came before from
            // whatever comes next, and `pending` discards it if the buffer is
            // empty, so a spacer at the start of a block adds no leading space.
            Action::Spacer => self.need_space = true,
            Action::Fraction => self.fraction(h, &child_ctx, depth),
            action => {
                self.open(action);
                self.run_children(h, &child_ctx, depth + 1);
                self.close();
            }
        }
    }

    /// `<frac><nu>1</nu><dn>299 792 458</dn></frac>` → `1/299 792 458`.
    fn fraction(&mut self, h: &Handle, ctx: &Ctx, depth: u32) {
        let children: Vec<Handle> = h.children.borrow().iter().cloned().collect();
        for c in children {
            let is_denominator = El::new(&c).is_some_and(|el| el.tag == "dn");
            if is_denominator {
                self.emit_atom("/");
            }
            self.run_node(&c, ctx, depth + 1);
        }
    }

    fn open(&mut self, action: Action) {
        match action {
            // Unreachable: `run_element` handles these before calling `open`.
            Action::Skip | Action::Spacer | Action::Fraction => {
                self.frames.push(Frame::Inline)
            }
            Action::Inline => self.frames.push(Frame::Inline),
            Action::Block => {
                if self.join_once {
                    self.join_once = false;
                    self.need_space = true;
                } else {
                    self.flush();
                }
                self.frames.push(Frame::Block);
            }
            Action::BoldBlock => {
                if self.join_once {
                    self.join_once = false;
                    self.need_space = true;
                } else {
                    self.flush();
                }
                let pushed = self.push_mark("**", "**");
                self.frames.push(Frame::BoldBlock { pushed });
            }
            Action::Mark(token, close) => {
                let pushed = self.push_mark(token, close);
                self.frames.push(Frame::Mark { pushed });
            }
            Action::LeadIn { bold } => {
                let pushed = if bold { self.push_mark("**", "**") } else { false };
                self.frames.push(Frame::Mark { pushed });
                self.join_once = true;
            }
            Action::Heading(level) => {
                // Never nest headings: col puts `<span class="title">apple</span>`
                // inside an `<h4>`, and a nested heading would split the line.
                if self.heading > 0 {
                    self.frames.push(Frame::Inline);
                    return;
                }
                self.flush();
                let saved = self.heading;
                self.heading = level;
                self.frames.push(Frame::Heading { saved });
            }
            Action::LineBreak => {
                self.need_br = true;
                self.frames.push(Frame::Inline);
            }
            Action::Rule => {
                self.flush();
                self.out.push_str("---\n\n");
                self.frames.push(Frame::Inline);
            }
            Action::Image { src, alt } => {
                // Pad: CALD4 and OxEnCh8 put images straight against the
                // surrounding text with no whitespace, relying on CSS margins.
                self.need_space = true;
                self.emit_atom(&format!("![{}]({})", escape::text(&alt), escape::link_target(&src)));
                self.need_space = true;
                self.frames.push(Frame::Inline);
            }
            Action::Link { href, audio } => {
                if audio {
                    self.need_space = true;
                }
                self.pending();
                self.reopen_marks();
                // Render the label into a scratch buffer so an image-only label
                // can be replaced wholesale by `[audio](...)`.
                let saved = std::mem::take(&mut self.buf);
                let saved_dirty = std::mem::replace(&mut self.dirty, false);
                let saved_link_text = std::mem::replace(&mut self.link_text, false);
                self.need_space = false;
                self.need_br = false;
                self.frames.push(Frame::Link {
                    saved,
                    saved_dirty,
                    saved_link_text,
                    href,
                    audio,
                });
            }
            Action::Pad { before, after } => {
                if before {
                    self.need_space = true;
                }
                self.frames.push(Frame::Pad { after });
            }
            Action::List { ordered } => {
                self.flush();
                self.lists.push(if ordered { ListState::Ordered(1) } else { ListState::Unordered });
                self.frames.push(Frame::List);
            }
            Action::Item { bullet } => {
                self.flush();
                let marker = if bullet { "- ".to_owned() } else { self.next_marker() };
                let saved_indent = self.indent.clone();
                let saved_prefix = self.first_prefix.replace(format!("{saved_indent}{marker}"));
                let saved_blocks = self.blocks_emitted;
                self.indent = format!("{saved_indent}{}", " ".repeat(marker.chars().count()));
                self.frames.push(Frame::Item { saved_indent, saved_prefix, saved_blocks });
            }
            Action::Row => {
                self.flush();
                self.cell_index = 0;
                self.frames.push(Frame::Row);
            }
            Action::Cell => {
                if self.cell_index > 0 {
                    self.emit_atom(" | ");
                }
                self.cell_index += 1;
                self.frames.push(Frame::Cell);
            }
        }
    }

    fn close(&mut self) {
        match self.frames.pop() {
            None | Some(Frame::Inline) => {}
            Some(Frame::Pad { after }) => {
                if after {
                    self.need_space = true;
                }
            }
            Some(Frame::Block) => self.flush(),
            Some(Frame::Mark { pushed }) => {
                if pushed {
                    self.pop_mark();
                }
            }
            Some(Frame::BoldBlock { pushed }) => {
                if pushed {
                    self.pop_mark();
                }
                self.flush();
            }
            Some(Frame::Heading { saved }) => {
                let level = self.heading;
                self.heading = saved;
                self.close_heading(level);
            }
            Some(Frame::Item { saved_indent, saved_prefix, saved_blocks }) => {
                self.flush();
                // A single-paragraph item joins the next one with a single
                // newline, so example lists stay tight rather than becoming a
                // loose list with a blank line between every entry.
                if self.blocks_emitted - saved_blocks == 1 && self.out.ends_with("\n\n") {
                    self.out.pop();
                }
                self.indent = saved_indent;
                self.first_prefix = saved_prefix;
            }
            Some(Frame::List) => {
                self.lists.pop();
                self.flush();
                // Undo the last item's tightening so whatever follows the list
                // is not swallowed into it as a lazy continuation line.
                if !self.out.is_empty() && !self.out.ends_with("\n\n") {
                    self.out.push('\n');
                }
            }
            Some(Frame::Row) => {
                self.flush();
                self.cell_index = 0;
            }
            Some(Frame::Cell) => {}
            Some(Frame::Link { saved, saved_dirty, saved_link_text, href, audio }) => {
                let raw_label = std::mem::replace(&mut self.buf, saved);
                let label = raw_label.trim();
                let has_text = std::mem::replace(&mut self.link_text, saved_link_text);
                // Whitespace inside the label must not turn into a separator
                // *before* the link — `open` already wrote the real one. OALD9's
                // `a.sound` is padded with `&nbsp;`, which otherwise doubles it.
                self.need_space = false;
                self.need_br = false;
                // An audio link whose label is only an icon (`snd_uk.png`,
                // `uk_pron.png`) becomes `[audio](…)`. One that wraps real text —
                // oxfordPrimary links the headword itself to its recording, and
                // the whole Sound dictionary is `<a href="sound://…">英式发音apple</a>`
                // — keeps that text.
                let rendered = if audio && !has_text {
                    Some(format!("[audio]({})", escape::link_target(&href)))
                } else if label.is_empty() {
                    None
                } else {
                    Some(format!("[{}]({})", label, escape::link_target(&href)))
                };
                match rendered {
                    Some(r) => {
                        self.reopen_marks();
                        self.buf.push_str(&r);
                        self.dirty = true;
                        if audio {
                            self.need_space = true;
                        }
                    }
                    None => self.dirty = saved_dirty,
                }
            }
        }
    }

    // ---- inline accumulation -------------------------------------------------

    /// Append a text node, whitespace-collapsed.
    fn text(&mut self, raw: &str) {
        let mut core = String::with_capacity(raw.len());
        let mut lead_ws = false;
        let mut pending_space = false;
        let mut saw_ws = false;
        for c in raw.chars() {
            match c {
                // Zero-width and soft-hyphen characters carry no visible width.
                // OALD9 alone has 103,505 U+200B, including inside headwords.
                '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{00ad}' | '\u{feff}' => {}
                // `char::is_whitespace` covers NBSP, thin/em/ideographic space
                // and the run of tabs col pads every record with.
                c if c.is_whitespace() => {
                    saw_ws = true;
                    if core.is_empty() {
                        lead_ws = true;
                    } else {
                        pending_space = true;
                    }
                }
                c => {
                    if pending_space {
                        core.push(' ');
                        pending_space = false;
                    }
                    core.push(c);
                }
            }
        }
        if core.is_empty() {
            // A whitespace-only run still separates the atoms around it. This is
            // what keeps OxEnCh8's `<span class="smart-blank"> </span>` from
            // gluing its inflections into "gowentgoinggone".
            if saw_ws {
                self.need_space = true;
            }
            return;
        }
        if lead_ws {
            self.need_space = true;
        }
        let escaped = escape::text(&core);
        self.emit_atom(&escaped);
        self.link_text = true;
        if pending_space {
            self.need_space = true;
        }
    }

    fn emit_atom(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.pending();
        self.reopen_marks();
        self.buf.push_str(s);
        self.dirty = true;
    }

    /// Write the separator owed to the next atom. A no-op on an empty buffer, so
    /// a leading or trailing `<br>` produces no blank line.
    fn pending(&mut self) {
        if self.buf.is_empty() {
            self.need_space = false;
            self.need_br = false;
            return;
        }
        if self.need_br {
            self.buf.push_str("  \n");
        } else if self.need_space {
            self.buf.push(' ');
        }
        self.need_space = false;
        self.need_br = false;
    }

    fn reopen_marks(&mut self) {
        for m in self.marks.iter_mut() {
            if !m.in_buf {
                self.buf.push_str(m.token);
                m.len_at_open = self.buf.len();
                m.in_buf = true;
            }
        }
    }

    /// Open inline emphasis, unless we are inside a heading (where `*/rʌn/*`
    /// reads as noise rather than as formatting).
    fn push_mark(&mut self, token: &'static str, close: &'static str) -> bool {
        if self.heading > 0 {
            return false;
        }
        self.pending();
        self.reopen_marks();
        self.buf.push_str(token);
        self.marks.push(Mark { token, close, len_at_open: self.buf.len(), in_buf: true });
        true
    }

    fn pop_mark(&mut self) {
        let Some(m) = self.marks.pop() else { return };
        if !m.in_buf {
            // A flush already closed it and nothing has been emitted since.
            return;
        }
        if self.buf.len() > m.len_at_open {
            self.buf.push_str(m.close);
        } else {
            // Nothing was emitted between open and close: drop the marker too,
            // so `<b></b>` and `<span class="star"></span>` leave no trace.
            self.buf.truncate(m.len_at_open.saturating_sub(m.token.len()));
        }
    }

    // ---- block emission -----------------------------------------------------

    fn flush(&mut self) {
        self.need_space = false;
        self.need_br = false;
        if !self.dirty {
            self.buf.clear();
            for m in self.marks.iter_mut() {
                m.in_buf = false;
            }
            return;
        }
        for m in self.marks.iter_mut().rev() {
            if m.in_buf {
                self.buf.push_str(m.close);
                m.in_buf = false;
            }
        }
        // Take the buffer so `line_starts` can borrow it while `emit_block`
        // borrows `self` mutably.
        let raw = std::mem::take(&mut self.buf);
        let block = escape::line_starts(&raw);
        self.emit_block(&block);
        self.dirty = false;
    }

    fn close_heading(&mut self, level: u8) {
        self.need_space = false;
        self.need_br = false;
        for m in self.marks.iter_mut().rev() {
            if m.in_buf {
                self.buf.push_str(m.close);
                m.in_buf = false;
            }
        }
        if self.dirty {
            let text = self.buf.trim();
            if !text.is_empty() {
                let level = level.clamp(1, 6);
                self.out.push_str(&"#".repeat(level as usize));
                self.out.push(' ');
                self.out.push_str(text);
                self.out.push_str("\n\n");
            }
        }
        self.buf.clear();
        self.dirty = false;
    }

    fn emit_block(&mut self, block: &str) {
        let prefix = match self.first_prefix.take() {
            Some(p) => p,
            None => self.indent.clone(),
        };
        self.out.push_str(&prefix);
        let mut lines = block.split('\n');
        if let Some(first) = lines.next() {
            self.out.push_str(first);
        }
        for line in lines {
            self.out.push('\n');
            self.out.push_str(&self.indent);
            self.out.push_str(line);
        }
        self.out.push_str("\n\n");
        self.blocks_emitted += 1;
    }

    fn next_marker(&mut self) -> String {
        match self.lists.last_mut() {
            Some(ListState::Ordered(n)) => {
                let marker = format!("{n}. ");
                *n += 1;
                marker
            }
            // An `<li>` whose nearest list ancestor is missing — OALD9 puts
            // `<li class="sn-g">` inside a `<span class="sn-gs">`, and html5ever
            // does not synthesise a `<ul>` for it.
            _ => "- ".to_owned(),
        }
    }
}

fn next_ctx(el: &El, ctx: &Ctx) -> Ctx {
    let mut next = *ctx;
    if el.has_class("pron-gs") {
        next.pron_gs += 1;
    }
    next
}

/// Collapse blank-line runs and strip leading/trailing blank lines.
///
/// Deliberately does *not* right-trim each line: a two-space hard break is how
/// `<br>` survives, and `<br>` is the primary line separator in CALD4, COBUILD8,
/// oxfordPrimary and XinNiujin.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_nl = 0usize;
    for c in s.chars() {
        if c == '\n' {
            pending_nl += 1;
            continue;
        }
        if !out.is_empty() {
            for _ in 0..pending_nl.min(2) {
                out.push('\n');
            }
        }
        pending_nl = 0;
        out.push(c);
    }
    if out.is_empty() {
        return String::new();
    }
    out.push('\n');
    out
}
