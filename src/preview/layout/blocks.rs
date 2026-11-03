use std::ops::Range;

use super::inlines::{atom, atom_styled};
use super::{
    ATTACHMENT, Atom, Builder, CharMap, Ctx, Lead, Link, MAX_EMBEDS, Marker, Para, ParaKind, ParaStyle, Run, RunMap,
    RunStyle, WithoutEmbeds, layout,
};
use crate::markdown::doc::{
    AlertKind, Align, Block, BlockKind, Fold, Inline, InlineKind, ListKind, diagram_kind, is_image_name, is_math_fence,
};
use crate::markdown::reader::parse;
use crate::markdown::style::TextStyle;
use crate::markdown::text::{Byte, TextAt, count32, right_to_left};
use crate::markdown::typography::Open;

impl Builder<'_> {
    pub(super) fn next_group(&mut self) -> usize {
        self.group += 1;
        self.group
    }

    pub(super) fn push_para(
        &mut self,
        kind: ParaKind,
        ctx: &Ctx,
        marker: Lead,
        src: Range<Byte>,
        mut runs: Vec<Run>,
        insert_at: Byte,
    ) {
        let rtl = if matches!(kind, ParaKind::Code { .. }) {
            Some(false)
        } else {
            right_to_left(&runs.iter().map(|r| r.text.as_str()).collect::<String>())
        };
        let marker = marker.map(|(m, marker_src)| {
            runs.insert(0, atom(Atom::Marker, &marker_text(&m), marker_src));
            m
        });
        self.paras.push(Para {
            style: ParaStyle {
                kind,
                quote_depth: ctx.quote_depth,
                list_depth: ctx.list_depth,
                marker,
                group: ctx.group,
                tight: ctx.tight,
                space_before: false,
                alert: ctx.alert,
                align: ctx.align,
                html: ctx.html,
                rtl,
                mirrored: false,
            },
            runs,
            src,
            insert_at,
        });
        self.open.set(Open::default());
    }

    /// Mirrors the paragraphs of `group` since `start` when the group's text reads right to left; callout titles and
    /// code do not count, as they keep their own language, but a title takes the direction of its callout.
    pub(super) fn set_direction(&mut self, start: usize, group: usize) {
        let text: String = self.paras[start..]
            .iter()
            .filter(|p| p.style.group == group && !matches!(p.style.kind, ParaKind::AlertTitle | ParaKind::Code { .. }))
            .flat_map(|p| &p.runs)
            .filter(|r| !matches!(r.map, RunMap::Atom { atom: Atom::Marker, .. }))
            .map(|r| r.text.as_str())
            .collect();
        // A callout without letters in its body, or folded, reads in the direction of its title.
        let title = self.paras[start..]
            .iter()
            .find(|p| p.style.group == group && p.style.kind == ParaKind::AlertTitle)
            .and_then(|p| p.style.rtl);
        let mirrored = right_to_left(&text).or(title) == Some(true);
        for p in &mut self.paras[start..] {
            if p.style.group == group {
                p.style.mirrored = mirrored;
                if p.style.kind == ParaKind::AlertTitle {
                    p.style.rtl = Some(mirrored);
                }
            }
        }
        if mirrored {
            // TextKit places table cells left to right in text order, whatever column they name. Tables nested in a
            // quote or callout have a group of their own and were mirrored by their own call.
            let row = |p: &Para| match p.style.kind {
                ParaKind::TableCell { table, row, .. } if p.style.group == group => Some((table, row)),
                _ => None,
            };
            for cells in self.paras[start..].chunk_by_mut(|a, b| row(a).is_some() && row(a) == row(b)) {
                cells.reverse();
            }
        }
    }

    pub(super) fn block(&mut self, block: &Block, ctx: &Ctx, marker: Lead) {
        match &block.kind {
            BlockKind::Paragraph => self.paragraph(block, ctx, marker),
            BlockKind::Heading { level, setext, .. } => self.heading(block, ctx, marker, *level, *setext),
            BlockKind::BlockQuote => self.quote(block, ctx, marker),
            BlockKind::Alert {
                kind,
                title,
                marker: title_src,
                fold,
            } => self.alert(block, ctx, marker, *kind, title.as_deref(), title_src, fold.as_ref()),
            BlockKind::DefinitionList => self.definition_list(block, ctx, marker),
            BlockKind::Comment => {}
            BlockKind::List { kind, tight } => self.list(block, ctx, kind, *tight),
            BlockKind::CodeBlock {
                info, literal, content, ..
            } => self.code_block(block, ctx, marker, info, literal, content),
            BlockKind::HtmlBlock { literal } => self.html_block(literal, block.range.clone(), ctx, marker),
            BlockKind::FrontMatter { content } => self.front_matter(block, ctx, marker, content),
            BlockKind::ThematicBreak => {
                let run = atom(Atom::Rule, "\u{00A0}", block.range.clone());
                self.leaf_para(ParaKind::Rule, block, ctx, marker, run);
            }
            BlockKind::MathBlock { literal } => self.math_block(block, ctx, marker, literal),
            BlockKind::Table { alignments } => self.table(block, ctx, marker, alignments),
            BlockKind::FootnoteDefinition { name, label, number } => {
                self.footnote_definition(block, ctx, name, label, *number);
            }
            BlockKind::Item { .. }
            | BlockKind::TableRow { .. }
            | BlockKind::TableCell
            | BlockKind::DefinitionItem
            | BlockKind::DefinitionTerm
            | BlockKind::DefinitionDetails => {}
        }
    }

    /// A paragraph of one atom that stands for the whole block.
    fn leaf_para(&mut self, kind: ParaKind, block: &Block, ctx: &Ctx, marker: Lead, run: Run) {
        self.push_para(kind, ctx, marker, block.range.clone(), vec![run], block.range.start);
    }

    fn paragraph(&mut self, block: &Block, ctx: &Ctx, marker: Lead) {
        if let Some(target) = only_embed(block.inlines()) {
            if self.embeds >= MAX_EMBEDS {
                self.embeds_refused = true;
            } else if let Some((note, folder)) = self.resolver.embedded_note(target) {
                self.embeds += 1;
                self.embed(&note, &folder, block.range.clone(), ctx);
                return;
            }
        }
        let runs = self.inlines(block.inlines(), &RunStyle::default());
        let insert_at = block.inline_range().map_or(block.range.start, |r| r.start);
        self.push_para(ParaKind::Body, ctx, marker, block.range.clone(), runs, insert_at);
    }

    fn heading(&mut self, block: &Block, ctx: &Ctx, marker: Lead, level: u8, setext: bool) {
        let runs = self.inlines(block.inlines(), &RunStyle::default());
        let insert_at = block.inline_range().map_or_else(
            || {
                if setext {
                    block.range.start
                } else {
                    self.heading_insert(&block.range)
                }
            },
            |r| r.start,
        );
        self.push_para(
            ParaKind::Heading(level),
            ctx,
            marker,
            block.range.clone(),
            runs,
            insert_at,
        );
    }

    fn quote(&mut self, block: &Block, ctx: &Ctx, marker: Lead) {
        let inner = Ctx {
            quote_depth: ctx.quote_depth + 1,
            group: self.next_group(),
            ..ctx.clone()
        };
        let start = self.paras.len();
        let mut marker = marker;
        if block.children().is_empty() {
            self.push_para(
                ParaKind::Body,
                &inner,
                marker.take(),
                block.range.clone(),
                vec![],
                block.range.end,
            );
        }
        for child in block.children() {
            self.block(child, &inner, marker.take());
        }
        self.set_direction(start, inner.group);
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the parts of a callout, each a value of its own"
    )]
    fn alert(
        &mut self,
        block: &Block,
        ctx: &Ctx,
        marker: Lead,
        kind: AlertKind,
        title: Option<&str>,
        title_src: &Range<Byte>,
        fold: Option<&Fold>,
    ) {
        let inner = Ctx {
            quote_depth: ctx.quote_depth + 1,
            group: self.next_group(),
            alert: Some(kind),
            ..ctx.clone()
        };
        let label = title.unwrap_or(crate::i18n::t(kind.label()));
        let mut runs = vec![
            atom(Atom::AlertIcon(kind), &ATTACHMENT.to_string(), title_src.clone()),
            atom(Atom::Text, &format!(" {label}"), title_src.clone()),
        ];
        if let Some(f) = fold {
            runs.push(atom_styled(
                Atom::Fold { open: f.open },
                if f.open { " ▾" } else { " ▸" },
                f.sign..f.sign + 1,
                &RunStyle {
                    link: Some(Link::Fold),
                    ..Default::default()
                },
            ));
        }
        let start = self.paras.len();
        self.push_para(
            ParaKind::AlertTitle,
            &inner,
            marker,
            block.range.clone(),
            runs,
            title_src.end,
        );
        if fold.is_none_or(|f| f.open) {
            for child in block.children() {
                self.block(child, &inner, None);
            }
        }
        self.set_direction(start, inner.group);
    }

    fn definition_list(&mut self, block: &Block, ctx: &Ctx, marker: Lead) {
        let mut marker = marker;
        for item in block.children() {
            for part in item.children() {
                let (inner, bold) = match part.kind {
                    BlockKind::DefinitionTerm => (ctx.clone(), true),
                    _ => (
                        Ctx {
                            list_depth: ctx.list_depth + 1,
                            ..ctx.clone()
                        },
                        false,
                    ),
                };
                for child in part.children() {
                    let start = self.paras.len();
                    self.block(child, &inner, marker.take());
                    if bold {
                        for p in &mut self.paras[start..] {
                            for r in &mut p.runs {
                                r.style.text.bold = true;
                            }
                        }
                    }
                }
            }
        }
    }

    fn list(&mut self, block: &Block, ctx: &Ctx, kind: &ListKind, tight: bool) {
        let depth = ctx.list_depth;
        for (index, item) in block.children().iter().enumerate() {
            let BlockKind::Item {
                task,
                marker: marker_range,
            } = &item.kind
            else {
                continue;
            };
            let item_marker = match (task, kind) {
                (Some(t), _) => Marker::Task {
                    checked: t.checked,
                    symbol: Some(t.symbol),
                },
                (None, ListKind::Bullet(_)) => Marker::Bullet(depth),
                (None, ListKind::Ordered { start, .. }) => Marker::Number(start + index),
            };
            let marker_end = task
                .as_ref()
                .map_or(marker_range.end, |t| (t.symbol + 2).min(self.text.end()));
            let item_marker = (item_marker, marker_range.start..marker_end);
            let inner = Ctx {
                list_depth: depth + 1,
                tight,
                ..ctx.clone()
            };
            let mut first = Some(item_marker);
            if item.children().is_empty() {
                let after = task.as_ref().map_or(marker_range.end, |t| t.symbol + 2);
                let insert_at = (after + 1).min(item.range.end.max(after));
                self.push_para(
                    ParaKind::Body,
                    &inner,
                    first.take(),
                    item.range.clone(),
                    vec![],
                    insert_at,
                );
            }
            for child in item.children() {
                self.block(child, &inner, first.take());
            }
        }
    }

    fn code_block(&mut self, block: &Block, ctx: &Ctx, marker: Lead, info: &str, literal: &str, content: &Range<Byte>) {
        let lang = info.split_whitespace().next().unwrap_or("").to_string();
        let group = self.next_group();
        let inner = Ctx { group, ..ctx.clone() };
        let body = literal.strip_suffix('\n').unwrap_or(literal);
        if is_math_fence(info) && self.resolver.math_ready(body, true) {
            let run = atom(
                Atom::Math {
                    tex: body.into(),
                    display: true,
                },
                &ATTACHMENT.to_string(),
                block.range.clone(),
            );
            self.push_para(
                ParaKind::MathBlock,
                ctx,
                marker,
                block.range.clone(),
                vec![run],
                block.range.start,
            );
            return;
        }
        if let Some(kind) = diagram_kind(info) {
            let ready = self.resolver.diagram_ready(literal, kind);
            if ready {
                let run = atom(
                    Atom::Diagram {
                        source: literal.into(),
                        kind,
                    },
                    &ATTACHMENT.to_string(),
                    block.range.clone(),
                );
                self.push_para(
                    ParaKind::Diagram,
                    &inner,
                    marker,
                    block.range.clone(),
                    vec![run],
                    block.range.start,
                );
                return;
            }
        }
        let runs = if body.is_empty() {
            vec![]
        } else {
            vec![ending_at_last_char(self.aligned(
                body,
                content.clone(),
                RunStyle::default().code(),
            ))]
        };
        self.push_para(
            ParaKind::Code { info: lang.into() },
            &inner,
            marker,
            block.range.clone(),
            runs,
            content.start,
        );
    }

    fn front_matter(&mut self, block: &Block, ctx: &Ctx, marker: Lead, content: &Range<Byte>) {
        let keys: Vec<&str> = self
            .text
            .at(content.clone())
            .lines()
            .filter(|l| !l.starts_with([' ', '\t', '-', '#']))
            .filter_map(|l| l.split_once(':').map(|(k, _)| k.trim()))
            .filter(|k| !k.is_empty())
            .collect();
        let label = crate::i18n::t("Metadaten");
        let summary = if keys.is_empty() {
            label.to_string()
        } else {
            format!("{label}: {}", keys.join(", "))
        };
        let run = Run {
            text: summary,
            style: RunStyle::default(),
            map: RunMap::Atom {
                atom: Atom::Text,
                src: block.range.clone(),
            },
        };
        self.push_para(
            ParaKind::FrontMatter,
            ctx,
            marker,
            block.range.clone(),
            vec![run],
            block.range.start,
        );
    }

    fn math_block(&mut self, block: &Block, ctx: &Ctx, marker: Lead, literal: &str) {
        let run = if self.resolver.math_ready(literal, true) {
            atom(
                Atom::Math {
                    tex: literal.into(),
                    display: true,
                },
                &ATTACHMENT.to_string(),
                block.range.clone(),
            )
        } else {
            let shown = format!("$${}$$", literal);
            Run {
                text: shown,
                style: RunStyle::default().code(),
                map: RunMap::Atom {
                    atom: Atom::Text,
                    src: block.range.clone(),
                },
            }
        };
        self.push_para(
            ParaKind::MathBlock,
            ctx,
            marker,
            block.range.clone(),
            vec![run],
            block.range.start,
        );
    }

    fn table(&mut self, block: &Block, ctx: &Ctx, marker: Lead, alignments: &[Align]) {
        self.table += 1;
        let table = self.table;
        let group = self.next_group();
        let inner = Ctx { group, ..ctx.clone() };
        let columns = alignments.len();
        let start = self.paras.len();
        let mut marker = marker;
        for (row_index, row) in block.children().iter().enumerate() {
            let header = matches!(row.kind, BlockKind::TableRow { header: true });
            for (col, &align) in alignments.iter().enumerate() {
                let kind = ParaKind::TableCell {
                    table: count32(table),
                    row: count32(row_index),
                    col: count32(col),
                    header,
                    align,
                    columns: count32(columns),
                    span: 1,
                };
                match row.children().get(col) {
                    Some(cell) => {
                        let base = RunStyle::default().with_text(|text| TextStyle { bold: header, ..text });
                        let runs = self.inlines(cell.inlines(), &base);
                        let insert_at = cell
                            .inline_range()
                            .map_or_else(|| cell_insert(self.text, &cell.range), |r| r.start);
                        self.push_para(kind, &inner, marker.take(), cell.range.clone(), runs, insert_at);
                    }
                    None => {
                        let at = row.range.end;
                        self.push_para(kind, &inner, marker.take(), at..at, vec![], at);
                    }
                }
            }
        }
        self.set_direction(start, group);
    }

    fn footnote_definition(&mut self, block: &Block, ctx: &Ctx, name: &str, label: &Range<Byte>, number: Option<u32>) {
        let inner = Ctx {
            group: self.next_group(),
            ..ctx.clone()
        };
        let shown = number.map_or_else(|| name.into(), |n| n.to_string().into());
        let mut first = Some((Marker::Footnote(name.into(), shown), label.clone()));
        if block.children().is_empty() {
            self.push_para(
                ParaKind::FootnoteDef,
                &inner,
                first.take(),
                block.range.clone(),
                vec![],
                label.end,
            );
        }
        for child in block.children() {
            let start = self.paras.len();
            self.block(child, &inner, first.take());
            for p in &mut self.paras[start..] {
                if p.style.kind == ParaKind::Body {
                    p.style.kind = ParaKind::FootnoteDef;
                }
            }
        }
    }

    fn heading_insert(&self, range: &Range<Byte>) -> Byte {
        let src = self.text.at(range.clone());
        let hashes = src.bytes().take_while(|&b| b == b'#').count();
        (range.start + hashes + 1).min(range.end)
    }

    /// The paragraphs of an embedded note, framed like a quote; they show the note but edit nothing in it.
    fn embed(&mut self, note: &str, folder: &str, src: Range<Byte>, ctx: &Ctx) {
        let doc = parse(note);
        let inner = layout(&doc, &WithoutEmbeds(self.resolver), None);
        let mut groups = std::collections::HashMap::new();
        let frame = self.next_group();
        for mut p in inner
            .blocks
            .into_iter()
            .flat_map(|b| std::sync::Arc::unwrap_or_clone(b).paras)
        {
            p.style.quote_depth += ctx.quote_depth + 1;
            p.style.group = match p.style.group {
                0 => frame,
                g => *groups.entry(g).or_insert_with(|| self.next_group()),
            };
            if let Some(Marker::Task { symbol, .. }) = &mut p.style.marker {
                *symbol = None;
            }
            for run in &mut p.runs {
                let atom = match std::mem::replace(
                    &mut run.map,
                    RunMap::Text {
                        chars: CharMap::Verbatim(Byte::default()),
                        end: Byte::default(),
                    },
                ) {
                    RunMap::Atom {
                        atom: Atom::Image(mut image),
                        ..
                    } => {
                        if !image.url.contains(':') && !image.url.starts_with('/') {
                            image.url = format!("{folder}/{}", image.url).into();
                        }
                        Atom::Image(image)
                    }
                    RunMap::Atom { atom, .. } => atom,
                    RunMap::Text { .. } => Atom::Text,
                };
                run.map = RunMap::Atom { atom, src: src.clone() };
            }
            p.src = src.clone();
            p.insert_at = src.end;
            self.paras.push(p);
        }
    }
}

