use std::ops::Range;

use comrak::nodes::NodeValue;

use super::Reader;
use super::notes::INLINE_NOTE;
use super::positions::inline_note_len;
use crate::markdown::doc::{Inline, InlineKind};
use crate::markdown::text::{Byte, TextAt};
use crate::markdown::{markup, style, url};

impl Reader<'_> {
    pub(super) fn inlines<'a>(&self, node: comrak::nodes::Node<'a>) -> Vec<Inline> {
        let mut raw: Vec<Inline> = node.children().filter_map(|c| self.inline(c)).collect();
        // comrak's positions for a few extensions, such as inline footnotes over several lines, can run past the
        // next inline; everything after the tree relies on inlines following each other.
        let mut end = Byte::default();
        for i in &mut raw {
            if i.range.start < end {
                i.range.start = end.min(i.range.end.max(end));
            }
            i.range.end = i.range.end.max(i.range.start);
            end = i.range.end;
            keep_inside(&mut i.children, &i.range);
        }
        pair_html(raw)
    }

    fn inline<'a>(&self, node: comrak::nodes::Node<'a>) -> Option<Inline> {
        let data = node.data();
        let mut range = self.range(data.sourcepos);
        if let NodeValue::FootnoteReference(f) = &data.value
            && f.name.starts_with(INLINE_NOTE)
            && let Some(len) = self.text.get(range.start.get()..).and_then(inline_note_len)
        {
            range.end = range.start + len;
        }
        let Some(_level) = self.enter() else {
            return Some(self.raw_text(range));
        };
        let kind = match &data.value {
            NodeValue::Text(t) => InlineKind::Text(t.as_ref().into()),
            NodeValue::SoftBreak => InlineKind::SoftBreak,
            NodeValue::LineBreak => InlineKind::LineBreak,
            NodeValue::Code(c) => InlineKind::Code(c.literal.as_str().into()),
            NodeValue::HtmlInline(h) => InlineKind::Html(h.as_str().into()),
            NodeValue::Emph => InlineKind::Emph,
            NodeValue::Strong => InlineKind::Strong,
            NodeValue::Strikethrough => InlineKind::Strikethrough,
            NodeValue::Link(l) => InlineKind::Link {
                url: l.url.as_str().into(),
                title: l.title.as_str().into(),
            },
            NodeValue::Image(l) => InlineKind::Image {
                url: l.url.as_str().into(),
                title: l.title.as_str().into(),
            },
            NodeValue::FootnoteReference(f) => InlineKind::FootnoteRef {
                name: f.name.as_str().into(),
                ix: f.ix,
            },
            NodeValue::Math(m) => InlineKind::Math {
                display: m.display_math,
                literal: m.literal.as_str().into(),
            },
            NodeValue::Escaped => InlineKind::Text(self.text.at(range.clone()).trim_start_matches('\\').into()),
            NodeValue::Highlight => InlineKind::Highlight,
            NodeValue::Superscript => InlineKind::Superscript,
            NodeValue::Subscript => InlineKind::Subscript,
            NodeValue::SpoileredText => InlineKind::Spoiler,
            NodeValue::ShortCode(c) => InlineKind::Emoji(c.emoji.as_str().into()),
            NodeValue::WikiLink(w) => InlineKind::WikiLink {
                target: w.url.as_str().into(),
                embed: w.embed,
            },
            NodeValue::CommentInline(_) => InlineKind::Comment,
            // A single `|` pair in a table cell, which comrak reads after taking the backslashes out of `\|`.
            NodeValue::EscapedTag(_)
            | NodeValue::Document
            | NodeValue::FrontMatter(_)
            | NodeValue::BlockQuote
            | NodeValue::List(_)
            | NodeValue::Item(_)
            | NodeValue::DescriptionList
            | NodeValue::DescriptionItem(_)
            | NodeValue::DescriptionTerm
            | NodeValue::DescriptionDetails
            | NodeValue::CodeBlock(_)
            | NodeValue::HtmlBlock(_)
            | NodeValue::Paragraph
            | NodeValue::Heading(_)
            | NodeValue::ThematicBreak
            | NodeValue::FootnoteDefinition(_)
            | NodeValue::Table(_)
            | NodeValue::TableRow(_)
            | NodeValue::TableCell
            | NodeValue::TaskItem(_)
            | NodeValue::Raw(_)
            | NodeValue::Insert
            | NodeValue::MultilineBlockQuote(_)
            | NodeValue::Underline
            | NodeValue::Alert(_)
            | NodeValue::Callout(_)
            | NodeValue::CalloutTitle
            | NodeValue::Subtext
            | NodeValue::BlockDirective(_)
            | NodeValue::MultilineComment(_) => {
                InlineKind::Text(self.text.at(range.clone()).replace("\\|", "|").into())
            }
        };
        drop(data);
        let children = if kind.holds_inlines() {
            self.inlines(node).into()
        } else {
            Box::default()
        };
        Some(Inline { kind, range, children })
    }
}

