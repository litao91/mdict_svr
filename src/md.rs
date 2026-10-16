//! Record text decoding and HTML → Markdown conversion (done at query time).

/// Decode a stored record's bytes into text.
///
/// MDict dictionaries vary: modern v2.0 dictionaries store UTF-8 HTML, while many
/// v1.x dictionaries (e.g. Collins COBUILD) store UTF-16LE. Rather than trust the
/// header, sniff the bytes: an explicit BOM wins, otherwise a run of ASCII-with-
/// NUL-high-bytes indicates UTF-16LE. Falls back to UTF-8.
pub fn decode_record(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return utf16le(&bytes[2..]);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return utf16be(&bytes[2..]);
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if looks_like_utf16le(bytes) {
        return utf16le(bytes);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// True when the first code units look like ASCII encoded as UTF-16LE, i.e. most
/// high bytes are zero. HTML records start with markup/whitespace, so this is a
/// reliable discriminator against UTF-8 (which has no interleaved NULs).
fn looks_like_utf16le(bytes: &[u8]) -> bool {
    let units = (bytes.len() / 2).min(16);
    if units == 0 {
        return false;
    }
    let zeros = (0..units).filter(|&i| bytes[2 * i + 1] == 0).count();
    zeros * 4 >= units * 3
}

fn utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn utf16be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

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
    }

    #[test]
    fn decodes_utf8_record() {
        assert_eq!(decode_record("<b>hi</b>".as_bytes()), "<b>hi</b>");
    }

    #[test]
    fn decodes_utf16le_record() {
        let utf16: Vec<u8> = "<b>hi</b>"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(decode_record(&utf16), "<b>hi</b>");
    }

    #[test]
    fn decodes_bom_prefixed_records() {
        let mut le = vec![0xFF, 0xFE];
        le.extend("x".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode_record(&le), "x");

        let mut be = vec![0xFE, 0xFF];
        be.extend("x".encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(decode_record(&be), "x");

        let mut u8bom = vec![0xEF, 0xBB, 0xBF];
        u8bom.extend_from_slice(b"y");
        assert_eq!(decode_record(&u8bom), "y");
    }
}