/// A code block's source goes on behind its text with the line end: typing at the end of the text must not go there.
fn ending_at_last_char(mut run: Run) -> Run {
    if let RunMap::Text { chars, end } = &mut run.map
        && let Some((k, last)) = run.text.chars().enumerate().last()
        && let Some(at) = chars.get(&run.text, k)
    {
        // A tab that comrak counts as several spaces stands for more characters than the source has.
        *end = (at + last.len_utf8()).min(*end);
    }
    run
}

/// The target of a paragraph that is nothing but `![[note]]`.
fn only_embed(inlines: &[Inline]) -> Option<&str> {
    let mut found = None;
    for i in inlines {
        match &i.kind {
            InlineKind::WikiLink { target, embed: true } if found.is_none() && !is_image_name(target) => {
                found = Some(&**target)
            }
            InlineKind::Text(t) if t.trim().is_empty() => {}
            InlineKind::SoftBreak => {}
            _ => return None,
        }
    }
    found
}

pub(super) fn marker_text(m: &Marker) -> String {
    match m {
        Marker::Bullet(depth) => format!("{}\t", ["•", "◦", "▪"][*depth as usize % 3]),
        Marker::Number(n) => format!("{n}.\t"),
        Marker::Task { checked, .. } => format!("{}\t", if *checked { "☑" } else { "☐" }),
        Marker::Footnote(_, shown) => format!("{shown}.\t"),
    }
}

