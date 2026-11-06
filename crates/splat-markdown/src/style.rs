//! How text looks, and which Markdown and HTML make it look so: the one table behind the preview and both exports.

use super::doc::InlineKind;
use super::markup::Role;

/// Where text stands on its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Baseline {
    /// On the line, as running text stands.
    #[default]
    Normal,
    /// Raised, as superscript `^x^` or `<sup>`.
    Super,
    /// Lowered, as subscript `~x~` or `<sub>`.
    Sub,
}

/// The look of a piece of text. A new look needs a field here and its case in `nested_in`; each output then says what
/// the field means to it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct TextStyle {
    /// Inside strong text, `**text**` or `<strong>`.
    pub bold: bool,
    /// Inside emphasis, `*text*` or `<em>`.
    pub italic: bool,
    /// Inside struck text, `~~text~~` or `<del>`.
    pub strike: bool,
    /// Inside `<u>` or `<ins>`.
    pub underline: bool,
    /// Inside marked text, `==text==` or `<mark>`.
    pub highlight: bool,
    /// Inside a code span, or `<kbd>`, `<code>`, `<samp>` or `<tt>`.
    pub code: bool,
    /// Raised or lowered by superscript or subscript, or on the line.
    pub baseline: Baseline,
    /// Inside a spoiler, `||text||`, hidden until selected.
    pub spoiler: bool,
}

impl TextStyle {
    /// The style of text inside an inline of this kind.
    pub fn nested_in(mut self, kind: &InlineKind) -> TextStyle {
        match kind {
            InlineKind::Strong => self.bold = true,
            InlineKind::Emph => self.italic = true,
            InlineKind::Strikethrough => self.strike = true,
            InlineKind::Underline => self.underline = true,
            InlineKind::Highlight => self.highlight = true,
            InlineKind::Superscript => self.baseline = Baseline::Super,
            InlineKind::Subscript => self.baseline = Baseline::Sub,
            InlineKind::Spoiler => self.spoiler = true,
            InlineKind::Kbd | InlineKind::Code(_) => self.code = true,
            InlineKind::Span
            | InlineKind::Text(_)
            | InlineKind::SoftBreak
            | InlineKind::LineBreak
            | InlineKind::Html(_)
            | InlineKind::HtmlImage { .. }
            | InlineKind::Link { .. }
            | InlineKind::Image { .. }
            | InlineKind::FootnoteRef { .. }
            | InlineKind::Math { .. }
            | InlineKind::Emoji(_)
            | InlineKind::WikiLink { .. }
            | InlineKind::Comment => {}
        }
        self
    }
}

/// The HTML element that gives text the look of an inline, where one does.
pub(crate) fn html_element(kind: &InlineKind) -> Option<&'static str> {
    Some(match kind {
        InlineKind::Emph => "em",
        InlineKind::Strong => "strong",
        InlineKind::Strikethrough => "del",
        InlineKind::Underline => "u",
        InlineKind::Highlight => "mark",
        InlineKind::Superscript => "sup",
        InlineKind::Subscript => "sub",
        InlineKind::Kbd => "kbd",
        _ => return None,
    })
}

/// The inline an HTML element stands for, when it only changes how its content looks.
pub fn styled_element(name: &str) -> Option<InlineKind> {
    match Role::of(name) {
        Role::Look(look) => Some(look.inline_kind()),
        Role::Presentational => Some(InlineKind::Span),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elements_look_like_the_markdown_they_stand_for() {
        for (name, kind) in [
            ("strong", InlineKind::Strong),
            ("b", InlineKind::Strong),
            ("em", InlineKind::Emph),
            ("del", InlineKind::Strikethrough),
            ("mark", InlineKind::Highlight),
        ] {
            let plain = TextStyle::default();
            assert_eq!(
                plain.nested_in(&styled_element(name).unwrap()),
                plain.nested_in(&kind),
                "{name}"
            );
        }
        assert!(styled_element("div").is_none());
        assert_eq!(
            TextStyle::default().nested_in(&styled_element("span").unwrap()),
            TextStyle::default()
        );
    }

    #[test]
    fn looks_add_up() {
        let both = TextStyle::default()
            .nested_in(&InlineKind::Strong)
            .nested_in(&InlineKind::Emph);
        assert!(both.bold && both.italic && !both.code);
    }
}
