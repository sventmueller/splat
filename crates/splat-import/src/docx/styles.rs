//! `word/styles.xml` as what a paragraph or a run gets from its style. Style ids are localized (`berschrift1`), so only
//! names say what a style is.

use std::collections::HashMap;

use roxmltree::{Document, Node};

use super::package::{W, child_w, is_on, is_w, w_attr, w_val};
use crate::rich::is_monospace_font;

/// How far `w:basedOn` is followed.
const LONGEST_CHAIN: usize = 64;

/// A list a paragraph belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ListPlace {
    pub(super) id: u64,
    pub(super) level: Option<usize>,
}

impl ListPlace {
    /// What a `w:numPr` says.
    pub(super) fn read(properties: Node) -> Option<ListPlace> {
        let number = child_w(properties, "numPr")?;
        let id = child_w(number, "numId")?;
        Some(ListPlace {
            id: w_val(id)?.parse().ok()?,
            level: child_w(number, "ilvl")
                .and_then(w_val)
                .and_then(|level| level.parse().ok()),
        })
    }
}

/// What `w:rPr` says about a look, with `None` for what it leaves to what stands around it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RunProps {
    pub(super) bold: Option<bool>,
    pub(super) italic: Option<bool>,
    pub(super) strike: Option<bool>,
    pub(super) underline: Option<bool>,
    pub(super) highlight: Option<bool>,
    pub(super) superscript: Option<bool>,
    pub(super) subscript: Option<bool>,
    pub(super) monospace: Option<bool>,
    pub(super) hidden: Option<bool>,
}

impl RunProps {
    /// What the `w:rPr` element `properties` says.
    pub(super) fn read(properties: Node) -> RunProps {
        let mut props = RunProps::default();
        for child in properties.children().filter(|node| node.is_element()) {
            if child.tag_name().namespace() != Some(W) {
                continue;
            }
            match child.tag_name().name() {
                "b" => props.bold = Some(is_on(child)),
                "i" => props.italic = Some(is_on(child)),
                "strike" | "dstrike" => props.strike = Some(is_on(child) || props.strike == Some(true)),
                "u" | "highlight" => {
                    let on = !matches!(w_val(child), None | Some("none"));
                    if child.tag_name().name() == "u" {
                        props.underline = Some(on);
                    } else {
                        props.highlight = Some(on);
                    }
                }
                "vertAlign" => {
                    props.superscript = Some(w_val(child) == Some("superscript"));
                    props.subscript = Some(w_val(child) == Some("subscript"));
                }
                "rFonts" => {
                    if let Some(font) = w_attr(child, "ascii").or_else(|| w_attr(child, "hAnsi")) {
                        props.monospace = Some(is_monospace_font(font));
                    }
                }
                "vanish" | "webHidden" => props.hidden = Some(is_on(child)),
                _ => {}
            }
        }
        props
    }

    /// These properties with what `inherited` adds where they say nothing.
    fn over(self, inherited: RunProps) -> RunProps {
        RunProps {
            bold: self.bold.or(inherited.bold),
            italic: self.italic.or(inherited.italic),
            strike: self.strike.or(inherited.strike),
            underline: self.underline.or(inherited.underline),
            highlight: self.highlight.or(inherited.highlight),
            superscript: self.superscript.or(inherited.superscript),
            subscript: self.subscript.or(inherited.subscript),
            monospace: self.monospace.or(inherited.monospace),
            hidden: self.hidden.or(inherited.hidden),
        }
    }
}

/// What a style comes to, with all it is based on.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Style {
    pub(super) heading: Option<u8>,
    /// The heading level of a title or subtitle.
    pub(super) title: Option<u8>,
    pub(super) quote: bool,
    pub(super) code: bool,
    pub(super) inline_code: bool,
    pub(super) toc: bool,
    pub(super) footnote_text: bool,
    pub(super) list: Option<ListPlace>,
    pub(super) run: RunProps,
}

fn has_word(name: &str, word: &str) -> bool {
    name.split(|c: char| !c.is_alphanumeric()).any(|part| part == word)
}

fn heading_of_name(name: &str) -> Option<u8> {
    let number: u8 = name.strip_prefix("heading ")?.trim().parse().ok()?;
    (1..=9).contains(&number).then_some(number.min(6))
}

fn heading_of_outline(level: u8) -> Option<u8> {
    (level < 9).then_some((level + 1).min(6))
}

fn is_code_name(name: &str) -> bool {
    matches!(
        name,
        "html preformatted" | "preformatted text" | "source code" | "verbatim"
    ) || has_word(name, "code")
        || name.starts_with("code")
}

