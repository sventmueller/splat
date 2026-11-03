use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use super::{
    Builder, CharMap, Ctx, Embeds, Layout, Marker, Para, ParaKind, ParaStyle, Resolver, Run, RunMap, RunStyle, TopBlock,
};
use crate::markdown::doc::{Align, Block, BlockKind, Doc};
use crate::markdown::text::{Byte, TextAt, line_start, shifted, units};

struct Part {
    paras: Vec<Para>,
    src: Range<Byte>,
    /// Key and length of a block taken over unchanged.
    kept: Option<(u64, usize)>,
    context: u64,
    embeds: Embeds,
}

pub(super) fn build(
    doc: &Doc,
    resolver: &dyn Resolver,
    caret: Option<Byte>,
    old: Vec<Arc<TopBlock>>,
    reuse: &dyn Fn(usize) -> Option<isize>,
) -> Layout {
    let text = doc.text();
    let mut b = Builder {
        text,
        resolver,
        group: 0,
        table: 0,
        paras: Vec::new(),
        open: Default::default(),
        embeds: 0,
        embeds_refused: false,
    };
    // Footnote definitions comrak moved behind the text break the source order of the blocks.
    let mut old_starts: Vec<(Byte, usize)> = old.iter().enumerate().map(|(k, o)| (o.src.start, k)).collect();
    old_starts.sort_unstable();
    let mut old: Vec<Option<Arc<TopBlock>>> = old.into_iter().map(Some).collect();
    let mut parts: Vec<Part> = Vec::with_capacity(doc.blocks.len());
    let wrappers = html_wrappers(&doc.blocks);
    for (i, block) in doc.blocks.iter().enumerate() {
        let wrapper = wrappers.get(i).copied().unwrap_or_default();
        if wrapper.hidden {
            continue;
        }
        let context = wrapper.key();
        if let Some(shift) = reuse(i) {
            let at = shifted(block.range.start, -shift);
            // An inline note's definition can start where the block with its reference starts, which may be hidden now.
            let definition = matches!(block.kind, BlockKind::FootnoteDefinition { .. });
            let found = old_starts[old_starts.partition_point(|s| s.0 < at)..]
                .iter()
                .take_while(|s| s.0 == at)
                .map(|s| s.1)
                .find(|&k| old[k].as_ref().is_some_and(|o| shows_definition(o) == definition))
                .and_then(|k| old[k].take())
                .filter(|o| {
                    !o.src.is_empty()
                        && units(&o.src) == units(&block.range)
                        && o.context == context
                        && o.embeds.hold_behind(b.embeds)
                });
            if let Some(o) = found {
                let o = Arc::unwrap_or_clone(o);
                b.embeds += o.embeds.shown;
                let mut paras = o.paras;
                if shift != 0 {
                    for p in &mut paras {
                        shift_para(p, shift);
                    }
                }
                parts.push(Part {
                    paras,
                    src: block.range.clone(),
                    kept: Some((o.key, o.len_u16)),
                    context,
                    embeds: o.embeds,
                });
                continue;
            }
        }
        b.group = 0;
        b.table = 0;
        let ctx = Ctx {
            align: wrapper.align,
            ..Ctx::default()
        };
        let embeds_before = b.embeds;
        b.embeds_refused = false;
        b.block(block, &ctx, None);
        let paras = std::mem::take(&mut b.paras);
        if !paras.is_empty() {
            parts.push(Part {
                paras,
                src: block.range.clone(),
                kept: None,
                context,
                embeds: Embeds {
                    shown: b.embeds - embeds_before,
                    refused: b.embeds_refused,
                },
            });
        }
    }
    drop(old);
    if let Some(c) = caret {
        show_spaces_at_the_caret(text, &mut parts, c);
    }
    let mut parts = with_empty_lines(text, doc, parts, caret);
    mark_paragraphs_behind_tables(&mut parts);
    Layout {
        blocks: parts.into_iter().map(|part| Arc::new(part.finished())).collect(),
        doc_version: doc.version(),
    }
}

fn shows_definition(block: &TopBlock) -> bool {
    block
        .paras
        .first()
        .is_some_and(|p| matches!(p.style.marker, Some(Marker::Footnote(..))))
}

