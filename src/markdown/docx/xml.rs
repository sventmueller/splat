//! The parts of WordprocessingML Splat writes, as values: Word rejects elements in another order than its schema's, so
//! only `to_xml` decides the order.

use std::fmt::Write;

use crate::markdown::doc::InlineKind;
use crate::markdown::style::{Align, Baseline, TextStyle};

/// A hairline above or below a paragraph.
#[derive(Clone, Copy)]
pub(super) enum Line {
    Top,
    Bottom,
}

/// What a paragraph sets of `w:pPr`, in twentieths of a point where a size is meant.
#[derive(Clone, Default)]
pub(super) struct ParaProps {
    pub(super) style: Option<String>,
    /// Numbering id and level.
    pub(super) list: Option<(usize, usize)>,
    pub(super) line: Option<Line>,
    pub(super) bidi: bool,
    pub(super) before: Option<u32>,
    pub(super) after: Option<u32>,
    /// A fixed line height.
    pub(super) exact_line: Option<u32>,
    pub(super) indent: Option<usize>,
    pub(super) justify: Option<Align>,
}

impl ParaProps {
    pub(super) fn styled(style: &str) -> ParaProps {
        ParaProps {
            style: Some(style.to_string()),
            ..ParaProps::default()
        }
    }

    pub(super) fn is_code(&self) -> bool {
        self.style.as_deref() == Some("Code")
    }

    pub(super) fn to_xml(&self) -> String {
        let mut out = String::new();
        if let Some(style) = &self.style {
            let _ = write!(out, "<w:pStyle w:val=\"{style}\"/>");
        }
        if let Some((id, level)) = self.list {
            // Word's numbering knows nine levels.
            let level = level.min(8);
            let _ = write!(
                out,
                "<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{id}\"/></w:numPr>"
            );
        }
        match self.line {
            Some(Line::Top) => {
                out.push_str("<w:pBdr><w:top w:val=\"single\" w:sz=\"4\" w:space=\"1\" w:color=\"D2D2D7\"/></w:pBdr>")
            }
            Some(Line::Bottom) => out
                .push_str("<w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"D2D2D7\"/></w:pBdr>"),
            None => {}
        }
        if self.bidi {
            out.push_str("<w:bidi/>");
        }
        if self.before.is_some() || self.after.is_some() || self.exact_line.is_some() {
            out.push_str("<w:spacing");
            if let Some(before) = self.before {
                let _ = write!(out, " w:before=\"{before}\"");
            }
            if let Some(after) = self.after {
                let _ = write!(out, " w:after=\"{after}\"");
            }
            if let Some(line) = self.exact_line {
                let _ = write!(out, " w:line=\"{line}\" w:lineRule=\"exact\"");
            }
            out.push_str("/>");
        }
        if let Some(indent) = self.indent {
            let _ = write!(out, "<w:ind w:left=\"{indent}\"/>");
        }
        match self.justify {
            Some(Align::Left) => out.push_str("<w:jc w:val=\"left\"/>"),
            Some(Align::Center) => out.push_str("<w:jc w:val=\"center\"/>"),
            Some(Align::Right) => out.push_str("<w:jc w:val=\"right\"/>"),
            Some(Align::None) | None => {}
        }
        out
    }
}

/// How a piece of text looks.
#[derive(Clone, Copy, Default)]
pub(super) struct Props {
    pub(super) text: TextStyle,
    pub(super) tag: bool,
    pub(super) link: bool,
}

impl Props {
    pub(super) fn inside(self, kind: &InlineKind) -> Props {
        Props {
            text: self.text.inside(kind),
            ..self
        }
    }

    pub(super) fn linked(self) -> Props {
        Props { link: true, ..self }
    }

    pub(super) fn bold(self, bold: bool) -> Props {
        Props {
            text: TextStyle { bold, ..self.text },
            ..self
        }
    }

    pub(super) fn code(self) -> Props {
        Props {
            text: TextStyle {
                code: true,
                ..self.text
            },
            ..self
        }
    }

    pub(super) fn superscript(self) -> Props {
        Props {
            text: TextStyle {
                baseline: Baseline::Super,
                ..self.text
            },
            ..self
        }
    }

    fn to_xml(self) -> String {
        let TextStyle {
            bold,
            italic,
            strike,
            underline,
            highlight,
            code,
            baseline,
            spoiler,
        } = self.text;
        let mut out = String::new();
        // A run has one character style; a link in code keeps the code's font and takes the link's look directly.
        let linked_code = self.link && code;
        if code {
            out.push_str("<w:rStyle w:val=\"CodeChar\"/>");
        } else if self.link {
            out.push_str("<w:rStyle w:val=\"Hyperlink\"/>");
        }
        for (on, tag) in [
            (bold, "<w:b/>"),
            (italic, "<w:i/>"),
            (strike, "<w:strike/>"),
            (spoiler, "<w:color w:val=\"1D1D1F\"/>"),
            (self.tag && !spoiler, "<w:color w:val=\"0066CC\"/>"),
            (linked_code && !spoiler && !self.tag, "<w:color w:val=\"0563C1\"/>"),
            (highlight, "<w:highlight w:val=\"yellow\"/>"),
            (underline || linked_code, "<w:u w:val=\"single\"/>"),
            (spoiler, "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"1D1D1F\"/>"),
            (baseline == Baseline::Super, "<w:vertAlign w:val=\"superscript\"/>"),
            (baseline == Baseline::Sub, "<w:vertAlign w:val=\"subscript\"/>"),
        ] {
            if on {
                out.push_str(tag);
            }
        }
        out
    }
}

