//! The links a note holds and where each of them is written: wiki links, Markdown links and images from the tree, and
//! link reference definitions, which the tree leaves out.

use std::borrow::Cow;
use std::ops::Range;

use splat_markdown::doc::facts::{LONGEST_LABEL, ends_footnote_label};
use splat_markdown::doc::{Block, BlockKind, Doc, Inline, InlineKind, walk_blocks, walk_inlines};
use splat_markdown::text::{Byte, container_mark, decode_entity, indent_len, line_end, next_line_start, offsets};

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Written {
    /// Without the angle brackets.
    pub range: Range<usize>,
    pub angle: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum LinkTarget<'d> {
    /// `inner` is what stands between the brackets.
    Wiki { target: &'d str, inner: Range<usize> },
    /// `written` is missing where the address stands elsewhere, as for a link to a definition, or where it stands in
    /// HTML.
    Address {
        url: Cow<'d, str>,
        written: Option<Written>,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Link<'d> {
    pub whole: Range<usize>,
    pub says: LinkTarget<'d>,
}

/// The links of `doc` in the order of the text.
pub(crate) fn links_of(doc: &Doc) -> Vec<Link<'_>> {
    let text = doc.text();
    let mut links = Vec::new();
    walk_blocks(doc.blocks(), &mut |block| {
        walk_inlines(block.inlines(), &mut |inline| {
            links.extend(link_of(text, inline));
        });
    });
    definitions(doc, &mut links);
    links.sort_by_key(|link| link.whole.start);
    links
}

fn link_of<'d>(text: &str, inline: &'d Inline) -> Option<Link<'d>> {
    let whole = offsets(&inline.range());
    let says = match inline.kind() {
        InlineKind::WikiLink { target, embed } => {
            let source = text.get(whole.clone())?;
            let open = if *embed { "![[" } else { "[[" };
            source.strip_prefix(open)?.strip_suffix("]]")?;
            LinkTarget::Wiki {
                target,
                inner: whole.start + open.len()..whole.end - 2,
            }
        }
        InlineKind::Link { url, .. } | InlineKind::Image { url, .. } => LinkTarget::Address {
            url: Cow::Borrowed(url),
            written: if inline.written_as_html(text) {
                None
            } else {
                written_address(text, inline)
            },
        },
        _ => return None,
    };
    Some(Link { whole, says })
}

/// Where the address of a Markdown link stands; `None` if it stands in a definition.
fn written_address(text: &str, inline: &Inline) -> Option<Written> {
    let (from, end) = (inline.content_range().end.get(), inline.range().end.get());
    let open = from + text.get(from..end)?.find(']')? + 1;
    text[open..end].strip_prefix('(')?;
    destination(text, open + 1, end)
}

/// The address that starts at or behind `from`, which may have a line break and the marks of a quote in front of it;
/// where comrak's scanner ends it, so that the two agree on any input.
fn destination(text: &str, from: usize, limit: usize) -> Option<Written> {
    let rest = &text[from..limit];
    let start = from + (rest.len() - rest.trim_start_matches([' ', '\t', '\r', '\n', '>']).len());
    let bytes = &text.as_bytes()[start..limit];
    let mut at = 0;
    if bytes.first() == Some(&b'<') {
        at = 1;
        while let Some(&b) = bytes.get(at) {
            match b {
                b'>' => {
                    return Some(Written {
                        range: start + 1..start + at,
                        angle: true,
                    });
                }
                b'\\' => at += 2,
                b'\n' | b'\r' | b'<' => return None,
                _ => at += 1,
            }
        }
        return None;
    }
    let mut depth = 0usize;
    while let Some(&b) = bytes.get(at) {
        match b {
            b'\\' if bytes.get(at + 1).is_some_and(u8::is_ascii_punctuation) => at += 2,
            b'(' => {
                depth += 1;
                at += 1;
            }
            b')' if depth == 0 => break,
            b')' => {
                depth -= 1;
                at += 1;
            }
            0 => at += 1,
            b if b.is_ascii_whitespace() || b.is_ascii_control() => break,
            _ => at += 1,
        }
    }
    (at > 0 && text.is_char_boundary(start + at)).then_some(Written {
        range: start..start + at,
        angle: false,
    })
}

/// The first unit of an address or wiki target as comrak reads it: an escape or a character reference counts as what it
/// stands for. Gives its length in the source and what it starts with.
pub(crate) fn unit_at(raw: &str) -> Option<(usize, char)> {
    let c = raw.chars().next()?;
    if c == '\\'
        && let Some(next) = raw[1..].chars().next().filter(char::is_ascii_punctuation)
    {
        return Some((2, next));
    }
    if c == '&'
        && let Some((decoded, len)) = decode_entity(raw)
        && let Some(first) = decoded.chars().next()
    {
        return Some((len, first));
    }
    Some((c.len_utf8(), c))
}