fn is_inline_code_name(name: &str) -> bool {
    matches!(name, "verbatim char" | "source text" | "html code") || has_word(name, "code") || name.starts_with("code")
}

/// A `w:style` as it stands in the file.
struct Written {
    id: String,
    name: String,
    based_on: Option<String>,
    /// What `w:outlineLvl` says.
    outline: Option<u8>,
    list: Option<ListPlace>,
    run: RunProps,
    /// `w:default` on a paragraph style.
    default: bool,
}

impl Written {
    fn read(node: Node) -> Option<Written> {
        let properties = child_w(node, "pPr");
        Some(Written {
            id: w_attr(node, "styleId")?.to_string(),
            name: child_w(node, "name").and_then(w_val).unwrap_or("").to_lowercase(),
            based_on: child_w(node, "basedOn").and_then(w_val).map(str::to_string),
            outline: properties
                .and_then(|properties| child_w(properties, "outlineLvl"))
                .and_then(w_val)
                .and_then(|level| level.parse().ok()),
            list: properties.and_then(ListPlace::read),
            run: child_w(node, "rPr").map_or_else(RunProps::default, RunProps::read),
            default: w_attr(node, "type") == Some("paragraph") && w_attr(node, "default").is_some_and(|d| d != "0"),
        })
    }

    /// What this style comes to on top of what it is based on. A default style in a monospace font is the look of a
    /// whole document, as in a screenplay, which says nothing about code.
    fn resolve(&self, base: Style) -> Style {
        let name = self.name.as_str();
        let mut run = self.run.over(base.run);
        if self.default {
            run.monospace = None;
        }
        Style {
            heading: heading_of_name(name)
                .or_else(|| self.outline.and_then(heading_of_outline))
                .or(if self.outline.is_some() { None } else { base.heading }),
            title: match name {
                "title" => Some(1),
                "subtitle" => Some(2),
                _ => base.title,
            },
            quote: matches!(
                name,
                "quote" | "intense quote" | "block quote" | "blockquote" | "block text"
            ) || base.quote,
            code: is_code_name(name) || base.code,
            inline_code: is_inline_code_name(name) || base.inline_code,
            toc: name == "table of contents"
                || name == "toc heading"
                || name.strip_prefix("toc ").is_some_and(|n| n.parse::<u8>().is_ok())
                || base.toc,
            footnote_text: name == "footnote text" || base.footnote_text,
            list: self.list.or(base.list),
            run,
        }
    }
}

/// The styles of a document.
#[derive(Debug, Default)]
pub(super) struct Styles {
    resolved: HashMap<String, Style>,
    default_paragraph: Style,
}

impl Styles {
    /// The styles in a `word/styles.xml`.
    pub(super) fn read(document: &Document) -> Styles {
        let written: Vec<Written> = document
            .root_element()
            .children()
            .filter(|node| is_w(*node, "style"))
            .filter_map(Written::read)
            .collect();
        let by_id: HashMap<&str, &Written> = written.iter().map(|style| (style.id.as_str(), style)).collect();
        let mut resolved: HashMap<String, Style> = HashMap::with_capacity(written.len());
        for style in &written {
            resolve_chain(style, &by_id, &mut resolved);
        }
        let default_paragraph = written
            .iter()
            .find(|style| style.default)
            .and_then(|style| resolved.get(&style.id))
            .copied()
            .unwrap_or_default();
        Styles {
            resolved,
            default_paragraph,
        }
    }

    /// The style of a paragraph by what its `w:pStyle` says, the default one if there is no such style.
    pub(super) fn paragraph(&self, id: Option<&str>) -> Style {
        id.and_then(|id| self.resolved.get(id))
            .copied()
            .unwrap_or(self.default_paragraph)
    }

    /// The style a run's `w:rStyle` names.
    pub(super) fn character(&self, id: &str) -> Option<Style> {
        self.resolved.get(id).copied()
    }
}

