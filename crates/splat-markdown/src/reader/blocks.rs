use std::ops::Range;

use comrak::nodes::{AlertType, ListDelimType, ListType, NodeCodeBlock, NodeHeading, NodeValue, TableAlignment};

use super::Reader;
use crate::doc::{AlertKind, Align, Block, BlockKind, Fold, Inline, InlineKind, ListKind, Task};
use crate::text::{Byte, TextAt, container_mark, line_end, line_start, next_line_start};

fn alert_kind(alert: AlertType) -> AlertKind {
    match alert {
        AlertType::Note => AlertKind::Note,
        AlertType::Tip => AlertKind::Tip,
        AlertType::Important => AlertKind::Important,
        AlertType::Warning => AlertKind::Warning,
        AlertType::Caution => AlertKind::Caution,
    }
}

fn table_align(alignment: &TableAlignment) -> Align {
    match alignment {
        TableAlignment::None => Align::None,
        TableAlignment::Left => Align::Left,
        TableAlignment::Center => Align::Center,
        TableAlignment::Right => Align::Right,
    }
}

/// A callout type as the alert kind closest to it, and whether it is one of GitHub's.
fn callout_kind(name: &str) -> (AlertKind, bool) {
    match name.to_ascii_lowercase().as_str() {
        "note" => (AlertKind::Note, true),
        "tip" => (AlertKind::Tip, true),
        "important" => (AlertKind::Important, true),
        "warning" => (AlertKind::Warning, true),
        "caution" => (AlertKind::Caution, true),
        "hint" | "success" | "check" | "done" => (AlertKind::Tip, false),
        "question" | "help" | "faq" => (AlertKind::Important, false),
        "attention" => (AlertKind::Warning, false),
        "failure" | "fail" | "missing" | "danger" | "error" | "bug" => (AlertKind::Caution, false),
        _ => (AlertKind::Note, false),
    }
}

fn capitalized(name: &str) -> String {
    let lower = name.to_lowercase();
    let mut chars = lower.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

fn node_text<'a>(node: comrak::nodes::Node<'a>) -> String {
    node.descendants()
        .filter_map(|n| match &n.data().value {
            NodeValue::Text(t) => Some(t.to_string()),
            NodeValue::Code(c) => Some(c.literal.clone()),
            NodeValue::SoftBreak | NodeValue::LineBreak => Some(" ".to_string()),
            NodeValue::Document
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
            | NodeValue::HtmlInline(_)
            | NodeValue::Raw(_)
            | NodeValue::Emph
            | NodeValue::Strong
            | NodeValue::Strikethrough
            | NodeValue::Highlight
            | NodeValue::Insert
            | NodeValue::Superscript
            | NodeValue::Link(_)
            | NodeValue::Image(_)
            | NodeValue::FootnoteReference(_)
            | NodeValue::ShortCode(_)
            | NodeValue::Math(_)
            | NodeValue::MultilineBlockQuote(_)
            | NodeValue::Escaped
            | NodeValue::WikiLink(_)
            | NodeValue::Underline
            | NodeValue::Subscript
            | NodeValue::SpoileredText
            | NodeValue::EscapedTag(_)
            | NodeValue::Alert(_)
            | NodeValue::Callout(_)
            | NodeValue::CalloutTitle
            | NodeValue::Subtext
            | NodeValue::BlockDirective(_)
            | NodeValue::CommentInline(_)
            | NodeValue::MultilineComment(_) => None,
        })
        .collect()
}

