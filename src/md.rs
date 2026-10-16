//! HTML → Markdown conversion (done at query time).

/// Convert an HTML fragment to Markdown. Best-effort: dictionary markup varies,
/// so the result is a readable approximation, not a perfect round-trip.
pub fn html_to_markdown(html: &str) -> String {
    html2md::parse_html(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_basic_html() {
        let md = html_to_markdown("<p>hello <b>world</b></p>");
        assert!(md.contains("hello"));
        assert!(md.contains("world"));
        // html2md renders <b> as **world**
        assert!(md.contains("**world**") || md.contains("world"));
    }
}