/// An address as comrak hands it on: its character references read first, then its escapes.
pub(crate) fn cleaned(raw: &str) -> String {
    let mut references_read = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(c) = rest.chars().next() {
        match decode_entity(rest).filter(|_| c == '&') {
            Some((decoded, len)) => {
                references_read.push_str(&decoded);
                rest = &rest[len..];
            }
            None => {
                references_read.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    let mut out = String::with_capacity(references_read.len());
    let mut chars = references_read.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(if c == '\\' {
            chars.next_if(char::is_ascii_punctuation).unwrap_or(c)
        } else {
            c
        });
    }
    out
}

/// Whether only marks that open containers stand in front of `at` on its line, so that a definition can start there.
/// Looking back stops at the first other character, as a line can hold a great many labels.
fn opens_its_line(text: &str, at: usize) -> bool {
    let bytes = &text.as_bytes()[..at];
    let start = bytes
        .iter()
        .rposition(|b| !matches!(b, b' ' | b'\t' | b'>' | b'-' | b'+' | b'*' | b'.' | b')' | b'0'..=b'9'))
        .map_or(0, |at| at + 1);
    let breaks = start == 0 || matches!(bytes[start - 1], b'\n' | b'\r');
    breaks && only_opens_containers(&text[start..at])
}

fn only_opens_containers(mut head: &str) -> bool {
    loop {
        head = &head[indent_len(head)..];
        if head.is_empty() {
            return true;
        }
        match container_mark(head) {
            Some((_, len)) => head = &head[len..],
            None => return false,
        }
    }
}

/// Where the label ends at `close` opens, if nothing but its text stands between.
fn label_start(text: &str, close: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let lowest = close.saturating_sub(LONGEST_LABEL);
    (lowest..close)
        .rev()
        .find(|&at| {
            let backslashes = || bytes[lowest..at].iter().rev().take_while(|b| **b == b'\\').count();
            matches!(bytes[at], b'[' | b']') && backslashes() % 2 == 0
        })
        .filter(|&at| bytes[at] == b'[')
}

/// Where the line behind a whole definition at `at` starts, looking no further than `limit`: only what comrak took for
/// a definition stands there like this, since it reads such a line at the start of a paragraph as nothing else.
fn behind_definition(text: &str, at: usize, limit: usize) -> Option<usize> {
    let close = at + text.get(at..limit)?.strip_prefix('[')?.find(']')? + 1;
    text[close..limit].strip_prefix("]:")?;
    let written = destination(text, close + 2, limit)?;
    let behind = written.range.end + usize::from(written.angle);
    let line_end = line_end(text, Byte::new(behind));
    let line = &text[behind..line_end.get()];
    let shown = line.trim_matches([' ', '\t']);
    let only_a_title = match shown.as_bytes() {
        [] => true,
        [b'"', .., b'"'] | [b'\'', .., b'\''] | [b'(', .., b')'] => shown.len() >= 2 && line.starts_with([' ', '\t']),
        _ => false,
    };
    only_a_title.then(|| next_line_start(text, line_end).map_or(text.len(), Byte::get))
}

/// The source of a block that is no definition, whatever it looks like. Where the reader could not place what stands
/// behind a definition, a paragraph holds it as text, and the definition counts all the same.
fn taken_by(text: &str, block: &Block) -> Option<Range<usize>> {
    match &block.kind {
        BlockKind::Paragraph | BlockKind::Heading { .. } => {
            let mut range = offsets(&block.inline_range()?);
            while let Some(next) = behind_definition(text, range.start, range.end).filter(|next| *next <= range.end) {
                let rest = &text[next..range.end];
                range.start = range.end - rest.trim_start_matches([' ', '\t', '>']).len();
            }
            Some(range)
        }
        BlockKind::Alert { marker, .. } => Some(offsets(marker)),
        _ if block.is_leaf() => Some(offsets(&block.range)),
        _ => None,
    }
}

/// The definitions of links: every `]:` that stands outside of all text of the tree behind a label that opens its line,
/// with the address behind it.
fn definitions<'d>(doc: &'d Doc, links: &mut Vec<Link<'d>>) {
    let text = doc.text();
    let mut taken = Vec::new();
    walk_blocks(doc.blocks(), &mut |block| taken.extend(taken_by(text, block)));
    taken.sort_by_key(|range| range.start);
    let (mut next, mut reach) = (0, 0);
    for (close, _) in text.match_indices("]:") {
        while let Some(range) = taken.get(next).filter(|range| range.start <= close) {
            reach = reach.max(range.end);
            next += 1;
        }
        if close < reach || ends_footnote_label(text, close) {
            continue;
        }
        let Some(open) = label_start(text, close) else {
            continue;
        };
        if !opens_its_line(text, open) {
            continue;
        }
        let Some(written) = destination(text, close + 2, text.len()) else {
            continue;
        };
        let end = written.range.end + usize::from(written.angle);
        links.push(Link {
            whole: open..end,
            says: LinkTarget::Address {
                url: Cow::Owned(cleaned(&text[written.range.clone()])),
                written: Some(written),
            },
        });
    }
}

#[cfg(test)]
mod tests {
    use splat_markdown::reader::parse;

    use super::*;

    /// What each link of `text` says, in short.
    fn said(text: &str) -> Vec<String> {
        let doc = parse(text);
        links_of(&doc)
            .into_iter()
            .map(|link| match link.says {
                LinkTarget::Wiki { target, inner } => format!("wiki {target} in {}", &text[inner]),
                LinkTarget::Address { url, written } => match written {
                    Some(written) => format!(
                        "{url} at {}{}",
                        if written.angle { "<>" } else { "" },
                        &text[written.range]
                    ),
                    None => format!("{url} elsewhere"),
                },
            })
            .collect()
    }

    #[test]
    fn links_are_found_where_they_are_written() {
        assert_eq!(
            said(
                "[a](b.md) ![c](d.md \"t\") [e](<f g.md>) [h][i] [i] ![[j]] [[k|l]] <a href=\"m.md\">n</a>\n\n[i]: o%20p.md \"title\"\n"
            ),
            [
                "b.md at b.md",
                "d.md at d.md",
                "f g.md at <>f g.md",
                "o%20p.md elsewhere",
                "o%20p.md elsewhere",
                "wiki j in j",
                "wiki k in k|l",
                "m.md elsewhere",
                "o%20p.md at o%20p.md",
            ]
        );
    }

    #[test]
    fn an_address_is_read_up_to_its_end() {
        for (text, wanted) in [
            ("[a](b(c)d.md)\n", "b(c)d.md at b(c)d.md"),
            ("[a](b\\)c.md)\n", "b)c.md at b\\)c.md"),
            ("[a](b\u{0}c.md)\n", "b\u{0}c.md at b\u{0}c.md"),
            ("[a](<b\\>c.md>)\n", "b>c.md at <>b\\>c.md"),
            ("[a](\n  b.md\n  \"t\")\n", "b.md at b.md"),
            ("> [a](\n> b.md)\n", "b.md at b.md"),
            ("[a *b* [c]](d.md)\n", "d.md at d.md"),
            ("[](d.md)\n", "d.md at d.md"),
            ("![](d.md)\n", "d.md at d.md"),
            ("[a](b&amp;c.md)\n", "b&c.md at b&amp;c.md"),
        ] {
            assert_eq!(said(text), [wanted], "{text:?}");
        }
    }

    #[test]
    fn an_address_ends_where_comrak_ends_it() {
        let at = |text: &str| destination(text, 0, text.len()).map(|written| (written.range, written.angle));
        assert_eq!(at("a b"), Some((0..1, false)));
        assert_eq!(at("a\u{1}b"), Some((0..1, false)));
        assert_eq!(at("a\u{0}b"), Some((0..3, false)));
        assert_eq!(at("a\\ b"), Some((0..2, false)), "a backslash escapes only punctuation");
        assert_eq!(at("a\\)b)"), Some((0..4, false)));
        assert_eq!(at("(a)b)"), Some((0..4, false)));
        assert_eq!(at("(a"), Some((0..2, false)), "the parser tells whether this is a link");
        assert_eq!(at("<a\\>b> c"), Some((1..5, true)));
        assert_eq!(at("<a"), None);
        assert_eq!(at("<a\nb>"), None);
        assert_eq!(at("<a<b>"), None);
        assert_eq!(at(")"), None);
        assert_eq!(at(""), None);
    }

    #[test]
    fn an_empty_address_has_no_place_to_change() {
        assert_eq!(said("[a]()\n"), [" elsewhere"]);
        assert_eq!(said("[a](<>)\n"), [" at <>"]);
    }

    #[test]
    fn what_only_looks_like_a_link_is_none() {
        for text in [
            "`[a](b.md)` `[[c]]`\n",
            "```\n[a](b.md)\n[[c]]\n[r]: d.md\n```\n",
            "    [a](b.md)\n",
            "%%[a](b.md) [[c]]%%\n",
            "%%\n[a](b.md)\n[[c]]\n[r]: d.md\n%%\n",
            "<div>[a](b.md)\n[r]: d.md\n</div>\n",
            "---\nlink: \"[a](b.md)\"\n[r]: d.md\n---\n",
            "\\[a](b.md) \\[[c]]\n",
            "$$\n[r]: d.md\n$$\n",
            "text [r]: d.md\n",
            "text\n[r]: d.md\n",
            "[^1]: note\n",
            "[a]: \n",
            "x[r]: d.md\n\n# h [s]: e\n",
            "| [r]: d.md |\n|---|\n",
            "> [!note]: d.md\n> text\n",
        ] {
            assert_eq!(said(text), Vec::<String>::new(), "{text:?}");
        }
    }

    #[test]
    fn a_definition_is_found_inside_containers_and_over_lines() {
        for (text, wanted) in [
            ("[r]: a.md\n", "a.md at a.md"),
            ("  [r]: a.md \"t\"\n", "a.md at a.md"),
            ("[r]: <a b.md>\n", "a b.md at <>a b.md"),
            ("[r]:\n  a.md\n", "a.md at a.md"),
            ("[long\nlabel]: a.md\n", "a.md at a.md"),
            ("> [r]: a.md\n", "a.md at a.md"),
            ("- [r]: a.md\n", "a.md at a.md"),
            ("1. [r]: a.md\n", "a.md at a.md"),
            ("[r\\]s]: a.md\n", "a.md at a.md"),
            ("[r]: a\\_b.md\n", "a_b.md at a\\_b.md"),
            ("[r]: a.md\nText [r]\n", "a.md at a.md"),
        ] {
            assert!(said(text).contains(&wanted.to_string()), "{text:?}: {:?}", said(text));
        }
    }

    #[test]
    fn a_definition_in_front_of_what_the_reader_could_not_place_counts_all_the_same() {
        let text = "[d]: a.md \"T\"\n[e]: <b.md>\n <a href=\"x.md\">html</a> y\n";
        assert_eq!(said(text), ["a.md at a.md", "b.md at <>b.md"]);
        for (text, end) in [
            ("[a]: b\nc", Some(7)),
            ("[a]: b", Some(6)),
            ("[a]: b\r\nc", Some(8)),
            ("[a]: <b>  \nc", Some(11)),
            ("[a]: b \"t\" \nc", Some(12)),
            ("[a]: b 't'\nc", Some(11)),
            ("[a]: b (t)\nc", Some(11)),
            ("[a]: b\"t\"\nc", Some(10)),
            ("[a]: b c\nd", None),
            ("[a]: b \"t\nd", None),
            ("[a] b", None),
            ("a]: b", None),
            ("[a", None),
            ("[a]:", None),
        ] {
            assert_eq!(behind_definition(text, 0, text.len()), end, "{text:?}");
        }
        assert_eq!(behind_definition("x", 9, 9), None);
        assert_eq!(behind_definition("[a]: b\n", 0, 2), None, "beyond what to look at");
    }

    #[test]
    fn a_label_ends_where_its_brackets_do() {
        let at = |text: &str| label_start(text, text.find("]:").unwrap());
        assert_eq!(at("[a]: b"), Some(0));
        assert_eq!(at("x[a]: b"), Some(1));
        assert_eq!(at("a]: b"), None);
        assert_eq!(at("[a]b]: c"), None);
        assert_eq!(at("[a\\]b]: c"), Some(0));
        assert_eq!(at("[a\\\\]b]: c"), None);
        assert_eq!(
            at(&format!("[{}]: c", "a".repeat(LONGEST_LABEL + 1))),
            None,
            "too long for a label"
        );
    }

    #[test]
    fn units_stand_for_what_they_spell() {
        assert_eq!(unit_at(""), None);
        assert_eq!(unit_at("a"), Some((1, 'a')));
        assert_eq!(unit_at("ä"), Some((2, 'ä')));
        assert_eq!(unit_at("\\#"), Some((2, '#')));
        assert_eq!(unit_at("\\a"), Some((1, '\\')));
        assert_eq!(unit_at("\\"), Some((1, '\\')));
        assert_eq!(unit_at("&#35;x"), Some((5, '#')));
        assert_eq!(unit_at("&x"), Some((1, '&')));
        assert_eq!(cleaned("a\\_b&amp;c&#35;"), "a_b&c#");
        assert_eq!(cleaned("\\a&x"), "\\a&x");
        assert_eq!(cleaned("\\&amp;"), "&", "references come first");
        assert_eq!(cleaned("\\\\"), "\\");
        assert_eq!(cleaned("a\\"), "a\\");
    }

    #[test]
    fn only_marks_that_open_a_container_may_stand_in_front_of_a_definition() {
        for head in ["", "   ", ">", "> > ", "- ", "1. ", "> - 2) "] {
            assert!(only_opens_containers(head), "{head:?}");
        }
        for head in ["x", "> x", "1 ", "#"] {
            assert!(!only_opens_containers(head), "{head:?}");
        }
        assert!(opens_its_line("[a", 0));
        assert!(opens_its_line("x\n> [a", 4));
        assert!(opens_its_line("x\r- [a", 4));
        assert!(!opens_its_line("x [a", 2));
        assert!(!opens_its_line("1 [a", 2));
    }
}
