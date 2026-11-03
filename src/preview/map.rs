use std::ops::Range;

use crate::markdown::text::Byte;
pub use crate::markdown::text::Shown;

use super::layout::{CharMap, Layout, Para, ParaKind, Run, RunMap, RunStyle, para_len_u16};

/// Position of a paragraph in the preview text.
#[derive(Debug, Clone, Copy)]
pub struct ParaPos {
    pub block: usize,
    pub para: usize,
    pub start: Shown,
    /// In UTF-16 units.
    pub len: usize,
}

#[derive(Debug, Default)]
pub struct Index {
    pub paras: Vec<ParaPos>,
    pub block_starts: Vec<Shown>,
    /// Blocks in source order; comrak moves footnote definitions written earlier in the text behind them.
    pub in_order: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertPoint {
    pub src: Byte,
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
        let mut pos = Shown::new(0);
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
                pos = pos + len + 1;
            }
        }
        Index {
            paras,
            block_starts,
            in_order,
        }
    }

    /// Index of the paragraph whose text (including its end) contains `p`.
    pub fn para_at(&self, p: Shown) -> Option<usize> {
        if self.paras.is_empty() {
            return None;
        }
        let i = self.paras.partition_point(|place| place.start <= p);
        Some(i.saturating_sub(1))
    }

    pub fn para<'a>(&self, layout: &'a Layout, i: usize) -> &'a Para {
        let place = self.paras[i];
        &layout.blocks[place.block].paras[place.para]
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
    let mut unit = 0;
    for (k, c) in text.chars().enumerate() {
        if unit >= u16_offset {
            return k;
        }
        unit += c.len_utf16();
    }
    text.chars().count()
}

fn text_source(chars: &CharMap, text: &str, end: Byte, k: usize) -> Byte {
    chars.get(text, k).unwrap_or(end)
}

fn text_start(run: &Run) -> Option<Byte> {
    match &run.map {
        RunMap::Text { chars, end } => Some(text_source(chars, &run.text, *end, 0)),
        RunMap::Atom { .. } => None,
    }
}

/// The start of `next` if only a line's prefix, its quote marks and indentation, lies between `from` and it.
fn behind_line_prefix(text: &str, next: Option<&Run>, from: Byte) -> Option<Byte> {
    let start = text_start(next?)?;
    let between = text.get(from.get()..start.get())?;
    between
        .bytes()
        .all(|b| matches!(b, b' ' | b'\t' | b'>'))
        .then_some(start)
}

/// Source position of the start of a paragraph's content.
pub fn content_start(para: &Para) -> Byte {
    match para.runs.first() {
        Some(r) => match &r.map {
            RunMap::Text { chars, end } => text_source(chars, &r.text, *end, 0),
            RunMap::Atom { src, .. } => src.start,
        },
        None => para.insert_at,
    }
}

/// Source position of the end of a paragraph's content.
pub fn content_end(para: &Para) -> Byte {
    match para.runs.last().map(|r| &r.map) {
        Some(RunMap::Text { end, .. }) => *end,
        Some(RunMap::Atom { src, .. }) => src.end,
        None => para.insert_at,
    }
}

/// Where text goes that is typed behind a line break shown as a space: on the next line, in front of its text.
fn behind_atom(run: &Run, next: Option<&Run>) -> Option<Byte> {
    let line_break = matches!(
        run.map,
        RunMap::Atom {
            atom: super::layout::Atom::Text,
            ..
        }
    ) && run.text == " ";
    if line_break { text_start(next?) } else { None }
}

