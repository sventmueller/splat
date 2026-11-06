//! Foreign formats as Markdown and Word files read and merged: pure functions over bytes and strings, safe on whatever
//! input arrives. The app turns the errors into text and writes the files; it needs no window and no macOS.

#![forbid(unsafe_code)]
#![warn(missing_docs, unreachable_pub)]

pub mod docx;
pub mod html;
mod package;
pub mod rich;
pub mod rtf;
pub mod tabular;
pub mod template;
mod xml;
pub mod zip;

use rich::{Block, Rich};

/// What the pasteboard offers, in the order of preference html, rtf, plain.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pasted<'a> {
    /// The HTML flavour, as text.
    pub html: Option<&'a str>,
    /// The RTF flavour, in its bytes.
    pub rtf: Option<&'a [u8]>,
    /// The plain text flavour.
    pub plain: Option<&'a str>,
}

/// A picture that needs a place in the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSource<'a> {
    /// An address as the pasted content gives it.
    Address(&'a str),
    /// A picture the pasted content carries along.
    Data {
        /// Its media type, as `image/png`.
        mime: &'a str,
        /// The picture in its format.
        bytes: &'a [u8],
    },
}

/// Pasted content as Markdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Converted {
    /// The text, without a line break at its end.
    pub markdown: String,
    /// More than one block, or a block that is no paragraph: it needs lines of its own.
    pub blocks: bool,
}

/// `place` says where an image goes in Markdown: its destination, or None to leave it out (its alt text stays).
pub fn paste_as_markdown(pasted: &Pasted, place: &mut dyn FnMut(&ImageSource) -> Option<String>) -> Option<Converted> {
    if let Some(converted) = pasted.html.and_then(|html| convert(html::from_html(html), place)) {
        return Some(converted);
    }
    if let Some(converted) = pasted.rtf.and_then(|rtf| convert(rtf::from_rtf(rtf), place)) {
        return Some(converted);
    }
    let plain = pasted.plain.filter(|plain| !plain.is_empty())?;
    if let Some(table) = tabular::from_text(plain).and_then(|table| convert(table, place)) {
        return Some(table);
    }
    Some(Converted {
        markdown: plain.to_string(),
        blocks: plain.trim().lines().any(|line| line.trim().is_empty()),
    })
}

/// `rich` as what a paste needs, or nothing if it holds nothing.
fn convert(mut rich: Rich, place: &mut dyn FnMut(&ImageSource) -> Option<String>) -> Option<Converted> {
    rich.place_images(place);
    rich.settle_images();
    let rich = rich.tidied();
    let blocks = match rich.blocks.as_slice() {
        [] => return None,
        [Block::Paragraph(_)] => false,
        _ => true,
    };
    let mut markdown = rich.tidy_markdown();
    markdown.truncate(markdown.trim_end_matches('\n').len());
    Some(Converted { markdown, blocks })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing(_: &ImageSource) -> Option<String> {
        None
    }

    fn pasted(html: Option<&str>, rtf: Option<&[u8]>, plain: Option<&str>) -> Option<Converted> {
        paste_as_markdown(&Pasted { html, rtf, plain }, &mut nothing)
    }

    fn markdown(converted: Option<Converted>) -> (String, bool) {
        let converted = converted.expect("something on offer");
        (converted.markdown, converted.blocks)
    }

    #[test]
    fn nothing_on_offer_gives_nothing() {
        assert_eq!(pasted(None, None, None), None);
        assert_eq!(pasted(None, None, Some("")), None);
        assert_eq!(pasted(Some("<p> </p>"), Some(b"no rtf"), None), None);
    }

    #[test]
    fn html_comes_first_if_it_yields_content() {
        let rtf: &[u8] = br"{\rtf1 \b fett}";
        assert_eq!(
            markdown(pasted(Some("<b>a</b>"), Some(rtf), Some("c"))),
            ("**a**".to_string(), false)
        );
        assert_eq!(
            markdown(pasted(Some("<meta charset=utf-8>"), Some(rtf), Some("c"))),
            ("**fett**".to_string(), false)
        );
        assert_eq!(
            markdown(pasted(Some(" "), Some(b"{\\rtf1 }"), Some("c"))),
            ("c".to_string(), false)
        );
    }

    #[test]
    fn plain_text_stays_as_it_is_unless_it_is_a_table() {
        assert_eq!(
            markdown(pasted(None, None, Some("# Titel\n\n*a*\n"))),
            ("# Titel\n\n*a*\n".to_string(), true)
        );
        assert_eq!(
            markdown(pasted(None, None, Some("a\nb\n"))),
            ("a\nb\n".to_string(), false)
        );
        assert!(markdown(pasted(None, None, Some("a\r\n \r\nb"))).1);
        assert_eq!(
            markdown(pasted(None, None, Some("a\tb\n1\t2\n"))),
            ("| a | b |\n|---:|---:|\n| 1 | 2 |".to_string(), true)
        );
    }

    #[test]
    fn blocks_tell_what_needs_lines_of_its_own() {
        assert!(!markdown(pasted(Some("a <i>b</i>"), None, None)).1);
        assert!(markdown(pasted(Some("<p>a</p><p>b</p>"), None, None)).1);
        assert!(markdown(pasted(Some("<h1>a</h1>"), None, None)).1);
        assert!(markdown(pasted(Some("<ul><li>a</ul>"), None, None)).1);
    }

    #[test]
    fn pictures_go_where_place_says_and_leave_their_description_otherwise() {
        let html = "<img src=\"https://a.bc/d.png\" alt=\"x\"> <img src=\"e.png\" alt=\"y\">";
        let mut seen = Vec::new();
        let converted = paste_as_markdown(
            &Pasted {
                html: Some(html),
                ..Pasted::default()
            },
            &mut |source| {
                if let ImageSource::Address(address) = source {
                    seen.push((*address).to_string());
                    return address.starts_with("https").then(|| "images/one two.png".to_string());
                }
                None
            },
        );
        assert_eq!(seen, ["https://a.bc/d.png", "e.png"]);
        assert_eq!(markdown(converted).0, "![x](<images/one two.png>) y");
    }
}
