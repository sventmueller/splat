//! XML from anywhere, read with roxmltree without letting it take quadratic time: it compares each attribute with those of
//! its element and looks through the namespaces in scope for each prefix.

use roxmltree::{Document, Error, ParsingOptions};

/// Why a part of a Word file cannot be read as XML.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum XmlError {
    TooLarge,
    Damaged,
}

/// More than any element Word or LibreOffice writes.
const MOST_ATTRIBUTES: usize = 256;
/// More than in scope anywhere in what Word or LibreOffice write.
const MOST_NAMESPACES: usize = 256;
/// About as many nodes as a `word/document.xml` of the size allowed can hold when it is not made to hurt.
const MOST_NODES: u32 = 16_000_000;

/// `text` as a document with neither a document type nor more than the limits above.
pub(crate) fn parse(text: &str) -> Result<Document<'_>, XmlError> {
    if !within_limits(text.as_bytes()) {
        return Err(XmlError::Damaged);
    }
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: MOST_NODES,
        ..ParsingOptions::default()
    };
    Document::parse_with_options(text, options).map_err(|error| match error {
        Error::NodesLimitReached => XmlError::TooLarge,
        _ => XmlError::Damaged,
    })
}

/// Whether no element has too many attributes and too many namespaces are nowhere in scope. It only has to be right for
/// what roxmltree accepts; what it reads wrongly in other input, roxmltree refuses after it.
fn within_limits(xml: &[u8]) -> bool {
    let mut at = 0;
    let mut depth = 0usize;
    let mut declared: Vec<(usize, usize)> = Vec::new();
    let mut in_scope = 0;
    while let Some(open) = xml[at..].iter().position(|b| *b == b'<') {
        at += open + 1;
        match xml.get(at) {
            Some(b'/') => {
                depth = depth.saturating_sub(1);
                while declared.last().is_some_and(|(level, _)| *level > depth) {
                    in_scope -= declared.pop().map_or(0, |(_, count)| count);
                }
            }
            Some(b'!' | b'?') => at = behind_markup(xml, at),
            Some(_) => {
                let tag = read_tag(xml, at);
                at = tag.end;
                if tag.attributes > MOST_ATTRIBUTES {
                    return false;
                }
                in_scope += tag.namespaces;
                if in_scope > MOST_NAMESPACES {
                    return false;
                }
                if tag.closed {
                    in_scope -= tag.namespaces;
                } else {
                    depth += 1;
                    if tag.namespaces > 0 {
                        declared.push((depth, tag.namespaces));
                    }
                }
            }
            None => return true,
        }
    }
    true
}

/// Behind a comment, a CDATA section or an instruction starting at `at`, which holds what follows the `<`.
pub(crate) fn behind_markup(xml: &[u8], at: usize) -> usize {
    let rest = &xml[at..];
    let end: &[u8] = if rest.starts_with(b"!--") {
        b"-->"
    } else if rest.starts_with(b"![CDATA[") {
        b"]]>"
    } else if rest.starts_with(b"?") {
        b"?>"
    } else {
        b">"
    };
    rest.windows(end.len())
        .position(|window| window == end)
        .map_or(xml.len(), |found| at + found + end.len())
}

struct ReadTag {
    end: usize,
    attributes: usize,
    namespaces: usize,
    closed: bool,
}

/// What the tag whose name starts at `at` holds, and where it ends.
fn read_tag(xml: &[u8], at: usize) -> ReadTag {
    let mut tag = ReadTag {
        end: xml.len(),
        attributes: 0,
        namespaces: 0,
        closed: false,
    };
    let mut quote = None;
    let mut name_start = at;
    for (n, byte) in xml[at..].iter().enumerate() {
        let here = at + n;
        match quote {
            Some(open) => {
                if *byte == open {
                    quote = None;
                }
            }
            None => match byte {
                b'"' | b'\'' => quote = Some(*byte),
                b'=' => {
                    tag.attributes += 1;
                    if xml[name_start..here].trim_ascii().starts_with(b"xmlns") {
                        tag.namespaces += 1;
                    }
                }
                b'>' => {
                    tag.end = here + 1;
                    tag.closed = here > at && xml[here - 1] == b'/';
                    return tag;
                }
                b if b.is_ascii_whitespace() => name_start = here + 1,
                _ => {}
            },
        }
    }
    tag
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attributes(count: usize) -> String {
        (0..count).map(|n| format!(" a{n}=\"1\"")).collect()
    }

    #[test]
    fn ordinary_documents_are_read() {
        let doc = parse("<?xml version=\"1.0\"?><!-- c --><a xmlns:b=\"u\" c='>'><b:d/><![CDATA[<x>]]>t</a>").unwrap();
        assert_eq!(doc.root_element().tag_name().name(), "a");
        assert_eq!(doc.root_element().attribute("c"), Some(">"));
    }

    #[test]
    fn a_document_type_is_refused() {
        let bomb = "<!DOCTYPE a [<!ENTITY b \"c\">]><a>&b;</a>";
        assert_eq!(parse(bomb).unwrap_err(), XmlError::Damaged);
        assert_eq!(parse("<a>").unwrap_err(), XmlError::Damaged);
    }

    #[test]
    fn too_many_attributes_are_refused_before_they_take_quadratic_time() {
        let fits = format!("<a{}/>", attributes(MOST_ATTRIBUTES));
        assert!(parse(&fits).is_ok());
        let too_many = format!("<a{}/>", attributes(MOST_ATTRIBUTES + 1));
        assert_eq!(parse(&too_many).unwrap_err(), XmlError::Damaged);
    }

    #[test]
    fn too_many_namespaces_in_scope_are_refused_but_not_those_that_end() {
        let spaces =
            |range: std::ops::Range<usize>| -> String { range.map(|n| format!(" xmlns:n{n}=\"u\"")).collect() };
        let nested: String = (0..=MOST_NAMESPACES)
            .map(|n| format!("<a{}>", spaces(n..n + 1)))
            .collect();
        let closing = "</a>".repeat(MOST_NAMESPACES + 1);
        assert_eq!(parse(&format!("{nested}{closing}")).unwrap_err(), XmlError::Damaged);

        let side_by_side: String = (0..MOST_NAMESPACES * 2)
            .map(|n| format!("<b{}/><c{}></c>", spaces(n..n + 1), spaces(n..n + 1)))
            .collect();
        assert!(parse(&format!("<a>{side_by_side}</a>")).is_ok());
    }

    #[test]
    fn deep_nesting_is_read_without_a_stack() {
        let depth = 200_000;
        let doc = format!("{}x{}", "<a>".repeat(depth), "</a>".repeat(depth));
        assert_eq!(parse(&doc).unwrap().descendants().count(), depth + 2);
    }

    #[test]
    fn cut_off_markup_is_refused_after_the_scan() {
        for cut in ["<a", "<a b=\"", "<!-- a", "<![CDATA[", "<?x", "<!x", "<"] {
            assert_eq!(parse(cut).unwrap_err(), XmlError::Damaged, "{cut}");
        }
    }
}