fn cell_insert(text: &str, range: &Range<Byte>) -> Byte {
    let s = text.at(range.clone());
    let lead = s.len() - s.trim_start().len();
    if lead > 0 { range.start + 1 } else { range.start }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Everything, laid, para_with, paras, texts};
    use super::super::{NoImages, Resolver, layout};
    use super::*;
    use crate::markdown::text::bytes;

    #[test]
    fn right_to_left_text_mirrors_its_quote_callout_and_table() {
        let t = "פסקה\n\n- [x] משימה\n\n> [!NOTE]\n> הערה\n\n> Quote\n\n| שם | ערך |\n|---|---|\n| 1 | 2 |\n\n```\n# הערה\n```\n";
        let d = parse(t);
        let l = layout(&d, &NoImages, None);
        let paras: Vec<&Para> = l.blocks.iter().flat_map(|b| &b.paras).collect();
        let rtl: Vec<Option<bool>> = paras.iter().map(|p| p.style.rtl).collect();
        let mirrored: Vec<bool> = paras.iter().map(|p| p.style.mirrored).collect();
        assert_eq!(
            rtl,
            [
                Some(true),
                Some(true),
                Some(true),
                Some(true),
                Some(false),
                Some(true),
                Some(true),
                None,
                None,
                Some(false)
            ]
        );
        assert_eq!(
            mirrored,
            [false, false, true, true, false, true, true, true, true, false]
        );
        let cells: Vec<&str> = paras[5..9].iter().map(|p| p.runs[0].text.as_str()).collect();
        assert_eq!(cells, ["ערך", "שם", "2", "1"]);
    }

    #[test]
    fn a_table_in_a_quote_and_a_callout_without_body_take_their_own_direction() {
        let cells = |t: &str| -> (Vec<String>, Vec<bool>) {
            let d = parse(t);
            let l = layout(&d, &NoImages, None);
            l.blocks
                .iter()
                .flat_map(|b| &b.paras)
                .filter(|p| matches!(p.style.kind, ParaKind::TableCell { .. }))
                .map(|p| (p.runs[0].text.clone(), p.style.mirrored))
                .unzip()
        };
        let table = "> | א | ב |\n> |---|---|\n> | 1 | 2 |\n";
        assert_eq!(
            cells(&format!("> שלום\n>\n{table}")),
            (vec!["ב".into(), "א".into(), "2".into(), "1".into()], vec![true; 4])
        );
        assert_eq!(cells(&format!("> [!NOTE]\n> הערה\n>\n{table}")).0, ["ב", "א", "2", "1"]);
        assert_eq!(
            cells("> שלום\n>\n> | a | b |\n> |---|---|\n> | 1 | 2 |\n"),
            (vec!["a".into(), "b".into(), "1".into(), "2".into()], vec![false; 4])
        );
        for t in ["> [!NOTE] הערה\n", "> [!NOTE]- הערה\n> תוכן\n"] {
            let d = parse(t);
            let l = layout(&d, &NoImages, None);
            let title = &l.blocks[0].paras[0];
            assert_eq!(title.style.kind, ParaKind::AlertTitle, "{t}");
            assert_eq!((title.style.rtl, title.style.mirrored), (Some(true), true), "{t}");
        }
    }

    #[test]
    fn paragraphs_lists_and_tables() {
        let t = "# Titel\n\nText **fett**\n\n- a\n- [x] b\n\n| h | i |\n|---|---|\n| 1 |   |\n";
        let d = parse(t);
        let l = layout(&d, &NoImages, None);
        assert_eq!(texts(&l), vec!["Titel", "Text fett", "•\ta", "☑\tb", "h", "i", "1", ""]);
        let item = &l.blocks[2].paras[1];
        assert!(matches!(item.style.marker, Some(Marker::Task { checked: true, .. })));
        let empty_cell = &l.blocks[3].paras[3];
        assert_eq!(t.at(empty_cell.insert_at..empty_cell.insert_at + 1), " ");
    }

    #[test]
    fn extended_syntax_shows_as_formatting() {
        let t = "Begriff\n\n: Erklärung\n\n> [!tip]- Zu\n> versteckt\n\n> [!tip]+ Auf\n> sichtbar\n\n\
                 A ==b== :rocket: ||c|| %%x%% [[Seite|T]] ![[bild.png]]\n";
        let d = parse(t);
        let l = layout(&d, &NoImages, None);
        let o = ATTACHMENT;
        assert_eq!(
            texts(&l),
            vec![
                "Begriff".to_string(),
                "Erklärung".into(),
                format!("{o} Zu ▸"),
                format!("{o} Auf ▾"),
                "sichtbar".into(),
                format!("A b 🚀 c  T {o}"),
            ]
        );
        let paras: Vec<&Para> = l.blocks.iter().flat_map(|b| &b.paras).collect();
        assert!(paras[0].runs.iter().all(|r| r.style.text.bold));
        assert_eq!(paras[1].style.list_depth, 1);
        let fold = paras[2].runs.last().unwrap();
        let RunMap::Atom {
            atom: Atom::Fold { open: false },
            src,
        } = &fold.map
        else {
            panic!("{fold:?}")
        };
        assert_eq!(t.at(src.clone()), "-");
        let runs = &paras[5].runs;
        assert!(runs.iter().any(|r| r.text == "b" && r.style.text.highlight));
        assert!(runs.iter().any(|r| r.text == "c" && r.style.text.spoiler));
        assert!(
            runs.iter()
                .any(|r| r.text == "T" && r.style.link == Some(Link::WikiPage("Seite".into())))
        );
        assert!(
            runs.iter()
                .any(|r| matches!(&r.map, RunMap::Atom { atom: Atom::Image(i), .. } if &*i.url == "bild.png"))
        );
    }

    #[test]
    fn a_table_column_keeps_its_alignment() {
        let l = laid("| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n");
        let of = |text: &str| match para_with(&l, text).style.kind {
            ParaKind::TableCell { align, .. } => align,
            _ => panic!("no cell"),
        };
        assert_eq!((of("1"), of("2"), of("3")), (Align::Left, Align::Center, Align::Right));
    }

    #[test]
    fn a_quote_inside_a_quote_does_not_lend_it_its_direction() {
        let l = laid("> > [!NOTE] שלום\n> > 456\n>\n> 123\n");
        assert!(!para_with(&l, "123").style.mirrored);
    }

    #[test]
    fn quotes_and_callouts_form_their_own_groups() {
        let l = laid("> a\n\n> b\n\n> [!NOTE] t\n> c\n\n> [!TIP] u\n> d\n");
        let a = para_with(&l, "a");
        assert_ne!(a.style.group, 0);
        assert_eq!(a.style.quote_depth, 1);
        let c = para_with(&l, "c");
        let d = para_with(&l, "d");
        assert_ne!(c.style.group, 0);
        assert_eq!(c.style.quote_depth, 1);
        assert_eq!(c.style.alert, Some(AlertKind::Note));
        assert_eq!(d.style.alert, Some(AlertKind::Tip));
    }

    #[test]
    fn a_folding_callout_has_a_sign_to_click() {
        let l = laid("> [!NOTE]- t\n> a\n");
        let fold = paras(&l)[0].runs.iter().find(|r| {
            matches!(
                r.map,
                RunMap::Atom {
                    atom: Atom::Fold { .. },
                    ..
                }
            )
        });
        assert_eq!(fold.unwrap().style.link, Some(Link::Fold));
    }

    fn marker_of<'a>(t: &'a str, p: &Para) -> &'a str {
        match &p.runs[0].map {
            RunMap::Atom {
                atom: Atom::Marker,
                src,
            } => t.at(src.clone()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn list_items_know_their_number_depth_and_marker() {
        let l = laid("3. a\n4. b\n");
        assert_eq!(para_with(&l, "a").style.marker, Some(Marker::Number(3)));
        assert_eq!(para_with(&l, "b").style.marker, Some(Marker::Number(4)));
        let t = "- [x] a\n  - b\n";
        let l = laid(t);
        let a = para_with(&l, "a");
        assert_eq!(marker_of(t, a), "- [x]");
        assert_eq!(a.style.list_depth, 1);
        let b = para_with(&l, "b");
        assert_eq!(b.style.marker, Some(Marker::Bullet(1)));
        assert_eq!(b.style.list_depth, 2);
        assert!(a.style.tight, "no blank line between the items");
        let l = laid("- a\n\n- b\n");
        assert!(!para_with(&l, "a").style.tight);
    }

    #[test]
    fn text_typed_into_an_empty_item_lands_behind_its_marker() {
        let at = |t: &str| {
            let l = laid(t);
            paras(&l).iter().find(|p| p.style.marker.is_some()).unwrap().insert_at
        };
        assert_eq!(at("x\n\n-\n\ny\n"), Byte::new(4));
        assert_eq!(at("-\n"), Byte::new(1));
        assert_eq!(at("-   \n\ny\n"), Byte::new(2));
    }

    #[test]
    fn a_formula_needs_a_math_fence_and_its_picture() {
        let code = |r: &dyn Resolver, t: &str| {
            matches!(paras(&layout(&parse(t), r, None))[0].style.kind, ParaKind::Code { .. })
        };
        assert!(code(&Everything, "```rust\nx\n```\n"));
        assert!(!code(&Everything, "```math\nx\n```\n"));
        assert!(code(&NoImages, "```math\nx\n```\n"));
        assert_ne!(paras(&laid("```\nx\n```\n"))[0].style.group, 0);
    }

    #[test]
    fn front_matter_lists_only_the_keys_at_the_start_of_a_line() {
        let l = laid("---\na: 1\n  b: 2\n- c: 3\n# d: 4\n: e\nf\n---\n");
        assert_eq!(texts(&l)[0], format!("{}: a", crate::i18n::t("Metadaten")));
    }

    #[test]
    fn tables_in_one_block_count_up_and_their_head_is_bold() {
        let l = laid("> | a |\n> |---|\n> | 1 |\n>\n> | b |\n> |---|\n> | 2 |\n");
        let table = |text: &str| match para_with(&l, text).style.kind {
            ParaKind::TableCell { table, .. } => table,
            _ => panic!("no cell"),
        };
        assert_eq!((table("a"), table("b")), (1, 2));
        assert!(para_with(&l, "a").runs[0].style.text.bold);
        assert!(!para_with(&l, "1").runs[0].style.text.bold);
    }

    #[test]
    fn a_footnote_definition_is_its_own_group_of_its_own_kind() {
        let l = laid("a[^x]\n\n[^x]: b\n\n    c\n");
        let b = para_with(&l, "b");
        assert_eq!(b.style.kind, ParaKind::FootnoteDef);
        assert_eq!(para_with(&l, "c").style.kind, ParaKind::FootnoteDef);
        assert_ne!(b.style.group, 0);
    }

    #[test]
    fn text_typed_into_an_empty_heading_lands_behind_its_marks() {
        let at = |t: &str| paras(&laid(t))[0].insert_at;
        assert_eq!(at("##\n"), Byte::new(2), "not behind the end");
        assert_eq!(at("## \n"), Byte::new(3));
        let t = "x\n\n## \n";
        assert_eq!(paras(&laid(t))[1].insert_at, Byte::new(6));
    }

    #[test]
    fn pictures_of_an_embedded_note_are_found_from_the_note_folder() {
        let t = "![[Bilder]]\n";
        let l = layout(&parse(t), &Everything, None);
        let urls: Vec<String> = paras(&l)[0]
            .runs
            .iter()
            .filter_map(|r| match &r.map {
                RunMap::Atom {
                    atom: Atom::Image(i), ..
                } => Some(i.url.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(urls, ["Ordner/a.png", "https://x.de/b.png", "/abs/c.png"]);
    }

    #[test]
    fn only_a_note_alone_in_its_paragraph_is_embedded() {
        let inline = |kind: InlineKind| Inline {
            kind,
            range: bytes(0..0),
            children: Box::default(),
        };
        let note = |target: &str| {
            inline(InlineKind::WikiLink {
                target: target.into(),
                embed: true,
            })
        };
        assert_eq!(only_embed(&[note("a")]), Some("a"));
        assert_eq!(only_embed(&[note("a"), inline(InlineKind::SoftBreak)]), Some("a"));
        assert_eq!(
            only_embed(&[inline(InlineKind::Text(" ".into())), note("a")]),
            Some("a")
        );
        assert_eq!(only_embed(&[note("a"), note("b")]), None);
        assert_eq!(only_embed(&[note("a"), inline(InlineKind::Text("x".into()))]), None);
        assert_eq!(only_embed(&[note("a.png")]), None);
    }

    #[test]
    fn text_typed_into_an_empty_cell_lands_behind_its_space() {
        assert_eq!(cell_insert("|  |", &bytes(1..3)), Byte::new(2));
        assert_eq!(cell_insert("x|a", &bytes(2..3)), Byte::new(2));
    }

    #[test]
    fn text_typed_into_an_empty_task_lands_behind_its_box() {
        let t = "- [ ]\n\nx\n";
        let l = laid(t);
        let item = paras(&l).iter().find(|p| p.style.marker.is_some()).copied().unwrap();
        assert_eq!(item.insert_at, Byte::new(5));
    }
}
