//! What an element says about the look of what it holds, by its name, class and inline style. Looks from style sheets
//! count only in what Cocoa writes, which keeps strikethrough and fonts there; Excel's describe cells as nobody sees them.

use std::collections::HashMap;
use std::sync::Arc;

use crate::rich::{Mark, Marks, is_monospace_font};

pub(super) type Attributes = [(String, String)];

/// The value of an attribute, whose name `read_tag` gave in lower case.
pub(super) fn attr<'a>(attrs: &'a Attributes, name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// A Word list paragraph's place, as `mso-list:l0 level2 lfo1` says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WordList {
    pub(super) level: usize,
    pub(super) list: u64,
}

/// What is in force where the text stands; closing an element brings back what it found.
#[derive(Clone, Debug, Default)]
pub(super) struct Style {
    pub(super) marks: Marks,
    pub(super) code: bool,
    pub(super) link: Option<Arc<str>>,
    pub(super) heading: Option<u8>,
    pub(super) word_list: Option<WordList>,
    /// The text is a Word list's bullet or number, which stands in no paragraph.
    pub(super) in_marker: bool,
    pub(super) language: Option<Arc<str>>,
    /// The paragraph's left margin in points.
    pub(super) indent: Option<f32>,
    /// The paragraph's first line starts left of the others, as a list item's marker does.
    pub(super) hanging: bool,
    /// The spaces Word puts behind a list's bullet or number, which its filtered pages mark in no other way.
    pub(super) marker_spaces: bool,
    /// The text is a Word footnote's reference, by the note's anchor.
    pub(super) note_reference: Option<Arc<str>>,
    /// The text is the number in front of a Word footnote, which leads back to its reference.
    pub(super) note_number: bool,
    /// The blocks are a Word footnote's, by its anchor.
    pub(super) note: Option<Arc<str>>,
}

/// The rules of a style sheet for classes, as `p.p1 {…}` or `.s1 {…}` give them: by class, the element they are for and
/// their declarations.
#[derive(Debug, Default)]
pub(super) struct Sheet {
    rules: HashMap<String, Vec<(Option<String>, String)>>,
}

impl Sheet {
    /// Takes in the rules of `css` that name one class and at most an element.
    pub(super) fn read(&mut self, css: &str) {
        let mut rest = css;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}').map(|close| open + close) else {
                return;
            };
            let declarations = rest[open + 1..close].trim().to_ascii_lowercase();
            for selector in rest[..open].split(',') {
                let selector = selector.trim().trim_start_matches("<!--").trim();
                if let Some((element, class)) = selector.split_once('.')
                    && !class.is_empty()
                    && class
                        .bytes()
                        .chain(element.bytes())
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                {
                    let element = (!element.is_empty()).then(|| element.to_ascii_lowercase());
                    self.rules
                        .entry(class.to_string())
                        .or_default()
                        .push((element, declarations.clone()));
                }
            }
            rest = &rest[close + 1..];
        }
    }

    /// The declarations the classes of an element called `name` give it.
    pub(super) fn declarations(&self, name: &str, attrs: &Attributes) -> String {
        let mut css = String::new();
        if self.rules.is_empty() {
            return css;
        }
        for class in attr(attrs, "class").unwrap_or("").split_ascii_whitespace() {
            for (element, declarations) in self.rules.get(class).into_iter().flatten() {
                if element.as_deref().is_none_or(|element| element == name) {
                    css.push_str(declarations);
                    css.push(';');
                }
            }
        }
        css
    }
}

impl Style {
    /// What is in force inside an element called `name`.
    pub(super) fn inside(&self, name: &str, attrs: &Attributes, sheet: &Sheet) -> Style {
        let mut style = self.clone();
        style.apply_element(name, attrs);
        if let Some(class) = attr(attrs, "class") {
            style.apply_class(class);
        }
        let from_sheet = sheet.declarations(name, attrs);
        if !from_sheet.is_empty() {
            style.apply_css(name, &from_sheet);
        }
        if let Some(css) = attr(attrs, "style") {
            style.apply_css(name, css);
        }
        style
    }