/// Resolves a style and what it is based on, once each, so that neither a cycle nor a long chain costs anything.
fn resolve_chain<'a>(style: &'a Written, by_id: &HashMap<&str, &'a Written>, resolved: &mut HashMap<String, Style>) {
    let mut chain: Vec<&Written> = Vec::new();
    let mut at = Some(style);
    let mut base = Style::default();
    while let Some(current) = at {
        if let Some(known) = resolved.get(&current.id) {
            base = *known;
            break;
        }
        if chain.len() == LONGEST_CHAIN || chain.iter().any(|seen| seen.id == current.id) {
            break;
        }
        chain.push(current);
        at = current
            .based_on
            .as_deref()
            .and_then(|parent| by_id.get(parent).copied());
    }
    for link in chain.iter().rev() {
        base = link.resolve(base);
        resolved.insert(link.id.clone(), base);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(body: &str) -> Styles {
        let xml = format!("<w:styles xmlns:w=\"{W}\">{body}</w:styles>");
        Styles::read(&roxmltree::Document::parse(&xml).unwrap())
    }

    #[test]
    fn a_style_is_known_by_its_name_not_its_id() {
        let all = styles(
            "<w:style w:type=\"paragraph\" w:styleId=\"berschrift1\"><w:name w:val=\"heading 1\"/></w:style>\
             <w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"Text\"/></w:style>\
             <w:style w:type=\"paragraph\" w:styleId=\"h9\"><w:name w:val=\"Heading 9\"/></w:style>",
        );
        assert_eq!(all.paragraph(Some("berschrift1")).heading, Some(1));
        assert_eq!(all.paragraph(Some("Heading1")).heading, None);
        assert_eq!(all.paragraph(Some("h9")).heading, Some(6));
    }

    #[test]
    fn what_a_style_is_follows_what_it_is_based_on() {
        let all = styles(
            "<w:style w:styleId=\"a\"><w:name w:val=\"heading 2\"/><w:pPr><w:numPr><w:ilvl w:val=\"3\"/><w:numId w:val=\"7\"/></w:numPr></w:pPr><w:rPr><w:b/></w:rPr></w:style>\
             <w:style w:styleId=\"b\"><w:name w:val=\"Mine\"/><w:basedOn w:val=\"a\"/><w:rPr><w:b w:val=\"0\"/><w:i/></w:rPr></w:style>\
             <w:style w:styleId=\"c\"><w:name w:val=\"Other\"/><w:basedOn w:val=\"b\"/><w:pPr><w:outlineLvl w:val=\"9\"/></w:pPr></w:style>\
             <w:style w:styleId=\"d\"><w:name w:val=\"Outlined\"/><w:pPr><w:outlineLvl w:val=\"7\"/></w:pPr></w:style>\
             <w:style w:styleId=\"q\"><w:name w:val=\"Intense Quote\"/></w:style>\
             <w:style w:styleId=\"qq\"><w:name w:val=\"Mine\"/><w:basedOn w:val=\"q\"/></w:style>",
        );
        let b = all.paragraph(Some("b"));
        assert_eq!(b.heading, Some(2));
        assert_eq!(b.run.bold, Some(false));
        assert_eq!(b.run.italic, Some(true));
        assert_eq!(b.list, Some(ListPlace { id: 7, level: Some(3) }));
        assert_eq!(all.paragraph(Some("c")).heading, None);
        assert_eq!(all.paragraph(Some("d")).heading, Some(6));
        assert!(all.paragraph(Some("qq")).quote);
        assert!(all.character("nothing").is_none());
    }

    #[test]
    fn a_cycle_or_a_long_chain_ends() {
        let cycle = styles(
            "<w:style w:styleId=\"a\"><w:name w:val=\"heading 3\"/><w:basedOn w:val=\"b\"/></w:style>\
             <w:style w:styleId=\"b\"><w:name w:val=\"Mine\"/><w:basedOn w:val=\"a\"/></w:style>\
             <w:style w:styleId=\"me\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"me\"/></w:style>",
        );
        assert_eq!(cycle.paragraph(Some("a")).heading, Some(3));
        assert!(cycle.paragraph(Some("me")).quote);
        let long: String = (0..1000)
            .map(|n| {
                format!(
                    "<w:style w:styleId=\"s{n}\"><w:name w:val=\"x\"/><w:basedOn w:val=\"s{}\"/></w:style>",
                    n + 1
                )
            })
            .collect();
        let chain = styles(&format!(
            "{long}<w:style w:styleId=\"s1000\"><w:name w:val=\"Quote\"/></w:style>"
        ));
        assert!(!chain.paragraph(Some("s0")).quote, "as far as a chain is followed");
        assert!(chain.paragraph(Some("s1000")).quote);
    }

    #[test]
    fn the_default_style_says_nothing_about_code() {
        let all = styles(
            "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Standard\"><w:name w:val=\"Normal\"/><w:rPr><w:rFonts w:ascii=\"Courier New\"/><w:b/></w:rPr></w:style>\
             <w:style w:type=\"paragraph\" w:styleId=\"Other\"><w:name w:val=\"Other\"/><w:basedOn w:val=\"Standard\"/></w:style>\
             <w:style w:type=\"paragraph\" w:styleId=\"Plain\"><w:name w:val=\"Plain\"/><w:rPr><w:rFonts w:hAnsi=\"Consolas\"/></w:rPr></w:style>",
        );
        assert_eq!(all.paragraph(None).run.monospace, None);
        assert_eq!(all.paragraph(None).run.bold, Some(true));
        assert_eq!(all.paragraph(Some("Unknown")).run.bold, Some(true));
        assert_eq!(all.paragraph(Some("Other")).run.monospace, None);
        assert_eq!(all.paragraph(Some("Plain")).run.monospace, Some(true));
    }

    #[test]
    fn names_tell_what_a_style_is_for() {
        for name in [
            "Code",
            "Code Block",
            "CodeBlock",
            "HTML Preformatted",
            "Source Code",
            "Verbatim",
            "Preformatted Text",
        ] {
            assert!(is_code_name(&name.to_lowercase()), "{name}");
        }
        for name in ["Barcode", "Encoded", "Normal"] {
            assert!(!is_code_name(&name.to_lowercase()), "{name}");
        }
        for name in ["Code Char", "HTML Code", "Verbatim Char", "Source Text", "inline code"] {
            assert!(is_inline_code_name(&name.to_lowercase()), "{name}");
        }
        assert!(!is_inline_code_name("strong"));
        let all = styles(
            "<w:style w:styleId=\"t\"><w:name w:val=\"Title\"/></w:style>\
             <w:style w:styleId=\"s\"><w:name w:val=\"Subtitle\"/></w:style>\
             <w:style w:styleId=\"toc\"><w:name w:val=\"toc 2\"/></w:style>\
             <w:style w:styleId=\"th\"><w:name w:val=\"TOC Heading\"/></w:style>\
             <w:style w:styleId=\"tc\"><w:name w:val=\"Table of Contents\"/></w:style>\
             <w:style w:styleId=\"not\"><w:name w:val=\"toc x\"/></w:style>\
             <w:style w:styleId=\"f\"><w:name w:val=\"footnote text\"/></w:style>\
             <w:style w:name=\"no id\"/>",
        );
        assert_eq!(all.paragraph(Some("t")).title, Some(1));
        assert_eq!(all.paragraph(Some("s")).title, Some(2));
        assert!(all.paragraph(Some("toc")).toc && all.paragraph(Some("th")).toc && all.paragraph(Some("tc")).toc);
        assert!(!all.paragraph(Some("not")).toc);
        assert!(all.paragraph(Some("f")).footnote_text);
    }

    #[test]
    fn run_properties_read_what_they_say() {
        let read = |xml: &str| {
            let doc = format!("<w:rPr xmlns:w=\"{W}\" xmlns:x=\"urn:x\">{xml}</w:rPr>");
            RunProps::read(roxmltree::Document::parse(&doc).unwrap().root_element())
        };
        assert_eq!(read("<w:b/>").bold, Some(true));
        assert_eq!(read("<w:b w:val=\"false\"/>").bold, Some(false));
        assert_eq!(read("<w:i w:val=\"off\"/>").italic, Some(false));
        assert_eq!(read("<w:dstrike/>").strike, Some(true));
        assert_eq!(read("<w:strike/><w:dstrike w:val=\"0\"/>").strike, Some(true));
        assert_eq!(read("<w:u w:val=\"double\"/>").underline, Some(true));
        assert_eq!(read("<w:u w:val=\"none\"/>").underline, Some(false));
        assert_eq!(read("<w:u/>").underline, Some(false));
        assert_eq!(read("<w:highlight w:val=\"red\"/>").highlight, Some(true));
        assert_eq!(read("<w:highlight w:val=\"none\"/>").highlight, Some(false));
        let raised = read("<w:vertAlign w:val=\"superscript\"/>");
        assert_eq!((raised.superscript, raised.subscript), (Some(true), Some(false)));
        let lowered = read("<w:vertAlign w:val=\"subscript\"/>");
        assert_eq!((lowered.superscript, lowered.subscript), (Some(false), Some(true)));
        assert_eq!(read("<w:rFonts w:cs=\"Menlo\"/>").monospace, None);
        assert_eq!(read("<w:rFonts w:ascii=\"Arial\"/>").monospace, Some(false));
        assert_eq!(read("<w:vanish/>").hidden, Some(true));
        assert_eq!(read("<w:webHidden/>").hidden, Some(true));
        assert_eq!(read("<x:b/><w:sz w:val=\"3\"/>"), RunProps::default());
    }
}