/// Only a paragraph ending on the caret's line can show trailing spaces.
fn show_spaces_at_the_caret(text: &str, parts: &mut [Part], c: Byte) {
    let c_in_text = c.min(text.end());
    let line = line_start(text, c_in_text)..text.from(c_in_text).find('\n').map_or(text.end(), |i| c + i);
    for part in parts
        .iter_mut()
        .filter(|p| p.src.start <= line.end && p.src.end >= line.start)
    {
        for para in &mut part.paras {
            let before = para.runs.len();
            show_trailing_spaces(text, para, c);
            if para.runs.len() != before {
                part.kept = None;
            }
        }
    }
}

/// `parts` with the empty paragraphs the blank lines of the source show as, and the one for a caret on a blank line.
fn with_empty_lines(text: &str, doc: &Doc, parts: Vec<Part>, caret: Option<Byte>) -> Vec<Part> {
    let ranges: Vec<Range<Byte>> = parts.iter().map(|p| p.src.clone()).collect();
    let spots = empty_line_spots(text, &ranges);
    let virtual_at = caret.filter(|&c| {
        // Footnote definitions comrak moved behind the text break the source order of the blocks.
        let inside = doc.blocks.iter().any(|b| b.range.start <= c && c <= b.range.end);
        let line_begin = line_start(text, c);
        let line_end = text.from(c).find('\n').map_or(text.end(), |i| c + i);
        !inside && text.at(line_begin..line_end).trim().is_empty() && spots.binary_search(&line_begin).is_err()
    });
    let virtual_at = virtual_at.or(if parts.is_empty() && spots.is_empty() {
        Some(text.end())
    } else {
        None
    });
    let mut extra: Vec<Byte> = spots.into_iter().chain(virtual_at).collect();
    extra.sort_unstable();
    let mut merged = Vec::with_capacity(parts.len() + extra.len());
    let mut extra = extra.into_iter().peekable();
    let mut in_order_until = Byte::default();
    for part in parts {
        // Empty lines after the text come before the footnote definitions comrak moved behind it.
        let moved = part.src.start < in_order_until;
        while let Some(&c) = extra.peek().filter(|&&c| moved || c <= part.src.start) {
            merged.push(Part::empty(c));
            extra.next();
        }
        if !moved {
            in_order_until = part.src.start;
        }
        merged.push(part);
    }
    merged.extend(extra.map(Part::empty));
    merged
}

/// A table cannot carry spacing below itself, so the first paragraph behind it does.
fn mark_paragraphs_behind_tables(parts: &mut [Part]) {
    for i in 1..parts.len() {
        let after_table = parts[i - 1]
            .paras
            .last()
            .is_some_and(|p| matches!(p.style.kind, ParaKind::TableCell { .. }));
        let part = &mut parts[i];
        if let Some(first) = part.paras.first_mut()
            && first.style.space_before != after_table
        {
            first.style.space_before = after_table;
            part.kept = None;
        }
    }
}

impl Part {
    fn empty(at: Byte) -> Part {
        Part {
            paras: vec![empty_para(at)],
            src: at..at,
            kept: None,
            context: 0,
            embeds: Embeds::default(),
        }
    }

    fn finished(self) -> TopBlock {
        let Part {
            mut paras,
            src,
            kept,
            context,
            embeds,
        } = self;
        if let Some((key, len_u16)) = kept {
            return TopBlock {
                paras,
                src,
                key,
                len_u16,
                context,
                embeds,
            };
        }
        // Spare capacity from building the runs piece by piece is close to half the layout's memory.
        for p in &mut paras {
            for r in &mut p.runs {
                r.text.shrink_to_fit();
            }
            p.runs.shrink_to_fit();
        }
        paras.shrink_to_fit();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for p in &paras {
            render_hash(p, &mut hasher);
        }
        let len_u16 = paras.iter().map(para_len_u16).sum::<usize>() + paras.len();
        TopBlock {
            paras,
            src,
            key: hasher.finish(),
            len_u16,
            context,
            embeds,
        }
    }
}

/// What HTML wrapping a top-level block asks of it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
struct Wrapper {
    align: Align,
    /// Inside a `<details>` element that is not open.
    hidden: bool,
}

