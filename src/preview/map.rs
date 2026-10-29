use std::ops::Range;

use super::layout::{CharMap, Layout, Para, ParaKind, RunMap, RunStyle, para_len_u16};

/// Position of a paragraph in the preview text (UTF-16 offsets).
#[derive(Debug, Clone, Copy)]
pub struct ParaPos {
    pub block: usize,
    pub para: usize,
    pub start: usize,
    pub len: usize,
}

#[derive(Debug, Default)]
pub struct Index {
    pub paras: Vec<ParaPos>,
    pub block_starts: Vec<usize>,
    /// Blocks in source order; comrak moves footnote definitions written earlier in the text behind them.
    pub in_order: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertPoint {
    pub src: usize,
    pub code: bool,
    pub at_para_start: bool,
    pub para_kind: ParaKind,
    /// The paragraph shows embedded HTML.
    pub html: bool,
}

impl Index {
    pub fn new(layout: &Layout) -> Index {
        let mut paras = Vec::new();
        let mut block_starts = Vec::new();
        let mut pos = 0;
        let in_order = layout
            .blocks
            .windows(2)
            .position(|w| w[1].src.start < w[0].src.start)
            .map_or(layout.blocks.len(), |i| i + 1);
        for (b, block) in layout.blocks.iter().enumerate() {
            block_starts.push(pos);
            for (p, para) in block.paras.iter().enumerate() {
                let len = para_len_u16(para);
                paras.push(ParaPos {
                    block: b,
                    para: p,
                    start: pos,
                    len,
                });
                pos += len + 1;
            }
        }
        Index {
            paras,
            block_starts,
            in_order,
        }
    }

    /// Index of the paragraph whose text (including its end) contains `p`.
    pub fn para_at(&self, p: usize) -> Option<usize> {
        if self.paras.is_empty() {
            return None;
        }
        let i = self.paras.partition_point(|pp| pp.start <= p);
        Some(i.saturating_sub(1))
    }

    pub fn para<'a>(&self, layout: &'a Layout, i: usize) -> &'a Para {
        let pp = self.paras[i];
        &layout.blocks[pp.block].paras[pp.para]
    }
}

/// UTF-16 length of the list or footnote marker at the start of a paragraph.
pub fn marker_len(para: &Para) -> usize {
    match para.runs.first() {
        Some(r)
            if matches!(
                r.map,
                RunMap::Atom {
                    atom: super::layout::Atom::Marker,
                    ..
                }
            ) =>
        {
            run_len(&r.text)
        }
        _ => 0,
    }
}

fn run_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Char index inside `text` of the given UTF-16 offset.
fn char_index(text: &str, u16_offset: usize) -> usize {
    let mut u = 0;
    for (k, c) in text.chars().enumerate() {
        if u >= u16_offset {
            return k;
        }
        u += c.len_utf16();
    }
    text.chars().count()
}

fn text_source(chars: &CharMap, text: &str, end: usize, k: usize) -> usize {
    chars.get(text, k).unwrap_or(end)
}

fn first_text_start(para: &Para, from_run: usize) -> Option<usize> {
    para.runs[from_run..].iter().find_map(|r| match &r.map {
        RunMap::Text { chars, end } => Some(text_source(chars, &r.text, *end, 0)),
        RunMap::Atom { .. } => None,
    })
}

/// Source offset of the start of a paragraph's content.
pub fn content_start(para: &Para) -> usize {
    match para.runs.first() {
        Some(r) => match &r.map {
            RunMap::Text { chars, end } => text_source(chars, &r.text, *end, 0),
            RunMap::Atom { src, .. } => src.start,
        },
        None => para.insert_at,
    }
}

/// Source offset of the end of a paragraph's content.
pub fn content_end(para: &Para) -> usize {
    match para.runs.last().map(|r| &r.map) {
        Some(RunMap::Text { end, .. }) => *end,
        Some(RunMap::Atom { src, .. }) => src.end,
        None => para.insert_at,
    }
}

/// Where text typed at preview offset `p` goes. The character left of the caret decides, like typing attributes.
pub fn insert_point(layout: &Layout, index: &Index, p: usize) -> Option<InsertPoint> {
    let pi = index.para_at(p)?;
    let pp = index.paras[pi];
    let para = index.para(layout, pi);
    let marker_len = marker_len(para);
    let off = p.saturating_sub(pp.start).min(pp.len).max(marker_len);
    let make = |src: usize, style: &RunStyle| InsertPoint {
        src,
        code: style.code || matches!(para.style.kind, ParaKind::Code { .. }),
        at_para_start: off == marker_len,
        para_kind: para.style.kind.clone(),
        html: para.style.html,
    };
    if off == marker_len {
        let first = usize::from(marker_len > 0);
        return Some(match para.runs.get(first) {
            Some(r) => match &r.map {
                RunMap::Text { chars, end } => make(text_source(chars, &r.text, *end, 0), &r.style),
                RunMap::Atom { src, .. } => make(src.start, &RunStyle::default()),
            },
            None => make(para.insert_at, &RunStyle::default()),
        });
    }
    let mut u = 0;
    for (ri, run) in para.runs.iter().enumerate() {
        let len = run_len(&run.text);
        if off > u && off <= u + len {
            return Some(match &run.map {
                RunMap::Text { chars, end } => {
                    let k = char_index(&run.text, off - u);
                    make(text_source(chars, &run.text, *end, k), &run.style)
                }
                RunMap::Atom { src, .. } => {
                    if off < u + len {
                        make(src.start, &RunStyle::default())
                    } else {
                        let next = first_text_start(para, ri + 1).filter(|_| {
                            matches!(
                                run.map,
                                RunMap::Atom {
                                    atom: super::layout::Atom::Text,
                                    ..
                                }
                            ) && run.text == " "
                        });
                        make(
                            next.unwrap_or(src.end),
                            &RunStyle {
                                link: None,
                                ..run.style.clone()
                            },
                        )
                    }
                }
            });
        }
        u += len;
    }
    Some(make(para.insert_at, &RunStyle::default()))
}

