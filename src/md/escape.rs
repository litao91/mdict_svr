//! Markdown escaping.
//!
//! Deliberately minimal. Over-escaping was one of the worst things the previous
//! converter did — it turned an ordinary hyphen into `\-`, which renders
//! correctly but reads as noise, and dictionary text is full of hyphens.

use std::borrow::Cow;

/// Escape characters that would be read as inline markup.
///
/// `[` and `]` are left alone on purpose: CommonMark only treats them as a link
/// when a `](` or a matching reference definition follows, and dictionary text
/// is full of literal brackets that must stay readable (`[C]`, `[U]`, `[T]`,
/// `[intr]`).
pub fn text(s: &str) -> Cow<'_, str> {
    if s.find(['\\', '*', '_', '`', '<']).is_none() {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '<') {
            out.push('\\');
        }
        out.push(c);
    }
    Cow::Owned(out)
}

/// Escape constructs that are only meaningful at the start of a line, so a
/// definition that happens to begin with `1.` or `-` is not reinterpreted as a
/// list. Applied per line, because a block can contain hard breaks.
pub fn line_starts(block: &str) -> Cow<'_, str> {
    let risky = block.split('\n').any(|line| {
        matches!(
            line.as_bytes().first(),
            Some(b'#') | Some(b'>') | Some(b'|') | Some(b'-') | Some(b'+') | Some(b'0'..=b'9')
        )
    });
    if !risky {
        return Cow::Borrowed(block);
    }
    let mut out = String::with_capacity(block.len() + 8);
    for (i, line) in block.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        escape_line_start(line, &mut out);
    }
    Cow::Owned(out)
}

fn escape_line_start(line: &str, out: &mut String) {
    let bytes = line.as_bytes();
    match bytes.first() {
        None => {}
        // `>foo` is a blockquote even without the space, and `|` starts a table row.
        Some(b'>') | Some(b'|') => {
            out.push('\\');
            out.push_str(line);
        }
        Some(b'#') | Some(b'-') | Some(b'+') => {
            if bytes.len() == 1 || bytes[1].is_ascii_whitespace() {
                out.push('\\');
            }
            out.push_str(line);
        }
        Some(b) if b.is_ascii_digit() => escape_ordered_marker(line, bytes, out),
        _ => out.push_str(line),
    }
}

/// `1.` / `1)` followed by whitespace or end-of-line is an ordered-list marker.
/// Escape the punctuation, not the digits, so the number stays readable —
/// XinNiujin numbers its senses `<b>1. </b>`, COBUILD8 numbers them `1)`.
fn escape_ordered_marker(line: &str, bytes: &[u8], out: &mut String) {
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    let marker = bytes.get(digits).copied();
    if !matches!(marker, Some(b'.') | Some(b')')) {
        out.push_str(line);
        return;
    }
    // `digits + 1` is a valid char boundary: the marker is ASCII.
    let after = &line[digits + 1..];
    if after.is_empty() || after.starts_with(char::is_whitespace) {
        out.push_str(&line[..digits]);
        out.push('\\');
        out.push(marker.unwrap() as char);
        out.push_str(after);
    } else {
        out.push_str(line);
    }
}

/// Escape a link destination so a space, pipe or parenthesis in it cannot break
/// out of the `(...)` — the corpus has `hand 2.jpg`, `file:///p|/ald8/htm/x.jpg`
/// and `entry://Adam's apple`.
///
/// Non-ASCII is left literal: CJK is valid in a CommonMark destination and far
/// more readable than `%E8%98%8B`.
pub fn link_target(url: &str) -> Cow<'_, str> {
    if url
        .chars()
        .all(|c| !matches!(c, ' ' | '(' | ')' | '|' | '<' | '>' | '"' | '\'' | '\t' | '\n' | '\r') && (c as u32) >= 0x20 && c != '\u{7f}')
    {
        return Cow::Borrowed(url);
    }
    let mut out = String::with_capacity(url.len() + 8);
    for c in url.chars() {
        let encoded = match c {
            ' ' => Some("%20"),
            '(' => Some("%28"),
            ')' => Some("%29"),
            '|' => Some("%7C"),
            '<' => Some("%3C"),
            '>' => Some("%3E"),
            '"' => Some("%22"),
            '\'' => Some("%27"),
            _ => None,
        };
        match encoded {
            Some(seq) => out.push_str(seq),
            _ if (c as u32) < 0x20 || c == '\u{7f}' => percent_encode_bytes(c, &mut out),
            _ => out.push(c),
        }
    }
    Cow::Owned(out)
}

fn percent_encode_bytes(c: char, out: &mut String) {
    let mut buf = [0u8; 4];
    for b in c.encode_utf8(&mut buf).as_bytes() {
        out.push('%');
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 0xf) as usize]);
    }
}

const HEX: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F',
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_inline_markup() {
        assert_eq!(text("a * b _ c ` d < e \\ f"), r"a \* b \_ c \` d \< e \\ f");
        // brackets stay readable
        assert_eq!(text("[C or U]"), "[C or U]");
        // fast path when nothing needs escaping
        assert!(matches!(text("plain"), Cow::Borrowed(_)));
    }

    #[test]
    fn never_escapes_a_mid_text_hyphen() {
        assert_eq!(line_starts("well-known"), "well-known");
        assert_eq!(text("well-known"), "well-known");
    }

    #[test]
    fn escapes_line_start_constructs() {
        assert_eq!(line_starts("# not a heading"), r"\# not a heading");
        assert_eq!(line_starts("- not a bullet"), r"\- not a bullet");
        assert_eq!(line_starts("> not a quote"), r"\> not a quote");
        assert_eq!(line_starts("1) (intr) to move"), r"1\) (intr) to move");
        assert_eq!(line_starts("1. the round fruit"), r"1\. the round fruit");
        // `#hashtag` and `1.5` are not markup
        assert_eq!(line_starts("#hashtag"), "#hashtag");
        assert_eq!(line_starts("1.5 kg"), "1.5 kg");
    }

    #[test]
    fn escapes_every_line_of_a_hard_broken_block() {
        assert_eq!(line_starts("a  \n- b"), "a  \n\\- b");
    }

    #[test]
    fn escapes_link_targets() {
        assert_eq!(link_target("hand 2.jpg"), "hand%202.jpg");
        assert_eq!(link_target("file:///p|/ald8/x.jpg"), "file:///p%7C/ald8/x.jpg");
        assert_eq!(link_target("entry://Adam's apple"), "entry://Adam%27s%20apple");
        // CJK stays literal
        assert_eq!(link_target("sound://audio/苹果.mp3"), "sound://audio/苹果.mp3");
        assert!(matches!(link_target("/res/ox/a.mp3"), Cow::Borrowed(_)));
    }
}