impl Wrapper {
    fn key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut h);
        h.finish()
    }
}

/// The HTML wrapper of every top-level block: an HTML block may open `<details>` or `<div align="center">` and a
/// later one close it, with Markdown between them.
fn html_wrappers(blocks: &[Block]) -> Vec<Wrapper> {
    let mut open = crate::markdown::markup::OpenElements::<Wrapper>::default();
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        out.push(open.innermost().copied().unwrap_or_default());
        let BlockKind::HtmlBlock { literal } = &block.kind else {
            continue;
        };
        for name in crate::markdown::markup::stray_closes(literal) {
            open.close(&name);
        }
        let nodes = crate::markdown::markup::parse(literal);
        for e in crate::markdown::markup::open_at_end(&nodes) {
            let around = open.innermost().copied().unwrap_or_default();
            let wrapper = Wrapper {
                align: e
                    .align()
                    .or((e.name == "center").then_some(Align::Center))
                    .unwrap_or(around.align),
                hidden: around.hidden || (e.name == "details" && e.attr("open").is_none()),
            };
            open.open(e.name.clone(), wrapper);
        }
    }
    out
}

/// Moves every source offset of a laid-out paragraph by `delta`.
fn shift_para(p: &mut Para, delta: isize) {
    let at = |o: Byte| shifted(o, delta);
    p.src = at(p.src.start)..at(p.src.end);
    p.insert_at = at(p.insert_at);
    if let Some(Marker::Task {
        symbol: Some(symbol), ..
    }) = &mut p.style.marker
    {
        *symbol = at(*symbol);
    }
    for r in &mut p.runs {
        match &mut r.map {
            RunMap::Text { chars, end } => {
                *end = at(*end);
                match chars {
                    CharMap::Verbatim(start) => *start = at(*start),
                    CharMap::Offsets(base, _) => *base = at(*base),
                }
            }
            RunMap::Atom { src, .. } => *src = at(src.start)..at(src.end),
        }
    }
}

fn empty_para(at: Byte) -> Para {
    Para {
        style: ParaStyle {
            kind: ParaKind::Body,
            quote_depth: 0,
            list_depth: 0,
            marker: None,
            group: 0,
            tight: false,
            space_before: false,
            alert: None,
            align: Align::None,
            html: false,
            rtl: None,
            mirrored: false,
        },
        runs: vec![],
        src: at..at,
        insert_at: at,
    }
}

/// Line starts that the preview shows as empty paragraphs. Markdown merges blank lines, so every second extra blank
/// line between blocks stands for one empty line; each keeps a blank line above and below, which makes text typed
/// into it a paragraph of its own.
pub fn empty_line_spots(text: &str, blocks: &[Range<Byte>]) -> Vec<Byte> {
    let mut spots = Vec::new();
    let mut run: Vec<Byte> = Vec::new();
    let mut flush = |run: &mut Vec<Byte>, at_start: bool, at_end: bool| {
        let first = if at_start { 0 } else { 1 };
        let limit = if at_end { run.len() } else { run.len().saturating_sub(1) };
        spots.extend(
            run.iter()
                .enumerate()
                .skip(first)
                .step_by(2)
                .filter(|(i, _)| *i < limit)
                .map(|(_, &s)| s),
        );
        run.clear();
    };
    let mut region = |from: Byte, to: Byte, at_start: bool, at_end: bool| {
        let mut start = from;
        let mut lead = at_start;
        let pieces: Vec<&str> = text.at(from..to).split('\n').collect();
        let count = if at_end { pieces.len() } else { pieces.len() - 1 };
        for piece in &pieces[..count] {
            if piece.trim().is_empty() {
                run.push(start);
            } else {
                flush(&mut run, lead, false);
                lead = false;
            }
            start += piece.len() + 1;
        }
        flush(&mut run, lead, at_end);
    };
    let next_line = |end: Byte, limit: Byte| {
        if text.upto(end).ends_with('\n') {
            Some(end)
        } else {
            text.at(end..limit).find('\n').map(|i| end + i + 1)
        }
    };
    let mut prev_end: Option<Byte> = None;
    for b in blocks {
        match prev_end {
            None => region(Byte::default(), b.start, true, false),
            Some(end) => {
                if let Some(from) = next_line(end, b.start.max(end)) {
                    region(from, b.start.max(from), false, false);
                }
            }
        }
        prev_end = Some(b.end.max(prev_end.unwrap_or_default()));
    }
    match prev_end {
        Some(end) => {
            if let Some(from) = next_line(end, text.end()) {
                region(from, text.end(), false, true);
            }
        }
        None => region(Byte::default(), text.end(), true, true),
    }
    spots
}