impl Reader<'_> {
    /// The inlines of a leaf, their positions checked against the source; if those cannot be told, the source as it
    /// stands, which shows the markup but edits what it says.
    fn leaf_inlines<'a>(&self, node: comrak::nodes::Node<'a>, range: &Range<Byte>) -> Vec<Inline> {
        super::positions::checked(self.text, range, self.inlines(node)).unwrap_or_else(|| {
            vec![Inline {
                kind: InlineKind::Text(self.text.at(range.clone()).replace(['\r', '\n'], " ").into()),
                range: range.clone(),
                children: Box::default(),
            }]
        })
    }

    /// A paragraph, or the formula it holds and nothing else.
    fn paragraph<'a>(&self, node: comrak::nodes::Node<'a>, mut range: Range<Byte>) -> Block {
        let inlines = self.leaf_inlines(node, &range);
        // Link reference definitions at its start are part of comrak's paragraph, but of nothing it shows.
        if let Some(first) = inlines.first() {
            range.start = range.start.max(first.range.start.min(range.end));
        }
        if let [only] = inlines.as_slice()
            && let InlineKind::Math { display: true, literal } = &only.kind
        {
            return Block::leaf(
                BlockKind::MathBlock {
                    literal: literal.clone(),
                },
                range,
                Box::default(),
            );
        }
        Block::leaf(BlockKind::Paragraph, range, inlines)
    }

    fn callout<'a>(&self, node: comrak::nodes::Node<'a>, callout_type: &str, range: &Range<Byte>) -> BlockKind {
        let (kind, github) = callout_kind(callout_type);
        let custom = node
            .children()
            .find(|n| matches!(n.data().value, NodeValue::CalloutTitle))
            .map(node_text)
            .filter(|t| !t.trim().is_empty());
        let title = custom.or_else(|| (!github).then(|| capitalized(callout_type)));
        let marker = self.alert_marker(range);
        let sign = self.text.at(marker.clone()).find("]").map(|i| marker.start + i + 1);
        let fold = sign.and_then(|at| match self.text.as_bytes().get(at.get()) {
            Some(b'-') => Some(Fold { open: false, sign: at }),
            Some(b'+') => Some(Fold { open: true, sign: at }),
            _ => None,
        });
        BlockKind::Alert {
            kind,
            title: title.map(Into::into),
            marker,
            fold,
        }
    }

    fn list_kind(&self, list: &comrak::nodes::NodeList, range: &Range<Byte>) -> ListKind {
        match list.list_type {
            ListType::Bullet => ListKind::Bullet(self.text.from(range.start).chars().next().unwrap_or('-')),
            ListType::Ordered => ListKind::Ordered {
                start: list.start,
                delim: match list.delimiter {
                    ListDelimType::Period => '.',
                    ListDelimType::Paren => ')',
                },
            },
        }
    }

    pub(super) fn block<'a>(&self, node: comrak::nodes::Node<'a>) -> Option<Block> {
        let data = node.data();
        let range = self.range(data.sourcepos);
        let Some(_level) = self.enter() else {
            let raw = self.raw_text(range.clone());
            return Some(Block::leaf(BlockKind::Paragraph, range, [raw]));
        };
        let kind = match &data.value {
            NodeValue::Paragraph => return Some(self.paragraph(node, range)),
            NodeValue::Heading(h) => return Some(self.heading(node, h, range)),
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => BlockKind::BlockQuote,
            NodeValue::Alert(a) => BlockKind::Alert {
                kind: alert_kind(a.alert_type),
                title: a.title.as_deref().map(Into::into),
                marker: self.alert_marker(&range),
                fold: None,
            },
            NodeValue::Callout(c) => self.callout(node, &c.callout_type, &range),
            NodeValue::CalloutTitle => return None,
            NodeValue::DescriptionList => BlockKind::DefinitionList,
            NodeValue::DescriptionItem(_) => BlockKind::DefinitionItem,
            NodeValue::DescriptionTerm => BlockKind::DefinitionTerm,
            NodeValue::DescriptionDetails => BlockKind::DefinitionDetails,
            NodeValue::MultilineComment(_) => BlockKind::Comment,
            NodeValue::List(l) => BlockKind::List {
                kind: self.list_kind(l, &range),
                tight: l.tight,
            },
            NodeValue::Item(_) => BlockKind::Item {
                task: None,
                marker: self.marker(range.start),
            },
            NodeValue::TaskItem(t) => BlockKind::Item {
                task: Some(Task {
                    checked: t.symbol.is_some(),
                    symbol: self.range(t.symbol_sourcepos).start,
                }),
                marker: self.marker(range.start),
            },
            NodeValue::CodeBlock(c) => self.code_block(c, &range),
            NodeValue::HtmlBlock(h) => BlockKind::HtmlBlock {
                literal: h.literal.as_str().into(),
            },
            NodeValue::ThematicBreak => BlockKind::ThematicBreak,
            NodeValue::FrontMatter(_) => BlockKind::FrontMatter {
                content: self.front_matter_content(&range),
            },
            NodeValue::Table(t) => BlockKind::Table {
                alignments: t.alignments.iter().map(table_align).collect(),
            },
            NodeValue::TableRow(header) => BlockKind::TableRow { header: *header },
            NodeValue::TableCell => return Some(self.table_cell(node, range)),
            NodeValue::FootnoteDefinition(f) => {
                let label_end = self.text.at(range.clone()).find("]:").map(|i| range.start + i + 2);
                BlockKind::FootnoteDefinition {
                    name: f.name.as_str().into(),
                    label: range.start..label_end.unwrap_or(range.start),
                    number: None,
                }
            }
            NodeValue::Document
            | NodeValue::Text(_)
            | NodeValue::SoftBreak
            | NodeValue::LineBreak
            | NodeValue::Code(_)
            | NodeValue::HtmlInline(_)
            | NodeValue::Raw(_)
            | NodeValue::Emph
            | NodeValue::Strong
            | NodeValue::Strikethrough
            | NodeValue::Highlight
            | NodeValue::Insert
            | NodeValue::Superscript
            | NodeValue::Link(_)
            | NodeValue::Image(_)
            | NodeValue::FootnoteReference(_)
            | NodeValue::ShortCode(_)
            | NodeValue::Math(_)
            | NodeValue::Escaped
            | NodeValue::WikiLink(_)
            | NodeValue::Underline
            | NodeValue::Subscript
            | NodeValue::SpoileredText
            | NodeValue::EscapedTag(_)
            | NodeValue::Subtext
            | NodeValue::BlockDirective(_)
            | NodeValue::CommentInline(_) => return None,
        };
        drop(data);
        let children: Vec<Block> = node.children().filter_map(|c| self.block(c)).collect();
        Some(Block::container(kind, range, children))
    }

    fn heading<'a>(&self, node: comrak::nodes::Node<'a>, heading: &NodeHeading, range: Range<Byte>) -> Block {
        let data = node.data();
        let attributes = data.attrs.as_ref();
        let kind = BlockKind::Heading {
            level: heading.level,
            setext: heading.setext,
            id: attributes.and_then(|a| a.id.as_deref()).map(Into::into),
            attrs: attributes.and_then(|_| self.heading_attrs(&range)),
        };
        let inlines = self.leaf_inlines(node, &range);
        Block::leaf(kind, range, inlines)
    }

    fn code_block(&self, code: &NodeCodeBlock, range: &Range<Byte>) -> BlockKind {
        BlockKind::CodeBlock {
            info: code.info.as_str().into(),
            literal: code.literal.as_str().into(),
            fenced: code.fenced,
            content: self.code_content(range, code.fenced, code.closed),
        }
    }

    /// What stands between the delimiter lines of front matter.
    fn front_matter_content(&self, range: &Range<Byte>) -> Range<Byte> {
        let slice = self.text.at(range.clone());
        let start = next_line_start(slice, Byte::default()).unwrap_or(slice.end());
        let trimmed = slice.trim_end();
        let end = line_start(trimmed, trimmed.end());
        let (start, end) = (range.start + start.get(), range.start + end.get());
        start..end.max(start)
    }

    fn table_cell<'a>(&self, node: comrak::nodes::Node<'a>, range: Range<Byte>) -> Block {
        *self.escapes.borrow_mut() = self
            .text
            .at(range.clone())
            .match_indices("\\|")
            .map(|(i, _)| range.start + i)
            .collect();
        let inlines = self.leaf_inlines(node, &range);
        self.escapes.borrow_mut().clear();
        Block::leaf(BlockKind::TableCell, range, inlines)
    }

    /// The `{…}` at the end of a heading's first line.
    fn heading_attrs(&self, range: &Range<Byte>) -> Option<Range<Byte>> {
        let line = self.text.at(range.clone());
        let line = line.upto(line_end(line, Byte::default()));
        let trimmed = line.trim_end().trim_end_matches('#').trim_end();
        let open = trimmed.ends_with('}').then(|| trimmed.rfind('{')).flatten()?;
        Some(range.start + open..range.start + trimmed.len())
    }

    fn alert_marker(&self, range: &Range<Byte>) -> Range<Byte> {
        let first_line = line_end(self.text, range.start).min(range.end);
        let start = self
            .text
            .at(range.start..first_line)
            .find("[!")
            .map_or(range.start, |i| range.start + i);
        let end = range.start + self.text.at(range.start..first_line).trim_end().len();
        start..end.max(start)
    }

    fn marker(&self, start: Byte) -> Range<Byte> {
        let len = container_mark(self.text.from(start)).map_or(0, |(_, len)| len);
        start..start + len
    }

    fn code_content(&self, range: &Range<Byte>, fenced: bool, closed: bool) -> Range<Byte> {
        let slice = self.text.at(range.clone());
        if !fenced {
            return range.clone();
        }
        let start = next_line_start(slice, Byte::default()).map_or(range.end, |at| range.start + at.get());
        let trimmed = slice.trim_end_matches(['\n', '\r']);
        let last_line_start = range.start + line_start(trimmed, trimmed.end()).get();
        let end = if closed && last_line_start > range.start {
            last_line_start
        } else {
            range.end
        };
        start.min(end)..end
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse;
    use super::super::tests::slice;
    use super::*;
    use crate::text::bytes;

    #[test]
    fn the_last_line_of_a_fence_is_code_unless_it_closes_the_fence() {
        let content = |text: &str| {
            let doc = parse(text);
            let BlockKind::CodeBlock { content, literal, .. } = &doc.blocks[0].kind else {
                panic!("{:?}", doc.blocks[0].kind);
            };
            assert_eq!(text.at(content.clone()).trim_end(), literal.trim_end(), "{text:?}");
            text.at(content.clone()).to_string()
        };
        assert_eq!(content("```\ncode\n```\n"), "code\n");
        assert_eq!(content("````\ncode\n```\n"), "code\n```");
        assert_eq!(content("```\ncode\n~~~\n"), "code\n~~~");
        assert_eq!(content("```\ncode\n"), "code");
        assert_eq!(content("~~~\n~~~\n"), "");
    }

    #[test]
    fn front_matter_is_one_block_before_the_text() {
        let text = "---\ntitle: Splat\ntags: [a, b]\n---\n\n# Kopf\n";
        let doc = parse(text);
        let BlockKind::FrontMatter { content } = &doc.blocks[0].kind else {
            panic!("{:?}", doc.blocks[0].kind);
        };
        assert_eq!(text.at(content.clone()), "title: Splat\ntags: [a, b]\n");
        assert_eq!(
            text.at(doc.blocks[0].range.clone()),
            "---\ntitle: Splat\ntags: [a, b]\n---"
        );
        assert!(matches!(doc.blocks[1].kind, BlockKind::Heading { level: 1, .. }));
        // Further down, `---` stays a rule.
        assert!(matches!(
            parse("Text\n\n---\n").blocks[1].kind,
            BlockKind::ThematicBreak
        ));
    }

    #[test]
    fn blocks_and_markers() {
        let t =
            "# Ü\n\n- [x] done\n10. ten\n\n> q\n\n| a | b |\n|---|:-:|\n| 1 | 2 |\n\n```rs\nfn x\n```\n\n$$\ny\n$$\n";
        let d = parse(t);
        assert!(matches!(d.blocks[0].kind, BlockKind::Heading { level: 1, .. }));
        let item = &d.blocks[1].children()[0];
        match &item.kind {
            BlockKind::Item {
                task: Some(task),
                marker,
            } => {
                assert!(task.checked);
                assert_eq!(slice(t, marker), "-");
                assert_eq!(t.at(task.symbol..task.symbol + 1), "x");
            }
            k => panic!("{k:?}"),
        }
        match &d.blocks[2].children()[0].kind {
            BlockKind::Item { marker, .. } => assert_eq!(slice(t, marker), "10."),
            k => panic!("{k:?}"),
        }
        assert!(matches!(d.blocks[3].kind, BlockKind::BlockQuote));
        match &d.blocks[4].kind {
            BlockKind::Table { alignments } => assert_eq!(&**alignments, &[Align::None, Align::Center]),
            k => panic!("{k:?}"),
        }
        match &d.blocks[5].kind {
            BlockKind::CodeBlock { content, info, .. } => {
                assert_eq!(&**info, "rs");
                assert_eq!(slice(t, content), "fn x\n");
            }
            k => panic!("{k:?}"),
        }
        assert!(matches!(d.blocks[6].kind, BlockKind::MathBlock { .. }));
    }

    #[test]
    fn alerts_and_highlights() {
        let t = "> [!WARNING] Achtung\n> Text <mark>wichtig</mark>\n\n> normal\n";
        let d = parse(t);
        match &d.blocks[0].kind {
            BlockKind::Alert {
                kind, title, marker, ..
            } => {
                assert_eq!(*kind, AlertKind::Warning);
                assert_eq!(title.as_deref(), Some("Achtung"));
                assert_eq!(slice(t, marker), "[!WARNING] Achtung");
            }
            k => panic!("{k:?}"),
        }
        let para = &d.blocks[0].children()[0];
        let mark = para.inlines().iter().find(|i| i.kind == InlineKind::Highlight).unwrap();
        assert_eq!(slice(t, &mark.content_range()), "wichtig");
        assert!(matches!(d.blocks[1].kind, BlockKind::BlockQuote));
    }

    #[test]
    fn every_callout_type_finds_the_alert_kind_closest_to_it() {
        for (name, kind, github) in [
            ("note", AlertKind::Note, true),
            ("TIP", AlertKind::Tip, true),
            ("important", AlertKind::Important, true),
            ("warning", AlertKind::Warning, true),
            ("caution", AlertKind::Caution, true),
            ("hint", AlertKind::Tip, false),
            ("done", AlertKind::Tip, false),
            ("faq", AlertKind::Important, false),
            ("attention", AlertKind::Warning, false),
            ("bug", AlertKind::Caution, false),
            ("abstract", AlertKind::Note, false),
        ] {
            assert_eq!(callout_kind(name), (kind, github), "{name}");
        }
    }

    #[test]
    fn a_callout_without_a_title_shows_its_type_and_its_title_is_no_block() {
        let t = "> [!danger]\n> Text\n";
        let BlockKind::Alert { kind, title, .. } = &parse(t).blocks[0].kind.clone() else {
            panic!("no alert");
        };
        assert_eq!(*kind, AlertKind::Caution);
        assert_eq!(title.as_deref(), Some("Danger"));
        let t = "> [!NOTE] Eigener `Titel`\n> Text\n";
        let d = parse(t);
        let BlockKind::Alert { title, .. } = &d.blocks[0].kind else {
            panic!("no alert");
        };
        assert_eq!(title.as_deref(), Some("Eigener Titel"));
        assert_eq!(d.blocks[0].children().len(), 1);
        assert_eq!(slice(t, &d.blocks[0].children()[0].range), "Text");
    }

    #[test]
    fn the_label_of_a_footnote_definition_ends_behind_its_colon() {
        let t = "x[^a]\n\n[^a]: b\n";
        let d = parse(t);
        let BlockKind::FootnoteDefinition { label, .. } = &d.blocks[1].kind else {
            panic!("no definition");
        };
        assert_eq!(slice(t, label), "[^a]:");
    }

    #[test]
    fn a_fence_with_text_behind_it_does_not_close_the_code() {
        let content = |t: &str| {
            let d = parse(t);
            let b = d
                .blocks
                .iter()
                .find(|b| matches!(b.kind, BlockKind::CodeBlock { .. }))
                .unwrap();
            let BlockKind::CodeBlock { content, .. } = &b.kind else {
                unreachable!();
            };
            t.at(content.clone()).to_string()
        };
        assert_eq!(content("```\na\n```\n"), "a\n");
        assert_eq!(content("```\na\n```x\n"), "a\n```x");
        assert_eq!(content("~~~\na\n~~~\n"), "a\n");
        assert_eq!(content("```\na\nb\n"), "a\nb");
        assert_eq!(content("x\n\n```"), "");
        let t = "x\n\n```";
        let d = parse(t);
        let BlockKind::CodeBlock { content, .. } = &d.blocks[1].kind else {
            panic!("no code");
        };
        assert_eq!(*content, bytes(t.len()..t.len()));
    }
}