/// The content of a paragraph, with the text the reader sees, which decides its direction.
#[derive(Default)]
pub(super) struct Runs {
    xml: String,
    text: String,
}

impl Runs {
    pub(super) fn is_empty(&self) -> bool {
        self.xml.is_empty()
    }

    pub(super) fn xml(&self) -> &str {
        &self.xml
    }

    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn push(&mut self, text: &str, props: Props) {
        let _ = write!(
            self.xml,
            "<w:r><w:rPr>{}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
            props.to_xml(),
            escape_xml(text)
        );
        self.text.push_str(text);
    }

    pub(super) fn line_break(&mut self) {
        self.xml.push_str("<w:r><w:br/></w:r>");
    }

    pub(super) fn picture(&mut self, drawing: &str) {
        self.xml.push_str(drawing);
    }

    /// `inner` as a link with the relationship `id`.
    pub(super) fn hyperlink(&mut self, id: usize, inner: Runs) {
        let _ = write!(self.xml, "<w:hyperlink r:id=\"rIdL{id}\">{}</w:hyperlink>", inner.xml);
        self.text.push_str(&inner.text);
    }

    pub(super) fn append(&mut self, other: Runs) {
        self.xml.push_str(&other.xml);
        self.text.push_str(&other.text);
    }
}

/// Escapes text for XML and drops the control characters XML 1.0 does not allow.
pub(super) fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraph_properties_come_in_the_order_of_the_schema() {
        let all = ParaProps {
            style: Some("Quote".into()),
            list: Some((2, 1)),
            line: Some(Line::Bottom),
            bidi: true,
            before: Some(160),
            after: Some(0),
            exact_line: Some(160),
            indent: Some(720),
            justify: Some(Align::Center),
        }
        .to_xml();
        let order = ["w:pStyle", "w:numPr", "w:pBdr", "w:bidi", "w:spacing", "w:ind", "w:jc"];
        let places: Vec<usize> = order.iter().map(|tag| all.find(tag).unwrap()).collect();
        assert!(places.is_sorted(), "{all}");
        assert!(all.contains("<w:spacing w:before=\"160\" w:after=\"0\" w:line=\"160\" w:lineRule=\"exact\"/>"));
        assert_eq!(ParaProps::default().to_xml(), "");
    }

    #[test]
    fn a_run_has_one_character_style() {
        let linked_code = Props::default().code().linked().to_xml();
        assert_eq!(linked_code.matches("w:rStyle").count(), 1, "{linked_code}");
        assert!(linked_code.contains("CodeChar") && linked_code.contains("<w:u w:val=\"single\"/>"));
    }

    #[test]
    fn runs_keep_the_text_the_reader_sees() {
        let mut runs = Runs::default();
        runs.push("a < b", Props::default());
        runs.line_break();
        assert_eq!(runs.text(), "a < b");
        assert!(runs.xml().contains("a &lt; b"));
    }

    #[test]
    fn a_link_alone_needs_no_look_of_its_own() {
        assert!(!Props::default().linked().to_xml().contains("<w:u "));
        assert!(!Props::default().code().to_xml().contains("<w:u "));
        let code_link = Props::default().code().linked();
        assert!(code_link.to_xml().contains("<w:color w:val=\"0563C1\"/>"));
        let tag = Props { tag: true, ..code_link };
        assert!(
            !tag.to_xml().contains("0563C1") && tag.to_xml().contains("0066CC"),
            "the tag's color wins"
        );
        let spoiler = Props {
            text: TextStyle {
                spoiler: true,
                ..code_link.text
            },
            ..code_link
        };
        assert!(!spoiler.to_xml().contains("0563C1"), "a hidden link looks hidden");
    }

    #[test]
    fn superscript_is_all_a_superscript_adds() {
        let plain = Props::default().bold(true);
        let up = plain.superscript();
        assert!(up.to_xml().contains("<w:vertAlign w:val=\"superscript\"/>") && up.to_xml().contains("<w:b/>"));
    }

    #[test]
    fn runs_are_empty_until_something_stands_in_them() {
        let mut runs = Runs::default();
        assert!(runs.is_empty());
        runs.line_break();
        assert_eq!(runs.xml(), "<w:r><w:br/></w:r>");
        assert!(!runs.is_empty());
    }

    #[test]
    fn characters_xml_cannot_hold_are_left_out() {
        assert_eq!(escape_xml("a\u{1}b\u{1f}\u{FFFE}c\u{FFFF}"), "abc");
        assert_eq!(escape_xml("a\u{20}\u{FFFD}\t"), "a \u{FFFD}\t");
    }
}