/// Markdown drops spaces at the end of a paragraph, so a space just typed there would vanish from the preview and the
/// next character would land in front of it. While the caret sits in such spaces they are shown as text.
fn show_trailing_spaces(text: &str, para: &mut Para, caret: Byte) {
    if !matches!(para.style.kind, ParaKind::Body | ParaKind::Heading(_)) {
        return;
    }
    let content_end = crate::preview::map::content_end(para);
    if content_end > text.end() {
        return;
    }
    let end = text
        .from(content_end)
        .find('\n')
        .map_or(text.end(), |i| content_end + i);
    let start = end
        - text
            .at(content_end..end)
            .bytes()
            .rev()
            .take_while(|b| matches!(b, b' ' | b'\t'))
            .count();
    if start == end || caret < start || caret > end {
        return;
    }
    para.runs.push(Run {
        text: text.at(start..end).to_string(),
        style: RunStyle::default(),
        map: RunMap::Text {
            chars: CharMap::Verbatim(start),
            end,
        },
    });
}

/// Hashes only what affects the rendered output, so blocks after an edit keep their key although
/// their source offsets moved.
fn render_hash(p: &Para, h: &mut impl Hasher) {
    let s = &p.style;
    (
        &s.kind,
        s.quote_depth,
        s.list_depth,
        s.group,
        s.tight,
        s.space_before,
        s.alert,
        s.align,
        s.html,
    )
        .hash(h);
    match &s.marker {
        Some(Marker::Task { checked, .. }) => (1u8, checked).hash(h),
        other => (0u8, other).hash(h),
    }
    for r in &p.runs {
        (&r.text, &r.style).hash(h);
        if let RunMap::Atom { atom, .. } = &r.map {
            atom.hash(h);
        }
    }
}

pub fn para_len_u16(p: &Para) -> usize {
    p.runs.iter().map(|r| crate::markdown::text::utf16_len(&r.text)).sum()
}

#[cfg(test)]
mod tests {
    use super::super::tests::{laid, para_with, texts};
    use super::super::{NoImages, layout, relayout, reusable};
    use super::*;
    use crate::markdown::reader::parse;

    #[test]
    fn spaces_at_the_caret_stay_visible_at_a_paragraph_end() {
        let t = "Wort **fett** \n\nNächster\n";
        let d = parse(t);
        assert_eq!(texts(&layout(&d, &NoImages, None))[0], "Wort fett");
        assert_eq!(texts(&layout(&d, &NoImages, Some(Byte::new(14))))[0], "Wort fett ");
        assert_eq!(texts(&layout(&d, &NoImages, Some(Byte::new(3))))[0], "Wort fett");
    }

    #[test]
    fn every_second_extra_blank_line_is_an_empty_paragraph() {
        let spots = |t: &str| {
            let d = parse(t);
            let ranges: Vec<Range<Byte>> = d.blocks.iter().map(|b| b.range.clone()).collect();
            empty_line_spots(t, &ranges)
                .into_iter()
                .map(Byte::get)
                .collect::<Vec<_>>()
        };
        assert_eq!(spots("A\n\nB\n"), Vec::<usize>::new());
        assert_eq!(spots("A\n\n\nB\n"), Vec::<usize>::new());
        assert_eq!(spots("A\n\n\n\nB\n"), vec![3]);
        assert_eq!(spots("A\n\n\n\n\n\nB\n"), vec![3, 5]);
        assert_eq!(spots("A\n\n\n"), vec![3]);
        assert_eq!(spots("\n\nA\n"), vec![0]);
        let t = "A\n\n\n\nB\n";
        let l = layout(&parse(t), &NoImages, None);
        assert_eq!(texts(&l), vec!["A", "", "B"]);
    }