/// Clamps the ranges of inlines into their container's.
fn keep_inside(children: &mut [Inline], outer: &Range<Byte>) {
    for c in children {
        c.range.start = c.range.start.clamp(outer.start, outer.end);
        c.range.end = c.range.end.clamp(c.range.start, outer.end);
        let range = c.range.clone();
        keep_inside(&mut c.children, &range);
    }
}

/// What a single inline HTML tag stands for: whether it closes, the container it opens or closes, or the inline it
/// is on its own.
enum HtmlTag {
    Container { closing: bool, kind: InlineKind },
    Alone(InlineKind),
}

fn html_tag(html: &str) -> Option<HtmlTag> {
    let trimmed = html.trim();
    if trimmed.starts_with("<!--") {
        return Some(HtmlTag::Alone(InlineKind::Comment));
    }
    let (tag, end) = markup::read_tag(trimmed, 0)?;
    if end != trimmed.len() {
        return None;
    }
    let (closing, name, attrs, self_closing) = match tag {
        markup::Tag::Open {
            name,
            attrs,
            self_closing,
        } => (false, name, attrs, self_closing),
        markup::Tag::Close { name } => (true, name, Vec::new(), false),
    };
    let attr = |key: &str| attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    let pixels = |key: &str| attr(key).and_then(markup::pixels);
    let kind = match name.as_str() {
        "br" if !closing => return Some(HtmlTag::Alone(InlineKind::LineBreak)),
        "img" if !closing => {
            let url = attr("src").filter(|u| url::is_harmless(u))?;
            return Some(HtmlTag::Alone(InlineKind::HtmlImage {
                url: url.into(),
                alt: attr("alt").unwrap_or("").into(),
                width: pixels("width"),
                height: pixels("height"),
            }));
        }
        _ if self_closing => return None,
        "a" => InlineKind::Link {
            url: attr("href").filter(|u| url::is_harmless(u)).unwrap_or("").into(),
            title: attr("title").unwrap_or("").into(),
        },
        name => style::styled_element(name)?,
    };
    Some(HtmlTag::Container { closing, kind })
}

/// Whether a closing tag ends the container an opening tag started: links match whatever their address.
fn same_container(open: &InlineKind, close: &InlineKind) -> bool {
    std::mem::discriminant(open) == std::mem::discriminant(close)
}

/// Inline HTML nested deeper stays raw: the containers `pair_html` makes add to `MAX_DEPTH`, which comrak's own nesting
/// uses up on its own.
const MAX_PAIRED: usize = 16;