    fn mark(&mut self, mark: Mark, on: bool) {
        self.marks = self.marks.with(mark, on);
    }

    fn apply_element(&mut self, name: &str, attrs: &Attributes) {
        match name {
            "b" | "strong" => self.mark(Mark::Bold, true),
            "i" | "em" | "cite" | "dfn" | "var" => self.mark(Mark::Italic, true),
            "u" | "ins" => self.mark(Mark::Underline, true),
            "s" | "del" | "strike" => self.mark(Mark::Strike, true),
            "mark" => self.mark(Mark::Highlight, true),
            "sup" => self.mark(Mark::Superscript, true),
            "sub" => self.mark(Mark::Subscript, true),
            "code" | "kbd" | "tt" | "samp" => self.code = true,
            "font" => self.code |= attr(attrs, "face").is_some_and(is_monospace_font),
            "a" => {
                if let Some(href) = attr(attrs, "href").map(str::trim) {
                    match href.strip_prefix("#_").and_then(word_note) {
                        Some(WordNote::Reference(note)) => self.note_reference = Some(note.into()),
                        Some(WordNote::Back) => self.note_number = true,
                        None => self.link = Some(href.into()),
                    }
                }
            }
            "div" => {
                if let Some(Some(WordNote::Reference(note))) = attr(attrs, "id").map(word_note) {
                    self.note = Some(note.into());
                }
            }
            "h1" => self.heading = Some(1),
            "h2" => self.heading = Some(2),
            "h3" => self.heading = Some(3),
            "h4" => self.heading = Some(4),
            "h5" => self.heading = Some(5),
            "h6" => self.heading = Some(6),
            _ => {}
        }
    }

    fn apply_class(&mut self, class: &str) {
        for token in class.split_ascii_whitespace() {
            if let Some(level) = token.strip_prefix("MsoHeading").and_then(|n| n.parse::<u8>().ok()) {
                self.heading = Some(level.clamp(1, 6));
            }
            if let Some(language) = language_in(token) {
                self.language = Some(language.into());
            }
        }
    }

    fn apply_css(&mut self, name: &str, css: &str) {
        let css = css.to_ascii_lowercase();
        for (property, value) in declarations(&css) {
            match property {
                "font-weight" => {
                    if let Some(bold) = is_bold(value) {
                        self.mark(Mark::Bold, bold);
                    }
                }
                "font-style" => match value {
                    "italic" | "oblique" => self.mark(Mark::Italic, true),
                    "normal" => self.mark(Mark::Italic, false),
                    _ => {}
                },
                "text-decoration" | "text-decoration-line" => self.apply_decoration(value),
                "vertical-align" => match value {
                    "super" => self.mark(Mark::Superscript, true),
                    "sub" => self.mark(Mark::Subscript, true),
                    "baseline" => {
                        self.mark(Mark::Superscript, false);
                        self.mark(Mark::Subscript, false);
                    }
                    _ => {}
                },
                "font-family" if matches!(name, "span" | "font") => self.code |= is_monospace_font(value),
                "font" if name == "span" => {
                    self.marker_spaces = is_marker_spaces(value);
                    self.code |= is_monospace_font(value);
                }
                "margin-left" if is_paragraph(name) => self.indent = points(value),
                "text-indent" if is_paragraph(name) => self.hanging = points(value).is_some_and(|indent| indent < 0.0),
                "mso-list" if value == "ignore" => self.in_marker = true,
                "mso-list" if matches!(name, "p" | "div") => self.word_list = word_list(value).or(self.word_list),
                _ => {}
            }
        }
    }

    fn apply_decoration(&mut self, value: &str) {
        for word in value.split_ascii_whitespace() {
            match word {
                "underline" => self.mark(Mark::Underline, true),
                "line-through" => self.mark(Mark::Strike, true),
                "none" => {
                    self.mark(Mark::Underline, false);
                    self.mark(Mark::Strike, false);
                }
                _ => {}
            }
        }
    }
}