/// Where text typed at preview offset `p` goes. The character left of the caret decides, like typing attributes.
pub fn insert_point(layout: &Layout, index: &Index, p: Shown) -> Option<InsertPoint> {
    let para_index = index.para_at(p)?;
    let place = index.paras[para_index];
    let para = index.para(layout, para_index);
    let marker_len = marker_len(para);
    let off = p.get().saturating_sub(place.start.get()).min(place.len).max(marker_len);
    let caret = place.start + off;
    let make = |src: Byte, style: &RunStyle| InsertPoint {
        src,
        code: style.text.code || matches!(para.style.kind, ParaKind::Code { .. }),
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
    for (ri, (at, run)) in para.runs_at(place.start).enumerate() {
        if caret > at.start && caret <= at.end {
            return Some(match &run.map {
                RunMap::Text { chars, end } => {
                    let k = char_index(&run.text, caret - at.start);
                    make(text_source(chars, &run.text, *end, k), &run.style)
                }
                RunMap::Atom { src, .. } if caret < at.end => make(src.start, &RunStyle::default()),
                RunMap::Atom { src, .. } => {
                    make(behind_atom(run, para.runs.get(ri + 1)).unwrap_or(src.end), &run.style)
                }
            });
        }
    }
    Some(make(para.insert_at, &RunStyle::default()))
}

/// Preview position for a source position; positions inside markup snap to the nearest rendered text.
pub fn preview_offset(layout: &Layout, index: &Index, src: Byte) -> Shown {
    let mut best: Option<(usize, Shown)> = None;
    let mut consider = |distance: usize, pos: Shown| {
        // At the same distance the later one wins: markup at the start of a paragraph belongs to its text.
        if best.is_none_or(|(d, _)| distance <= d) {
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
    let first_para = index.paras.partition_point(|place| place.block < bi.saturating_sub(1));
    for (i, place) in index.paras.iter().enumerate().skip(first_para) {
        if place.block > bi + 1 {
            break;
        }
        let para = index.para(layout, i);
        if para.runs.is_empty() {
            consider(para.insert_at.distance(src), place.start);
        }
        for (at, run) in para.runs_at(place.start) {
            match &run.map {
                RunMap::Text { chars, end } => {
                    let first = chars.get(&run.text, 0);
                    if let (Some(first), true) = (first, src <= *end)
                        && src >= first
                    {
                        let k = chars.count_before(&run.text, src);
                        let u16_in: usize = run.text.chars().take(k).map(char::len_utf16).sum();
                        return at.start + u16_in;
                    }
                    if let Some(first) = first {
                        consider(first.distance(src), at.start);
                        consider(end.distance(src), at.end);
                    }
                }
                RunMap::Atom { src: r, atom } => {
                    if r.start <= src && src < r.end {
                        let after = matches!(atom, super::layout::Atom::Marker);
                        return if after { at.end } else { at.start };
                    }
                    consider(r.start.distance(src), at.start);
                    consider(r.end.distance(src), at.end);
                }
            }
        }
    }
    best.map_or(Shown::new(0), |(_, p)| p)
}

/// Kinds whose paragraphs can be joined by deleting the break between them.
pub fn mergeable(kind: &ParaKind) -> bool {
    matches!(kind, ParaKind::Body | ParaKind::Heading(_) | ParaKind::FootnoteDef)
}

/// Source ranges to delete for a preview range. Markup between rendered characters stays,
/// unless the range joins two paragraphs.
pub fn deletion_ranges(text: &str, layout: &Layout, index: &Index, range: Range<Shown>) -> Vec<Range<Byte>> {
    let mut out = Vec::new();
    let (Some(first), Some(last)) = (index.para_at(range.start), index.para_at(range.end)) else {
        return out;
    };
    // Something that is deleted as a whole, like front matter or a rule, leaves no break behind.
    let goes_whole = |para_index: usize| {
        let place = index.paras[para_index];
        let para = index.para(layout, para_index);
        range.start <= place.start
            && place.start + place.len <= range.end
            && para.runs.iter().all(|r| matches!(r.map, RunMap::Atom { .. }))
    };
    let joins = first != last
        && (mergeable(&index.para(layout, first).style.kind) || goes_whole(first))
        && (mergeable(&index.para(layout, last).style.kind) || goes_whole(last));
    if joins {
        out.push(content_end(index.para(layout, first))..content_start(index.para(layout, last)));
    }
    for para_index in first..=last {
        let place = index.paras[para_index];
        let para = index.para(layout, para_index);
        for (ri, (at, run)) in para.runs_at(place.start).enumerate() {
            let (hit_start, hit_end) = (range.start.max(at.start), range.end.min(at.end));
            if hit_start < hit_end {
                match &run.map {
                    RunMap::Text { chars, end } => {
                        let k1 = char_index(&run.text, hit_start - at.start);
                        let k2 = char_index(&run.text, hit_end - at.start);
                        out.push(text_source(chars, &run.text, *end, k1)..text_source(chars, &run.text, *end, k2));
                    }
                    RunMap::Atom { src, .. } => {
                        let end = if run.text == " " {
                            behind_line_prefix(text, para.runs.get(ri + 1), src.end)
                        } else {
                            None
                        };
                        out.push(src.start..end.unwrap_or(src.end));
                    }
                }
            }
        }
        if !joins
            && para_index != first
            && para_index != last
            && para.runs.iter().all(|r| matches!(r.map, RunMap::Atom { .. }))
        {
            out.push(para.src.clone());
        }
    }
    normalize(out)
}

pub fn normalize(mut ranges: Vec<Range<Byte>>) -> Vec<Range<Byte>> {
    ranges.retain(|r| r.start < r.end);
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<Range<Byte>> = Vec::new();
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
    use crate::markdown::reader::parse;
    use crate::preview::layout::{NoImages, layout};

    /// Text appended after footnotes lies in the preview before the definitions comrak moved behind it.
    #[test]
    fn source_positions_find_their_block_behind_moved_footnotes() {
        let t = "Text^[innen] und[^1].\n\n[^1]: Anmerkung.\n\nNeu am Ende\n";
        let d = parse(t);
        let l = layout(&d, &NoImages, None);
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
            preview_offset(&l, &i, Byte::new(def_end)),
            Shown::new(preview[..shown_end].encode_utf16().count())
        );
        let end = t.find("Neu am Ende").unwrap() + "Neu am Ende".len();
        let p = preview_offset(&l, &i, Byte::new(end));
        let at = preview.find("Neu am Ende").unwrap() + "Neu am Ende".len();
        assert_eq!(p, Shown::new(preview[..at].encode_utf16().count()));

        assert!(preview.contains("1.\tinnen"), "{preview}");
        // The empty paragraph for a caret at the end of the text stays with the text, before the footnotes.
        let caret = t.len();
        let with_caret = layout(&parse(&format!("{t}\n")), &NoImages, Some(Byte::new(caret + 1)));
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
    use crate::markdown::reader::parse;
    use crate::markdown::text::{TextAt, bytes, offsets};
    use crate::preview::layout::{NoImages, layout};

    fn setup(t: &str) -> (Layout, Index) {
        let l = layout(&parse(t), &NoImages, None);
        let i = Index::new(&l);
        (l, i)
    }

    fn deleted(t: &str, from: usize, to: usize) -> String {
        let (l, i) = setup(t);
        let mut s = t.to_string();
        for d in deletion_ranges(t, &l, &i, Shown::new(from)..Shown::new(to))
            .iter()
            .rev()
        {
            s.replace_range(offsets(d), "");
        }
        s
    }

    #[test]
    fn a_deletion_joins_paragraphs_only_where_text_can_flow_together() {
        // Shown: `eins`, then the code: `ab`, `zwei`.
        let t = "eins\n\n```\nab\n```\n";
        assert_eq!(deleted(t, 2, 6), "ei\n\n```\nb\n```\n", "code stays code");
        assert_eq!(deleted("eins\n\n> zwei\n", 2, 7), "eiei\n", "a quote's text flows on");
        assert_eq!(deleted("# eins\n\nzwei\n", 2, 7), "# eiei\n");
        assert_eq!(deleted("a\n\nb\n\nc\n", 0, 4), "c\n", "through a whole paragraph");
    }

    #[test]
    fn what_goes_as_a_whole_leaves_no_break_but_only_when_all_of_it_goes() {
        assert_eq!(
            deleted("a\n\n---\n\nb\n", 0, 4),
            "b\n",
            "a rule between two deleted ends"
        );
        let (l, i) = setup("a\n\n---\n\nb\n");
        let rule = l.blocks[1].paras[0].runs[0].text.encode_utf16().count();
        assert_eq!(rule, 1);
        assert_eq!(i.paras[1].start, Shown::new(2));
        // From inside the paragraph to the end of the rule: the break behind the rule stays.
        assert_eq!(deleted("ab\n\n---\n\nc\n", 1, 4), "a\n\nc\n");
        // From the start of a rule to inside the paragraph: the break before the rule stays.
        assert_eq!(deleted("a\n\n---\n\nbc\n", 2, 5), "a\n\nc\n");
        assert_eq!(
            deleted("```\na\n```\n\n---\n\n```\nb\n```\n", 0, 4),
            "```\n\n```\n\n\n\n```\nb\n```\n",
            "between two code blocks, nothing flows together"
        );
    }

    #[test]
    fn a_space_for_a_line_break_goes_with_the_line_break_up_to_the_next_text() {
        assert_eq!(deleted("ab\ncd\n", 2, 3), "abcd\n");
        assert_eq!(deleted("> ab\n> cd\n", 2, 3), "> abcd\n");
        assert_eq!(
            deleted("ab\n![x](y.png)\n", 2, 3),
            "ab![x](y.png)\n",
            "no text to run up to"
        );
    }

    #[test]
    fn deletions_that_touch_are_one_and_those_with_a_gap_are_two() {
        assert_eq!(normalize(vec![bytes(0..2), bytes(2..4)]), vec![bytes(0..4)]);
        assert_eq!(normalize(vec![bytes(3..4), bytes(0..2)]), [bytes(0..2), bytes(3..4)]);
        assert_eq!(normalize(vec![bytes(0..4), bytes(1..2)]), vec![bytes(0..4)]);
        assert_eq!(normalize(vec![bytes(1..1), bytes(0..0)]), Vec::<Range<Byte>>::new());
    }

    #[test]
    fn only_running_text_flows_together() {
        for kind in [ParaKind::Body, ParaKind::Heading(1), ParaKind::FootnoteDef] {
            assert!(mergeable(&kind), "{kind:?}");
        }
        for kind in [
            ParaKind::Rule,
            ParaKind::MathBlock,
            ParaKind::Diagram,
            ParaKind::FrontMatter,
        ] {
            assert!(!mergeable(&kind), "{kind:?}");
        }
    }

    #[test]
    fn the_caret_finds_its_place_around_markers_and_atoms() {
        let t = "- ab\n";
        let (l, i) = setup(t);
        let marker = marker_len(&l.blocks[0].paras[0]);
        assert_eq!(marker, 2);
        assert_eq!(
            insert_point(&l, &i, Shown::new(0)).unwrap().src,
            Byte::new(t.find('a').unwrap()),
            "before the text, not the marker"
        );
        assert!(insert_point(&l, &i, Shown::new(marker)).unwrap().at_para_start);
        assert!(!insert_point(&l, &i, Shown::new(marker + 1)).unwrap().at_para_start);
        let t = "a ![b](c.png) d\n";
        let (l, i) = setup(t);
        assert_eq!(
            insert_point(&l, &i, Shown::new(3)).unwrap().src,
            Byte::new(t.find(" d").unwrap()),
            "behind the picture"
        );
        assert_eq!(
            insert_point(&l, &i, Shown::new(2)).unwrap().src,
            Byte::new(t.find('!').unwrap()),
            "before it"
        );
        let t = "ab\ncd\n";
        let (l, i) = setup(t);
        assert_eq!(
            insert_point(&l, &i, Shown::new(3)).unwrap().src,
            Byte::new(t.find('c').unwrap()),
            "behind a line end, on the next line"
        );
        let t = "[ab](c) d\n";
        let (l, i) = setup(t);
        let edge = insert_point(&l, &i, Shown::new(2)).unwrap();
        assert_eq!(
            edge.src,
            Byte::new(t.find("](").unwrap()),
            "typing at the end of a link stays in it"
        );
        assert!(insert_point(&l, &i, Shown::new(1)).is_some_and(|p| !p.code && !p.html));
        let t = "`ab` c\n";
        let (l, i) = setup(t);
        assert!(insert_point(&l, &i, Shown::new(1)).unwrap().code);
        assert!(!insert_point(&l, &i, Shown::new(3)).unwrap().code);
    }

    #[test]
    fn a_source_position_finds_the_nearest_thing_shown() {
        let t = "# Titel\n\n**fett** ![b](c.png)\n\n- Punkt\n";
        let (l, i) = setup(t);
        let at = |needle: &str| t.find(needle).unwrap();
        assert_eq!(
            preview_offset(&l, &i, Byte::new(0)),
            Shown::new(0),
            "the hashes stand for the start of the title"
        );
        assert_eq!(preview_offset(&l, &i, Byte::new(at("Titel"))), Shown::new(0));
        assert_eq!(preview_offset(&l, &i, Byte::new(at("itel"))), Shown::new(1));
        assert_eq!(
            preview_offset(&l, &i, Byte::new(at("\n\n**"))),
            Shown::new(5),
            "the end of a line is the end of what it shows"
        );
        let fett = i.paras[1].start;
        assert_eq!(
            preview_offset(&l, &i, Byte::new(at("**fett"))),
            fett,
            "markup snaps to the text"
        );
        assert_eq!(preview_offset(&l, &i, Byte::new(at("fett"))), fett);
        assert_eq!(preview_offset(&l, &i, Byte::new(at("ett"))), fett + 1);
        let picture = fett + 5;
        assert_eq!(
            preview_offset(&l, &i, Byte::new(at("![b]"))),
            picture,
            "at a picture, before it"
        );
        assert_eq!(
            preview_offset(&l, &i, Byte::new(at("[b]"))),
            picture,
            "inside its source too"
        );
        assert_eq!(
            preview_offset(&l, &i, Byte::new(at("- Punkt"))),
            i.paras[2].start + 2,
            "a marker counts behind itself"
        );
        assert_eq!(
            preview_offset(&l, &i, Byte::new(t.len())),
            i.paras[2].start + 7,
            "the end of the text"
        );
    }

    #[test]
    fn a_position_in_an_empty_paragraph_is_that_paragraph() {
        let t = "a\n\n\n\nb\n";
        let (l, i) = setup(t);
        let empty = i.paras.iter().position(|p| p.len == 0).expect("an empty paragraph");
        let at = i.para(&l, empty).insert_at;
        assert_eq!(preview_offset(&l, &i, at), i.paras[empty].start);
    }

    #[test]
    fn typing_at_end_of_bold_stays_bold() {
        let t = "a **fett** b\n";
        let (l, i) = setup(t);
        let point = insert_point(&l, &i, Shown::new(6)).unwrap();
        assert_eq!(point.src, Byte::new(t.find("** b").unwrap()));
        let ip0 = insert_point(&l, &i, Shown::new(0)).unwrap();
        assert_eq!(ip0.src, Byte::new(0));
    }

    #[test]
    fn preview_offsets_roundtrip() {
        let t = "# Ü\n\nx **fett** y\n";
        let (l, i) = setup(t);
        assert_eq!(
            preview_offset(&l, &i, Byte::new(t.find("fett").unwrap())),
            Shown::new(2 + 2)
        );
        assert_eq!(preview_offset(&l, &i, Byte::new(t.find("Ü").unwrap())), Shown::new(0));
        assert_eq!(
            preview_offset(&l, &i, Byte::new(t.find('y').unwrap())),
            Shown::new(2 + 7)
        );
    }

    #[test]
    fn deleting_across_formatting_keeps_delimiters() {
        let t = "ab **cd** ef\n";
        let (l, i) = setup(t);
        let r = deletion_ranges(t, &l, &i, Shown::new(1)..Shown::new(4));
        let mut s = t.to_string();
        for d in r.iter().rev() {
            s.replace_range(offsets(d), "");
        }
        assert_eq!(s, "a**d** ef\n");
    }

    #[test]
    fn deleting_paragraph_break_joins_blocks() {
        let t = "eins\n\n## zwei\n";
        let (l, i) = setup(t);
        let r = deletion_ranges(t, &l, &i, Shown::new(4)..Shown::new(5));
        let mut s = t.to_string();
        for d in r.iter().rev() {
            s.replace_range(offsets(d), "");
        }
        assert_eq!(s, "einszwei\n");
    }

    #[test]
    fn soft_break_maps_to_next_line_text() {
        let t = "> zitat\n> zwei\n";
        let (l, i) = setup(t);
        let point = insert_point(&l, &i, Shown::new(6)).unwrap();
        assert_eq!(t.at(point.src..point.src + 4), "zwei");
    }

    #[test]
    fn a_caret_inside_an_atom_goes_before_it_and_behind_it_goes_behind_it() {
        let t = "> [!note] Titel\n> Inhalt\n";
        let (l, i) = setup(t);
        let para = &l.blocks[0].paras[0];
        let label = para
            .runs
            .iter()
            .position(|r| r.text.contains("Titel"))
            .expect("a title");
        let RunMap::Atom { src, .. } = &para.runs[label].map else {
            panic!("the title is one piece");
        };
        let before: usize = para.runs[..label].iter().map(|r| run_len(&r.text)).sum();
        let len = run_len(&para.runs[label].text);
        for inside in [before + 1, before + len - 1] {
            assert_eq!(
                insert_point(&l, &i, Shown::new(inside)).unwrap().src,
                src.start,
                "{inside}"
            );
        }
        assert_eq!(insert_point(&l, &i, Shown::new(before + len)).unwrap().src, src.end);
    }

    #[test]
    fn only_a_space_for_a_line_break_runs_on_to_the_next_text() {
        let t = ":smile:**b**\n";
        let (l, i) = setup(t);
        assert_eq!(
            insert_point(&l, &i, Shown::new(2)).unwrap().src,
            Byte::new(t.find("**").unwrap())
        );
        let t = "ab\n![x](y.png)c\n";
        let (l, i) = setup(t);
        assert_eq!(
            insert_point(&l, &i, Shown::new(3)).unwrap().src,
            Byte::new(t.find('!').unwrap()),
            "before the picture"
        );
    }

    #[test]
    fn a_position_in_a_note_belongs_to_the_block_around_it_or_to_its_definition() {
        let t = "Text^[innen] und.\n";
        let (l, i) = setup(t);
        assert_eq!(
            preview_offset(&l, &i, Byte::new(t.find("innen").unwrap())),
            Shown::new(4),
            "the reference"
        );
        let t = "Text^[innen] und.\n\nzwei\n\ndrei\n";
        let (l, i) = setup(t);
        assert_eq!(
            preview_offset(&l, &i, Byte::new(t.find("innen").unwrap())),
            Shown::new(4),
            "far from the note"
        );

        let t = "a[^1].\n\n[^1]: Anm.\n\nb\n";
        let (l, i) = setup(t);
        let shown = i
            .paras
            .iter()
            .enumerate()
            .find(|(k, _)| i.para(&l, *k).runs.iter().any(|r| r.text == "Anm."));
        let (k, place) = shown.expect("the definition is shown");
        let run = i.para(&l, k).runs.iter().position(|r| r.text == "Anm.").unwrap();
        let lead: usize = i.para(&l, k).runs[..run].iter().map(|r| run_len(&r.text)).sum();
        assert_eq!(
            preview_offset(&l, &i, Byte::new(t.find("Anm.").unwrap() + 1)),
            place.start + lead + 1
        );
    }

    #[test]
    fn a_position_between_blocks_finds_the_nearer_one() {
        let t = "aa\n\n[r]: http://x\n\nbb\n\n[^1]: d\n\ncc[^1]\n";
        let (l, i) = setup(t);
        let bb = i
            .paras
            .iter()
            .enumerate()
            .find(|(k, _)| i.para(&l, *k).runs.iter().any(|r| r.text == "bb"));
        let bb = bb.expect("the block is shown").1.start;
        assert_eq!(
            preview_offset(&l, &i, Byte::new(t.find("bb").unwrap() - 1)),
            bb,
            "the next block"
        );
        assert_eq!(preview_offset(&l, &i, Byte::new(3)), Shown::new(2), "the block before");
    }

    #[test]
    fn behind_a_picture_is_behind_it_in_the_preview_as_well() {
        let t = "a ![b](c.png) d\n";
        let (l, i) = setup(t);
        assert_eq!(preview_offset(&l, &i, Byte::new(t.find(" d").unwrap())), Shown::new(3));
    }

    #[test]
    fn a_deletion_from_behind_a_rule_keeps_the_rule_and_its_break() {
        assert_eq!(deleted("---\n\nab\n", 1, 3), "---\n\nb\n");
    }

    #[test]
    fn a_rule_between_ends_that_do_not_flow_together_goes_alone() {
        assert_eq!(
            deleted("```\ncode\n```\n\n---\n\nabc\n", 2, 7),
            "```\nco\n```\n\n\n\nabc\n"
        );
    }

    #[test]
    fn a_rule_a_deletion_ends_at_stays() {
        assert_eq!(
            deleted("```\ncode\n```\n\n---\n\nabc\n", 2, 5),
            "```\nco\n```\n\n---\n\nabc\n"
        );
    }

    #[test]
    fn a_block_of_html_between_ends_that_do_not_flow_together_goes_with_its_tags() {
        assert_eq!(
            deleted("```\ncode\n```\n\n<div>x</div>\n\nabc\n", 2, 7),
            "```\nco\n```\n\n\n\nabc\n"
        );
    }

    #[test]
    fn a_rule_at_the_start_of_a_deletion_stays_when_the_deletion_starts_behind_it() {
        assert_eq!(deleted("---\n\n```\ncode\n```\n", 1, 4), "---\n\n```\nde\n```\n");
    }

    #[test]
    fn deleting_a_line_break_leaves_what_stands_between_it_and_the_next_text() {
        assert_eq!(deleted("ab\n![x](y.png)c\n", 2, 3), "ab![x](y.png)c\n", "a picture");
        assert_eq!(deleted("ab\n**cd**\n", 2, 3), "ab**cd**\n", "markers");
    }
}