/// Preview offset for a source offset; positions inside markup snap to the nearest rendered text.
pub fn preview_offset(layout: &Layout, index: &Index, src: usize) -> usize {
    let mut best: Option<(usize, usize)> = None;
    let mut consider = |distance: usize, pos: usize| {
        if best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, pos));
        }
    };
    // A block in source order that holds the offset wins; an inline footnote's definition lies inside one.
    let in_order = &layout.blocks[..index.in_order.min(layout.blocks.len())];
    let before = in_order.partition_point(|b| b.src.start <= src).saturating_sub(1);
    let holding = in_order
        .get(before)
        .filter(|b| b.src.start <= src && src <= b.src.end)
        .map(|_| before);
    let moved = || {
        (index.in_order..layout.blocks.len()).find(|&k| {
            let r = &layout.blocks[k].src;
            r.start <= src && src <= r.end
        })
    };
    let bi = holding.or_else(moved).unwrap_or(before);
    let first_para = index.paras.partition_point(|pp| pp.block < bi.saturating_sub(1));
    for (i, pp) in index.paras.iter().enumerate().skip(first_para) {
        if pp.block > bi + 1 {
            break;
        }
        let para = index.para(layout, i);
        let mut u = 0;
        if para.runs.is_empty() {
            consider(para.insert_at.abs_diff(src), pp.start);
        }
        for run in &para.runs {
            let len = run_len(&run.text);
            match &run.map {
                RunMap::Text { chars, end } => {
                    let first = chars.get(&run.text, 0);
                    if let (Some(first), true) = (first, src <= *end)
                        && src >= first
                    {
                        let k = chars.count_before(&run.text, src);
                        let u16_in: usize = run.text.chars().take(k).map(char::len_utf16).sum();
                        return pp.start + u + u16_in;
                    }
                    if let Some(first) = first {
                        consider(first.abs_diff(src), pp.start + u);
                        consider(end.abs_diff(src), pp.start + u + len);
                    }
                }
                RunMap::Atom { src: r, atom } => {
                    if r.start <= src && src < r.end {
                        let after = matches!(atom, super::layout::Atom::Marker);
                        return pp.start + u + if after { len } else { 0 };
                    }
                    consider(r.start.abs_diff(src), pp.start + u);
                    consider(r.end.abs_diff(src), pp.start + u + len);
                }
            }
            u += len;
        }
    }
    best.map_or(0, |(_, p)| p)
}

/// Kinds whose paragraphs can be joined by deleting the break between them.
pub fn mergeable(kind: &ParaKind) -> bool {
    matches!(kind, ParaKind::Body | ParaKind::Heading(_) | ParaKind::FootnoteDef)
}

/// Source ranges to delete for a preview range. Markup between rendered characters stays,
/// unless the range joins two paragraphs.
pub fn deletion_ranges(layout: &Layout, index: &Index, range: Range<usize>) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let (Some(first), Some(last)) = (index.para_at(range.start), index.para_at(range.end)) else {
        return out;
    };
    let joins = first != last
        && mergeable(&index.para(layout, first).style.kind)
        && mergeable(&index.para(layout, last).style.kind);
    if joins {
        out.push(content_end(index.para(layout, first))..content_start(index.para(layout, last)));
    }
    for pi in first..=last {
        let pp = index.paras[pi];
        let para = index.para(layout, pi);
        let a = range.start.max(pp.start) - pp.start;
        let b = range.end.min(pp.start + pp.len).saturating_sub(pp.start);
        let mut u = 0;
        for (ri, run) in para.runs.iter().enumerate() {
            let len = run_len(&run.text);
            let (ra, rb) = (a.max(u), b.min(u + len));
            if ra < rb {
                match &run.map {
                    RunMap::Text { chars, end } => {
                        let k1 = char_index(&run.text, ra - u);
                        let k2 = char_index(&run.text, rb - u);
                        out.push(text_source(chars, &run.text, *end, k1)..text_source(chars, &run.text, *end, k2));
                    }
                    RunMap::Atom { src, .. } => {
                        let next = first_text_start(para, ri + 1).filter(|&n| n >= src.end);
                        let end = if run.text == " " {
                            next.unwrap_or(src.end)
                        } else {
                            src.end
                        };
                        out.push(src.start..end);
                    }
                }
            }
            u += len;
        }
        if !joins && pi != first && pi != last && para.runs.iter().all(|r| matches!(r.map, RunMap::Atom { .. })) {
            out.push(para.src.clone());
        }
    }
    normalize(out)
}