fn declarations(css: &str) -> impl Iterator<Item = (&str, &str)> {
    css.split(';').filter_map(|declaration| {
        let (property, value) = declaration.split_once(':')?;
        Some((property.trim(), value.trim().trim_end_matches("!important").trim_end()))
    })
}

/// Google Docs wraps everything it copies in `<b style="font-weight:normal">`, so that normal has to take bold away.
fn is_bold(value: &str) -> Option<bool> {
    match value {
        "bold" | "bolder" => Some(true),
        "normal" | "lighter" => Some(false),
        _ => value.parse::<u32>().ok().map(|weight| weight >= 600),
    }
}

/// Whether an element's inline style makes it what `<pre>` is, as the HTML editors such as VS Code copy.
pub(super) fn is_preformatted(name: &str, attrs: &Attributes) -> bool {
    if !matches!(name, "div" | "p") {
        return false;
    }
    let Some(css) = attr(attrs, "style") else {
        return false;
    };
    let css = css.to_ascii_lowercase();
    let mut pre = false;
    let mut mono = false;
    for (property, value) in declarations(&css) {
        match property {
            "white-space" => pre = value == "pre",
            "font-family" => mono = is_monospace_font(value),
            _ => {}
        }
    }
    pre && mono
}

/// The language a class token names.
fn language_in(token: &str) -> Option<&str> {
    ["language-", "lang-", "highlight-source-"]
        .iter()
        .find_map(|prefix| token.strip_prefix(prefix))
        .filter(|language| !language.is_empty())
}

/// The language the classes of an element name.
pub(super) fn language_of(attrs: &Attributes) -> Option<Arc<str>> {
    attr(attrs, "class")?
        .split_ascii_whitespace()
        .find_map(language_in)
        .map(Into::into)
}

/// What a Word anchor such as `ftn1`, `ftnref1` or `edn2` is to its footnote or endnote.
#[derive(Debug, PartialEq, Eq)]
enum WordNote<'a> {
    /// The note itself, or a reference to it, by the anchor that names the note.
    Reference(&'a str),
    /// The way back from the note to its reference.
    Back,
}

fn word_note(anchor: &str) -> Option<WordNote<'_>> {
    let kind = ["ftn", "edn"].into_iter().find(|kind| anchor.starts_with(kind))?;
    let rest = &anchor[kind.len()..];
    let (back, number) = rest.strip_prefix("ref").map_or((false, rest), |number| (true, number));
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(if back {
        WordNote::Back
    } else {
        WordNote::Reference(anchor)
    })
}

fn is_paragraph(name: &str) -> bool {
    matches!(name, "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

/// Whether a `font` declaration is the one Word gives the spaces behind a list's bullet or number.
fn is_marker_spaces(font: &str) -> bool {
    font.starts_with("7.0pt") && font.contains("times new roman")
}

/// A CSS length in points.
fn points(length: &str) -> Option<f32> {
    let split = length.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(length.len());
    let number: f32 = length[..split].trim().parse().ok()?;
    let per_unit = match &length[split..] {
        "pt" => 1.0,
        "in" => 72.0,
        "cm" => 72.0 / 2.54,
        "mm" => 72.0 / 25.4,
        "pc" => 12.0,
        "px" => 0.75,
        "" if number == 0.0 => 1.0,
        _ => return None,
    };
    Some(number * per_unit).filter(|points| points.is_finite())
}

fn word_list(value: &str) -> Option<WordList> {
    let mut level = None;
    let (mut list, mut override_number) = (0u64, 0u64);
    for token in value.split_ascii_whitespace() {
        if let Some(n) = token.strip_prefix("level").and_then(|n| n.parse::<usize>().ok()) {
            level = Some(n.saturating_sub(1).min(8));
        } else if let Some(n) = token.strip_prefix("lfo").and_then(|n| n.parse().ok()) {
            override_number = n;
        } else if let Some(n) = token.strip_prefix('l').and_then(|n| n.parse().ok()) {
            list = n;
        }
    }
    level.map(|level| WordList {
        level,
        list: list.saturating_mul(1 << 20).saturating_add(override_number),
    })
}