/// Groups inline HTML such as `<b>…</b>`, `<a href>…</a>` or `<mark>…</mark>` into container inlines and turns
/// `<br>`, `<img>` and comments into their own inlines; unmatched tags stay raw HTML.
fn pair_html(raw: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    let mut open: Vec<(usize, InlineKind)> = Vec::new();
    for mut inline in raw {
        let tag = match &inline.kind {
            InlineKind::Html(h) => html_tag(h),
            _ => None,
        };
        if let Some(HtmlTag::Alone(kind)) = tag {
            inline.kind = kind;
            out.push(inline);
            continue;
        }
        if let Some(HtmlTag::Container { closing, kind }) = tag {
            if !closing {
                if open.len() < MAX_PAIRED {
                    open.push((out.len(), kind));
                }
                out.push(inline);
                continue;
            }
            if let Some(pos) = open.iter().rposition(|(_, k)| same_container(k, &kind)) {
                let (idx, kind) = open.remove(pos);
                open.truncate(pos);
                // `<a>` without an address that may be followed is no link to anyone.
                let kind = match kind {
                    InlineKind::Link { url, .. } if url.is_empty() => InlineKind::Span,
                    kind => kind,
                };
                let mut children: Vec<Inline> = out.drain(idx..).collect();
                let opener = children.remove(0);
                if children.is_empty() {
                    children.push(Inline {
                        kind: InlineKind::Text("".into()),
                        range: opener.range.end..opener.range.end,
                        children: Box::default(),
                    });
                }
                out.push(Inline {
                    kind,
                    range: opener.range.start..inline.range.end,
                    children: children.into(),
                });
                continue;
            }
        }
        out.push(inline);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::parse;
    use super::super::tests::slice;
    use super::*;
    use crate::markdown::text::bytes;

    #[test]
    fn inline_html_nested_beyond_the_limit_stays_raw() {
        let text = "<b>".repeat(MAX_PAIRED + 3) + "x" + &"</b>".repeat(MAX_PAIRED + 3);
        fn depth(inlines: &[Inline]) -> usize {
            inlines.iter().map(|i| 1 + depth(&i.children)).max().unwrap_or(0)
        }
        let doc = parse(&text);
        assert!(depth(doc.blocks[0].inlines()) <= MAX_PAIRED + 1);
    }

    #[test]
    fn an_html_link_without_an_address_is_no_link() {
        let doc = parse("<a>x</a> <a href=\"javascript:y\">z</a>\n");
        assert!(matches!(doc.blocks[0].inlines()[0].kind, InlineKind::Span));
    }

    #[test]
    fn an_empty_html_pair_has_its_content_between_the_tags() {
        let t = "<b title=\"äää\"></b> x\n";
        let d = parse(t);
        let b = &d.blocks[0].inlines()[0];
        assert_eq!(b.kind, InlineKind::Strong);
        assert_eq!(slice(t, &b.content_range()), "");
        assert_eq!(b.content_range().start, Byte::new(t.find("</b>").unwrap()));
    }

    #[test]
    fn inline_ranges_are_exact_with_multibyte_text() {
        let t = "Text **fett ü** und `co` <u>un</u> 😀 [li](http://x)\n";
        let d = parse(t);
        let p = &d.blocks[0];
        let strong = p.inlines().iter().find(|i| i.kind == InlineKind::Strong).unwrap();
        assert_eq!(slice(t, &strong.range), "**fett ü**");
        assert_eq!(slice(t, &strong.content_range()), "fett ü");
        let u = p.inlines().iter().find(|i| i.kind == InlineKind::Underline).unwrap();
        assert_eq!(slice(t, &u.range), "<u>un</u>");
        assert_eq!(slice(t, &u.content_range()), "un");
        let link = p
            .inlines()
            .iter()
            .find(|i| matches!(i.kind, InlineKind::Link { .. }))
            .unwrap();
        assert_eq!(slice(t, &link.range), "[li](http://x)");
    }

    #[test]
    fn unmatched_html_stays_raw() {
        let d = parse("a <u>b\n");
        assert!(
            d.blocks[0]
                .inlines()
                .iter()
                .any(|i| matches!(i.kind, InlineKind::Html(_)))
        );
    }

    #[test]
    fn escaped_text_keeps_literal() {
        let t = "a \\* b &amp; c\n";
        let d = parse(t);
        let text: String = d.blocks[0]
            .inlines()
            .iter()
            .filter_map(|i| match &i.kind {
                InlineKind::Text(s) => Some(&**s),
                _ => None,
            })
            .collect();
        assert_eq!(text, "a * b & c");
    }

    #[test]
    fn a_note_with_brackets_ends_at_its_own_bracket() {
        let reference = |t: &str| {
            let d = parse(t);
            let i = d.blocks[0]
                .inlines()
                .iter()
                .find(|i| matches!(i.kind, InlineKind::FootnoteRef { .. }))
                .unwrap()
                .clone();
            t.at(i.range).to_string()
        };
        assert_eq!(reference("a^[b [c] d] e"), "^[b [c] d]");
        assert_eq!(reference("a^[b [c [d]] e] f"), "^[b [c [d]] e]");
        assert_eq!(reference(r"a^[b \] c] d"), r"^[b \] c]");
    }

    #[test]
    fn a_hard_line_break_stays_a_break() {
        let d = parse("a  \nb\n");
        assert!(d.blocks[0].inlines().iter().any(|i| i.kind == InlineKind::LineBreak));
        let d = parse("a\nb\n");
        assert!(d.blocks[0].inlines().iter().any(|i| i.kind == InlineKind::SoftBreak));
    }

    #[test]
    fn inline_ranges_are_clamped_into_their_container() {
        let inline = |range: Range<usize>| Inline {
            kind: InlineKind::Emph,
            range: bytes(range),
            children: Box::default(),
        };
        let mut children = [Inline {
            children: Box::new([inline(0..50)]),
            ..inline(5..30)
        }];
        keep_inside(&mut children, &bytes(10..20));
        assert_eq!(children[0].range, bytes(10..20));
        assert_eq!(children[0].children[0].range, bytes(10..20));
    }

    #[test]
    fn single_html_tags_become_what_they_stand_for() {
        let kinds_of = |t: &str| {
            parse(t).blocks[0]
                .inlines()
                .iter()
                .map(|i| std::mem::discriminant(&i.kind))
                .collect::<Vec<_>>()
        };
        let text = std::mem::discriminant(&InlineKind::Text("".into()));
        let raw = std::mem::discriminant(&InlineKind::Html("".into()));
        let line = std::mem::discriminant(&InlineKind::LineBreak);
        assert_eq!(kinds_of("a<br>b"), [text, line, text]);
        assert_eq!(kinds_of("a</br>b"), [text, raw, text], "a closing tag breaks nothing");
        assert_eq!(kinds_of("a<b/>b"), [text, raw, text], "nothing to enclose");
        let t = "<a href=\"u\" title=\"t\">x</a>\n";
        let d = parse(t);
        assert_eq!(
            d.blocks[0].inlines()[0].kind,
            InlineKind::Link {
                url: "u".into(),
                title: "t".into()
            }
        );
    }

    #[test]
    fn a_closing_tag_of_another_kind_does_not_close() {
        let d = parse("<b>a</i> c\n");
        assert!(
            d.blocks[0].inlines().iter().all(|i| i.kind != InlineKind::Strong),
            "{d:?}"
        );
    }

    #[test]
    fn a_self_closing_tag_opens_nothing_a_later_one_could_close() {
        let d = parse("<b/>x</b> y\n");
        assert!(
            d.blocks[0].inlines().iter().all(|i| i.kind != InlineKind::Strong),
            "{d:?}"
        );
    }
}