pub fn normalize(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.retain(|r| r.start < r.end);
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::new();
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

#[cfg(test)]
mod order_tests {
    use super::*;
    use crate::markdown::doc::parse;
    use crate::preview::layout::{NoImages, layout};

    /// Text appended after footnotes lies in the preview before the definitions comrak moved behind it.
    #[test]
    fn source_positions_find_their_block_behind_moved_footnotes() {
        let t = "Text^[innen] und[^1].\n\n[^1]: Anmerkung.\n\nNeu am Ende\n";
        let d = parse(t);
        let l = layout(t, &d, &NoImages, None);
        let i = Index::new(&l);
        assert!(i.in_order < l.blocks.len());
        let mut preview = String::new();
        for b in &l.blocks {
            for p in &b.paras {
                preview.extend(p.runs.iter().map(|r| r.text.as_str()));
                preview.push('\n');
            }
        }
        // The end of a definition comrak moved belongs to it, not to the text before.
        let def_end = t.find("Anmerkung.").unwrap() + "Anmerkung.".len();
        let shown_end = preview.find("Anmerkung.").unwrap() + "Anmerkung.".len();
        assert_eq!(
            preview_offset(&l, &i, def_end),
            preview[..shown_end].encode_utf16().count()
        );
        let end = t.find("Neu am Ende").unwrap() + "Neu am Ende".len();
        let p = preview_offset(&l, &i, end);
        let at = preview.find("Neu am Ende").unwrap() + "Neu am Ende".len();
        assert_eq!(p, preview[..at].encode_utf16().count());

        assert!(preview.contains("1.\tinnen"), "{preview}");
        // The empty paragraph for a caret at the end of the text stays with the text, before the footnotes.
        let caret = t.len();
        let with_caret = layout(&format!("{t}\n"), &parse(&format!("{t}\n")), &NoImages, Some(caret + 1));
        let last_text = with_caret
            .blocks
            .iter()
            .position(|b| b.paras.iter().any(|p| p.runs.is_empty()))
            .unwrap();
        let footnotes = with_caret
            .blocks
            .iter()
            .position(|b| {
                matches!(
                    b.paras[0].style.marker,
                    Some(crate::preview::layout::Marker::Footnote(..))
                )
            })
            .unwrap();
        assert!(last_text < footnotes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::doc::parse;
    use crate::preview::layout::{NoImages, layout};

    fn setup(t: &str) -> (Layout, Index) {
        let l = layout(t, &parse(t), &NoImages, None);
        let i = Index::new(&l);
        (l, i)
    }

    #[test]
    fn typing_at_end_of_bold_stays_bold() {
        let t = "a **fett** b\n";
        let (l, i) = setup(t);
        let ip = insert_point(&l, &i, 6).unwrap();
        assert_eq!(ip.src, t.find("** b").unwrap());
        let ip0 = insert_point(&l, &i, 0).unwrap();
        assert_eq!(ip0.src, 0);
    }

    #[test]
    fn preview_offsets_roundtrip() {
        let t = "# Ü\n\nx **fett** y\n";
        let (l, i) = setup(t);
        assert_eq!(preview_offset(&l, &i, t.find("fett").unwrap()), 2 + 2);
        assert_eq!(preview_offset(&l, &i, t.find("Ü").unwrap()), 0);
        assert_eq!(preview_offset(&l, &i, t.find('y').unwrap()), 2 + 7);
    }

    #[test]
    fn deleting_across_formatting_keeps_delimiters() {
        let t = "ab **cd** ef\n";
        let (l, i) = setup(t);
        let r = deletion_ranges(&l, &i, 1..4);
        let mut s = t.to_string();
        for d in r.iter().rev() {
            s.replace_range(d.clone(), "");
        }
        assert_eq!(s, "a**d** ef\n");
    }

    #[test]
    fn deleting_paragraph_break_joins_blocks() {
        let t = "eins\n\n## zwei\n";
        let (l, i) = setup(t);
        let r = deletion_ranges(&l, &i, 4..5);
        let mut s = t.to_string();
        for d in r.iter().rev() {
            s.replace_range(d.clone(), "");
        }
        assert_eq!(s, "einszwei\n");
    }

    #[test]
    fn soft_break_maps_to_next_line_text() {
        let t = "> zitat\n> zwei\n";
        let (l, i) = setup(t);
        let ip = insert_point(&l, &i, 6).unwrap();
        assert_eq!(&t[ip.src..ip.src + 4], "zwei");
    }
}