    fn eight_blocks() -> (&'static str, String) {
        let old = "aaa\n\nbbb\n\nccc\n\nddd\n\neee\n\nfff\n\nggg\n\nhhh\n";
        (old, old.replace("bbb", "bbbxxxxx"))
    }

    #[test]
    fn a_block_behind_the_edit_is_taken_over_from_its_own_place() {
        let (old, new) = eight_blocks();
        let mut first = laid(old);
        let last = first.blocks.len() - 2;
        Arc::make_mut(&mut first.blocks[last]).paras[0].runs[0].text = "TAKEN".to_string();
        let mut doc = parse(old);
        let splice = crate::markdown::incremental::reparse(&mut doc, &new).unwrap();
        let again = relayout(&doc, &NoImages, None, first, &reusable(&splice, &doc, None));
        let shown = texts(&again);
        assert_eq!(shown[1], "bbbxxxxx");
        assert_eq!(shown[3..], ["ddd", "eee", "fff", "TAKEN", "hhh"], "{shown:?}");
    }

    #[test]
    fn a_definition_is_taken_over_from_its_own_place() {
        let old = "[^1]: eins\n\naaa[^1]\n\nbbb\n\nccc\n\nddd\n\neee[^2]\n\n^[drei]\n\nfff\n\n[^2]: zwei\n";
        let new = old.replace("ccc", "cccxxxxx");
        let mut first = laid(old);
        let definitions = |l: &Layout| l.blocks.iter().filter(|b| shows_definition(b)).count();
        assert_eq!(definitions(&first), 3);
        for block in first.blocks.iter_mut().filter(|b| shows_definition(b)) {
            Arc::make_mut(block).paras[0].runs[1].text = "TAKEN".to_string();
        }
        let mut doc = parse(old);
        let splice = crate::markdown::incremental::reparse(&mut doc, &new).unwrap();
        let again = relayout(&doc, &NoImages, None, first, &reusable(&splice, &doc, None));
        let shown: Vec<_> = again.blocks.iter().filter(|b| shows_definition(b)).collect();
        assert_eq!(shown.len(), 3);
        assert!(shown.iter().all(|b| b.paras[0].runs[1].text == "TAKEN"), "{shown:?}");
        para_with(&again, "[3]");
    }

    /// Finds a note for every name.
    struct EveryNote;

    impl Resolver for EveryNote {
        fn math_ready(&self, _: &str, _: bool) -> bool {
            false
        }
        fn diagram_ready(&self, _: &str, _: crate::markdown::doc::DiagramKind) -> bool {
            false
        }
        fn embedded_note(&self, _: &str) -> Option<(String, String)> {
            Some(("Inhalt\n".to_string(), "/n".to_string()))
        }
    }

    #[test]
    fn notes_embed_alike_whether_blocks_are_taken_over_or_not() {
        let notes = |from: usize, to: usize| (from..to).map(|i| format!("![[n{i}]]\n\n")).collect::<String>();
        let old = format!("{}Ende\n", notes(0, 70));
        for new in [
            format!("{}Ende\n", notes(1, 70)),
            format!("![[neu]]\n\n{old}"),
            old.replace("![[n69]]", "![[n69x]]"),
            old.replace("![[n3]]", "![[n3x]]"),
            old.replace("Ende", "Ende!"),
            format!("{}Ende\n", notes(0, 64)) + "\n![[n64]]\n",
        ] {
            let first = layout(&parse(&old), &EveryNote, None);
            let mut doc = parse(&old);
            let splice = crate::markdown::incremental::reparse(&mut doc, &new).expect("a part to parse");
            let again = relayout(&doc, &EveryNote, None, first, &reusable(&splice, &doc, None));
            let whole = layout(&parse(&new), &EveryNote, None);
            assert_eq!(texts(&again), texts(&whole), "{}", &new[..30]);
            let embeds = |l: &Layout| l.blocks.iter().map(|b| b.embeds.shown).sum::<usize>();
            assert_eq!(embeds(&again), embeds(&whole));
        }
    }

