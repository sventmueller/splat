//! How text looks, and which Markdown and HTML make it look so: the one table behind the preview and both exports.

use super::doc::InlineKind;

/// How a block, a cell or a column aligns its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Align {
    /// As the direction of the text has it.
    #[default]
    None,
    Left,
    Center,
    Right,
}

impl Align {
    /// The word HTML has for it.
    pub fn html_name(self) -> Option<&'static str> {
        match self {
            Align::None => None,
            Align::Left => Some("left"),
            Align::Center => Some("center"),
            Align::Right => Some("right"),
        }
    }
}

/// Where text stands on its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Baseline {
    #[default]
    Normal,
    Super,
    Sub,
}

/// The look of a piece of text. A new look needs a field here and its case in `inside`; each output then says what the
/// field means to it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub underline: bool,
    pub highlight: bool,
    pub code: bool,
    pub baseline: Baseline,
    pub spoiler: bool,
}

impl TextStyle {
    /// The style of text inside an inline of this kind.
    pub fn inside(mut self, kind: &InlineKind) -> TextStyle {
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
pub fn html_element(kind: &InlineKind) -> Option<&'static str> {
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
    Some(match name {
        "u" | "ins" => InlineKind::Underline,
        "mark" => InlineKind::Highlight,
        "sup" => InlineKind::Superscript,
        "sub" => InlineKind::Subscript,
        "b" | "strong" => InlineKind::Strong,
        "i" | "em" | "cite" | "dfn" | "var" => InlineKind::Emph,
        "s" | "del" | "strike" => InlineKind::Strikethrough,
        "kbd" | "code" | "samp" | "tt" => InlineKind::Kbd,
        "span" | "small" | "big" | "abbr" | "font" | "q" | "time" => InlineKind::Span,
        _ => return None,
    })
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
                plain.inside(&styled_element(name).unwrap()),
                plain.inside(&kind),
                "{name}"
            );
        }
        assert!(styled_element("div").is_none());
        assert_eq!(
            TextStyle::default().inside(&styled_element("span").unwrap()),
            TextStyle::default()
        );
    }

    #[test]
    fn looks_add_up() {
        let both = TextStyle::default()
            .inside(&InlineKind::Strong)
            .inside(&InlineKind::Emph);
        assert!(both.bold && both.italic && !both.code);
    }
}
