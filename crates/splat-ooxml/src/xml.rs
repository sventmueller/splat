//! Text written into XML, without the characters XML 1.0 cannot hold.

/// `text` as the content of an element. Tabs and line breaks stay as they are, which turns a carriage return into a
/// line feed for a reader; quotation marks are escaped too, so the result also fits a double-quoted attribute, where a
/// reader takes tabs and line breaks for spaces.
pub fn escape_text(text: &str) -> String {
    escaped(text, false)
}

/// `text` as a double-quoted attribute value, with tabs and line breaks as character references, since XML reads them
/// in an attribute as spaces.
pub fn escape_attribute(text: &str) -> String {
    escaped(text, true)
}

fn escaped(text: &str, attribute: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' if attribute => out.push_str("&#9;"),
            '\n' if attribute => out.push_str("&#10;"),
            '\r' if attribute => out.push_str("&#13;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if c < ' ' || matches!(c, '\u{FFFE}' | '\u{FFFF}') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_is_escaped_in_text_and_attributes() {
        assert_eq!(escape_text("a<b>&\"c'"), "a&lt;b&gt;&amp;&quot;c'");
        assert_eq!(escape_attribute("a<b>&\"c'"), "a&lt;b&gt;&amp;&quot;c'");
    }

    #[test]
    fn characters_xml_cannot_hold_are_left_out() {
        assert_eq!(escape_text("a\u{1}b\u{1f}\u{FFFE}c\u{FFFF}"), "abc");
        assert_eq!(escape_text("a\u{20}\u{FFFD}\t"), "a \u{FFFD}\t");
        assert_eq!(escape_attribute("a\u{0}b"), "ab");
    }

    #[test]
    fn white_space_survives_in_an_attribute_and_stays_as_it_is_in_text() {
        assert_eq!(escape_text("a\tb\nc\rd"), "a\tb\nc\rd");
        assert_eq!(escape_attribute("a\tb\nc\rd"), "a&#9;b&#10;c&#13;d");
    }
}