    #[test]
    fn a_taken_over_block_that_shows_its_spaces_counts_as_changed() {
        let old = "aaa  \n\nbbb\n\nccc\n\nddd\n\neee\n\nfff\n";
        let new = old.replace("eee", "eeee");
        let first = laid(old);
        let key = first.blocks[0].key;
        let mut doc = parse(old);
        let splice = crate::markdown::incremental::reparse(&mut doc, &new).unwrap();
        let again = relayout(
            &doc,
            &NoImages,
            Some(Byte::new(4)),
            first,
            &reusable(&splice, &doc, None),
        );
        assert_eq!(texts(&again)[0], "aaa  ");
        assert_ne!(again.blocks[0].key, key);
    }

    #[test]
    fn spaces_show_at_the_caret_from_their_first_to_their_last_position() {
        let t = "a  \n\nb  \n";
        let shown = |caret: usize| texts(&layout(&parse(t), &NoImages, Some(Byte::new(caret))));
        assert_eq!(shown(6), ["a", "b  "], "right behind the letter");
        assert_eq!(shown(7), ["a", "b  "], "between the spaces");
        assert_eq!(shown(8), ["a", "b  "], "behind them");
        assert_eq!(shown(1), ["a  ", "b"]);
        assert_eq!(shown(0), ["a", "b"], "in front of the letter");
    }

    #[test]
    fn a_blank_line_with_the_caret_gets_a_paragraph_of_its_own() {
        let shown = |t: &str, caret: usize| texts(&layout(&parse(t), &NoImages, Some(Byte::new(caret))));
        let t = "a\n\nb\n";
        assert_eq!(shown(t, 2), ["a", "", "b"]);
        assert_eq!(shown(t, 1), ["a", "b"], "at the end of a block");
        assert_eq!(shown(t, 3), ["a", "b"], "at its start");
        assert_eq!(shown(t, 4), ["a", "b"], "at the end of the last");
        assert_eq!(
            shown("a\n\n[r]: u\n\nb\n", 5),
            ["a", "b"],
            "a line holding a link definition is not blank"
        );
        assert_eq!(
            shown("a\n\n\n\nb\n", 3),
            ["a", "", "b"],
            "one that shows anyway gets no second"
        );
    }

    #[test]
    fn only_the_paragraph_right_behind_a_table_has_space_before_it() {
        let l = laid("x\n\n| a |\n|---|\n| 1 |\n\ny\n\nz\n");
        assert!(!para_with(&l, "x").style.space_before);
        assert!(para_with(&l, "y").style.space_before);
        assert!(!para_with(&l, "z").style.space_before);
    }

    #[test]
    fn a_wrapper_in_html_aligns_the_markdown_inside_it() {
        let inside = |open: &str, close: &str| {
            let t = format!("{open}\n\nText\n\n{close}\n");
            let l = laid(&t);
            para_with(&l, "Text").style.align
        };
        assert_eq!(inside("<div align=\"center\">", "</div>"), Align::Center);
        assert_eq!(inside("<div align=\"right\">", "</div>"), Align::Right);
        assert_eq!(inside("<div align=\"left\">", "</div>"), Align::Left);
        assert_eq!(inside("<center>", "</center>"), Align::Center);
        assert_eq!(inside("<div>", "</div>"), Align::None);
    }

    #[test]
    fn blank_lines_are_counted_from_the_end_of_the_line_a_block_ends_on() {
        let spots = |t: &str| {
            let d = parse(t);
            let ranges: Vec<Range<Byte>> = d.blocks.iter().map(|b| b.range.clone()).collect();
            empty_line_spots(t, &ranges)
        };
        assert_eq!(
            spots("a  \n\nb\n"),
            Vec::<Byte>::new(),
            "spaces behind a paragraph are no blank line"
        );
    }

    #[test]
    fn spaces_behind_a_paragraph_do_not_count_as_a_blank_line() {
        let d = parse("a \n\n\nb\n");
        let ranges: Vec<Range<Byte>> = d.blocks.iter().map(|b| b.range.clone()).collect();
        assert_eq!(empty_line_spots("a \n\n\nb\n", &ranges), Vec::<Byte>::new());
    }
}
