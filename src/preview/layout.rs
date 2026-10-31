use std::hash::{Hash, Hasher};
use std::ops::Range;

use crate::markdown::doc::{AlertKind, Align, Block, BlockKind, DiagramKind, Doc, Inline, InlineKind, ListKind};
use crate::markdown::incremental::Splice;
use crate::markdown::text::right_to_left;
use crate::markdown::typography::{Mark, Open, Quotes, marks, smarten, typographic_source};

mod html;

pub const ATTACHMENT: char = '\u{FFFC}';
pub const LINE_SEPARATOR: char = '\u{2028}';

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParaKind {
    Body,
    Heading(u8),
    Code {
        info: Box<str>,
    },
    Rule,
    MathBlock,
    Diagram,
    TableCell {
        table: u32,
        row: u32,
        col: u32,
        header: bool,
        align: AlignKey,
        columns: u32,
        /// Columns the cell spans, more than one only in HTML tables.
        span: u32,
    },
    FootnoteDef,
    AlertTitle,
    /// Front matter, shown as one line naming its keys.
    FrontMatter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AlignKey {
    #[default]
    Natural,
    Left,
    Center,
    Right,
}

impl From<Align> for AlignKey {
    fn from(a: Align) -> Self {
        match a {
            Align::None => AlignKey::Natural,
            Align::Left => AlignKey::Left,
            Align::Center => AlignKey::Center,
            Align::Right => AlignKey::Right,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParaStyle {
    pub kind: ParaKind,
    pub quote_depth: u8,
    pub list_depth: u8,
    /// Marker drawn before the first paragraph of a list item or footnote definition.
    pub marker: Option<Marker>,
    /// Paragraphs sharing a group id share one background or border block (code, quotes, tables).
    pub group: usize,
    pub tight: bool,
    /// First paragraph after a table, which cannot carry spacing below itself.
    pub space_before: bool,
    pub alert: Option<AlertKind>,
    /// Alignment embedded HTML asks for, as `<p align="center">`.
    pub align: AlignKey,
    /// Rendered from embedded HTML, which is edited in the source only.
    pub html: bool,
    /// Whether the paragraph reads right to left, from its first letter; `None` without letters.
    pub rtl: Option<bool>,
    /// The quote, callout or table around the paragraph reads right to left, which mirrors its border and columns.
    pub mirrored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Marker {
    Bullet(u8),
    Number(usize),
    Task {
        checked: bool,
        symbol: usize,
    },
    /// The footnote's name, which references point to, and the label shown.
    Footnote(Box<str>, Box<str>),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct RunStyle {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub underline: bool,
    pub highlight: bool,
    pub code: bool,
    pub superscript: bool,
    pub subscript: bool,
    pub html: bool,
    pub spoiler: bool,
    /// An Obsidian `#tag`.
    pub tag: bool,
    /// A mention, issue or merge request of the document's repository.
    pub reference: bool,
    /// A Pandoc citation.
    pub citation: bool,
    pub link: Option<Box<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Atom {
    /// Text that represents source without being editable character by character.
    Text,
    /// List bullet, number, checkbox or footnote label in front of a paragraph.
    Marker,
    /// Boxed, as the largest kind would otherwise size every run.
    Image(Box<ImageAtom>),
    Math {
        tex: Box<str>,
        display: bool,
    },
    Diagram {
        source: Box<str>,
        kind: DiagramKind,
    },
    FootnoteRef {
        name: Box<str>,
    },
    Rule,
    AlertIcon(AlertKind),
    /// The chevron of a foldable callout; its source is the `-` or `+` a click flips.
    Fold {
        open: bool,
    },
    /// The chevron of an HTML `<details>` element; its source is the opening tag, whose `open` a click toggles.
    Details {
        open: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageAtom {
    pub url: Box<str>,
    pub alt: Box<str>,
    /// The size embedded HTML asks for, in pixels.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RunMap {
    /// Source byte offset of every char of the run text, plus the end of the last char.
    Text {
        chars: CharMap,
        end: usize,
    },
    Atom {
        atom: Atom,
        src: Range<usize>,
    },
}

/// Source offsets of a run's characters. Text copied verbatim from the source needs no table, which saves eight
/// bytes per character for almost all text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CharMap {
    Verbatim(usize),
    /// Offsets relative to the first char's: a run never spans gigabytes.
    Offsets(usize, Box<[u32]>),
}

impl CharMap {
    /// Source offset of char `k` of the run text `text`, or `None` past its end.
    pub fn get(&self, text: &str, k: usize) -> Option<usize> {
        match self {
            CharMap::Verbatim(start) => text.char_indices().nth(k).map(|(b, _)| start + b),
            CharMap::Offsets(base, v) => v.get(k).map(|&o| base + o as usize),
        }
    }

    /// Number of chars of `text` whose source offset lies before `src`.
    pub fn count_before(&self, text: &str, src: usize) -> usize {
        match self {
            CharMap::Verbatim(start) => text.char_indices().take_while(|(b, _)| start + b < src).count(),
            CharMap::Offsets(base, v) => v.partition_point(|&c| base + (c as usize) < src),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
    pub map: RunMap,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Para {
    pub style: ParaStyle,
    pub runs: Vec<Run>,
    /// Source range of the leaf block this paragraph renders.
    pub src: Range<usize>,
    /// Where typed text goes when the paragraph has no text yet.
    pub insert_at: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopBlock {
    pub paras: Vec<Para>,
    pub src: Range<usize>,
    pub key: u64,
    pub len_u16: usize,
    /// The HTML wrappers around the block, as `<details>` before it, which a taken-over block must share.
    pub context: u64,
}

#[derive(Debug, Default)]
pub struct Layout {
    pub blocks: Vec<TopBlock>,
}

/// Decides whether rendered images for math and diagrams exist yet; otherwise their source is shown as text.
pub trait Resolver {
    fn math_ready(&self, tex: &str, display: bool) -> bool;
    fn diagram_ready(&self, source: &str, kind: DiagramKind) -> bool;
    /// The quotation marks straight quotes turn into, when typographic punctuation is on.
    fn quotes(&self) -> Option<Quotes> {
        None
    }
    /// Whether the document lies in a repository whose code host `@name`, `#123` and `!123` point into.
    fn references(&self) -> bool {
        false
    }
    /// How a citation key reads, such as `Doe 2020`, when the bibliography holds it.
    fn citation(&self, _key: &str) -> Option<String> {
        None
    }
    /// The text of the note `![[target]]` embeds and the folder it lies in.
    fn embedded_note(&self, _target: &str) -> Option<(String, String)> {
        None
    }
}

/// Everything of another resolver except embedded notes, so a note embedded in a note shows as a link.
struct WithoutEmbeds<'a>(&'a dyn Resolver);

impl Resolver for WithoutEmbeds<'_> {
    fn math_ready(&self, tex: &str, display: bool) -> bool {
        self.0.math_ready(tex, display)
    }
    fn diagram_ready(&self, source: &str, kind: DiagramKind) -> bool {
        self.0.diagram_ready(source, kind)
    }
    fn quotes(&self) -> Option<Quotes> {
        self.0.quotes()
    }
    fn references(&self) -> bool {
        self.0.references()
    }
    fn citation(&self, key: &str) -> Option<String> {
        self.0.citation(key)
    }
}

/// Shows formulas, diagrams and images as not rendered yet.
pub struct NoImages;

impl Resolver for NoImages {
    fn math_ready(&self, _: &str, _: bool) -> bool {
        false
    }
    fn diagram_ready(&self, _: &str, _: DiagramKind) -> bool {
        false
    }
}

struct Builder<'a> {
    text: &'a str,
    resolver: &'a dyn Resolver,
    group: usize,
    table: usize,
    paras: Vec<Para>,
    /// Quotes the runs of the next paragraph left open so far.
    open: std::cell::Cell<Open>,
}

#[derive(Clone, Default)]
struct Ctx {
    quote_depth: u8,
    list_depth: u8,
    group: usize,
    tight: bool,
    alert: Option<AlertKind>,
    align: AlignKey,
    html: bool,
}

/// Lays out the document for the preview. `caret` is the source caret: when it sits on a blank line
/// between blocks, an empty paragraph is shown there so a new paragraph can be typed.
pub fn layout(text: &str, doc: &Doc, resolver: &dyn Resolver, caret: Option<usize>) -> Layout {
    build(text, doc, resolver, caret, Vec::new(), &|_| None)
}

/// Like `layout`, but takes over the laid-out blocks of `old` for the document blocks `reuse` returns the offset
/// shift of; their text must be unchanged and they must not hold the previous caret.
pub fn relayout(
    text: &str,
    doc: &Doc,
    resolver: &dyn Resolver,
    caret: Option<usize>,
    old: Layout,
    reuse: &dyn Fn(usize) -> Option<isize>,
) -> Layout {
    build(text, doc, resolver, caret, old.blocks, reuse)
}

/// The `reuse` argument of `relayout` after a partial parse of the new `doc`: blocks outside the splice keep their
/// layout, shifted behind it, unless they held the caret the old layout was made for.
pub fn reusable<'a>(
    splice: &'a Splice,
    doc: &'a Doc,
    old_caret: Option<usize>,
) -> impl Fn(usize) -> Option<isize> + 'a {
    move |i| {
        let shift = if i < splice.new.start {
            0
        } else if i >= splice.new.end {
            splice.delta
        } else {
            return None;
        };
        let r = &doc.blocks.get(i)?.range;
        let (start, end) = ((r.start as isize - shift) as usize, (r.end as isize - shift) as usize);
        (!old_caret.is_some_and(|c| start <= c && c <= end)).then_some(shift)
    }
}

struct Part {
    paras: Vec<Para>,
    src: Range<usize>,
    /// Key and length of a block taken over unchanged.
    kept: Option<(u64, usize)>,
    context: u64,
}

fn build(
    text: &str,
    doc: &Doc,
    resolver: &dyn Resolver,
    caret: Option<usize>,
    old: Vec<TopBlock>,
    reuse: &dyn Fn(usize) -> Option<isize>,
) -> Layout {
    let mut b = Builder {
        text,
        resolver,
        group: 0,
        table: 0,
        paras: Vec::new(),
        open: Default::default(),
    };
    let old_starts: Vec<usize> = old.iter().map(|o| o.src.start).collect();
    let mut old: Vec<Option<TopBlock>> = old.into_iter().map(Some).collect();
    let mut parts: Vec<Part> = Vec::with_capacity(doc.blocks.len());
    let wrappers = html_wrappers(&doc.blocks);
    for (i, block) in doc.blocks.iter().enumerate() {
        let wrapper = wrappers.get(i).copied().unwrap_or_default();
        if wrapper.hidden {
            continue;
        }
        let context = wrapper.key();
        if let Some(shift) = reuse(i) {
            let at = (block.range.start as isize - shift) as usize;
            let found = old_starts
                .binary_search(&at)
                .ok()
                .and_then(|k| old[k].take())
                .filter(|o| !o.src.is_empty() && o.src.len() == block.range.len() && o.context == context);
            if let Some(o) = found {
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
        b.block(block, &ctx, None);
        let paras = std::mem::take(&mut b.paras);
        if !paras.is_empty() {
            parts.push(Part {
                paras,
                src: block.range.clone(),
                kept: None,
                context,
            });
        }
    }
    drop(old);
    if let Some(c) = caret {
        // Only a paragraph ending on the caret's line can show trailing spaces.
        let line = text[..c.min(text.len())].rfind('\n').map_or(0, |i| i + 1)
            ..text[c.min(text.len())..].find('\n').map_or(text.len(), |i| c + i);
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
    let ranges: Vec<Range<usize>> = parts.iter().map(|p| p.src.clone()).collect();
    let spots = empty_line_spots(text, &ranges);
    let virtual_at = caret.filter(|&c| {
        // Footnote definitions comrak moved behind the text break the source order of the blocks.
        let inside = doc.blocks.iter().any(|b| b.range.start <= c && c <= b.range.end);
        let line_start = text[..c].rfind('\n').map_or(0, |i| i + 1);
        let line_end = text[c..].find('\n').map_or(text.len(), |i| c + i);
        !inside && text[line_start..line_end].trim().is_empty() && spots.binary_search(&line_start).is_err()
    });
    let virtual_at = virtual_at.or(if parts.is_empty() && spots.is_empty() {
        Some(text.len())
    } else {
        None
    });
    let mut extra: Vec<usize> = spots.into_iter().chain(virtual_at).collect();
    extra.sort_unstable();
    let mut merged = Vec::with_capacity(parts.len() + extra.len());
    let mut extra = extra.into_iter().peekable();
    let mut in_order_until = 0;
    for part in parts {
        // Empty lines after the text come before the footnote definitions comrak moved behind it.
        let moved = part.src.start < in_order_until;
        while let Some(&c) = extra.peek().filter(|&&c| moved || c <= part.src.start) {
            merged.push(Part {
                paras: vec![empty_para(c)],
                src: c..c,
                kept: None,
                context: 0,
            });
            extra.next();
        }
        if !moved {
            in_order_until = part.src.start;
        }
        merged.push(part);
    }
    merged.extend(extra.map(|c| Part {
        paras: vec![empty_para(c)],
        src: c..c,
        kept: None,
        context: 0,
    }));
    let mut parts = merged;
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
    let blocks = parts
        .into_iter()
        .map(
            |Part {
                 mut paras,
                 src,
                 kept,
                 context,
             }| {
                if let Some((key, len_u16)) = kept {
                    return TopBlock {
                        paras,
                        src,
                        key,
                        len_u16,
                        context,
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
                }
            },
        )
        .collect();
    Layout { blocks }
}

/// What HTML wrapping a top-level block asks of it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
struct Wrapper {
    align: AlignKey,
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
    let mut stack: Vec<(String, Option<AlignKey>, bool)> = Vec::new();
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        out.push(Wrapper {
            align: stack.iter().rev().find_map(|(_, a, _)| *a).unwrap_or_default(),
            hidden: stack.iter().any(|(_, _, hidden)| *hidden),
        });
        let BlockKind::HtmlBlock { literal } = &block.kind else {
            continue;
        };
        for name in crate::markdown::markup::stray_closes(literal) {
            if let Some(at) = stack.iter().rposition(|(n, _, _)| *n == name) {
                stack.truncate(at);
            }
        }
        let nodes = crate::markdown::markup::parse(literal);
        for e in crate::markdown::markup::open_at_end(&nodes) {
            let hidden = e.name == "details" && e.attr("open").is_none();
            stack.push((
                e.name.clone(),
                e.align()
                    .map(align_key)
                    .or((e.name == "center").then_some(AlignKey::Center)),
                hidden,
            ));
        }
    }
    out
}

fn align_key(name: &str) -> AlignKey {
    match name {
        "center" => AlignKey::Center,
        "right" => AlignKey::Right,
        "left" => AlignKey::Left,
        _ => AlignKey::Natural,
    }
}

/// Moves every source offset of a laid-out paragraph by `delta`.
fn shift_para(p: &mut Para, delta: isize) {
    let at = |o: usize| (o as isize + delta) as usize;
    p.src = at(p.src.start)..at(p.src.end);
    p.insert_at = at(p.insert_at);
    // An embedded note's checkboxes point nowhere and stay so.
    if let Some(Marker::Task { symbol, .. }) = &mut p.style.marker
        && *symbol != usize::MAX
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

fn empty_para(at: usize) -> Para {
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
            align: AlignKey::Natural,
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
pub fn empty_line_spots(text: &str, blocks: &[Range<usize>]) -> Vec<usize> {
    let mut spots = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    let mut flush = |run: &mut Vec<usize>, at_start: bool, at_end: bool| {
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
    let mut region = |from: usize, to: usize, at_start: bool, at_end: bool| {
        let mut start = from;
        let mut lead = at_start;
        let pieces: Vec<&str> = text[from..to].split('\n').collect();
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
    let next_line = |end: usize, limit: usize| {
        if end > 0 && text.as_bytes()[end - 1] == b'\n' {
            Some(end)
        } else {
            text[end..limit].find('\n').map(|i| end + i + 1)
        }
    };
    let mut prev_end: Option<usize> = None;
    for b in blocks {
        match prev_end {
            None => region(0, b.start, true, false),
            Some(end) => {
                if let Some(from) = next_line(end, b.start.max(end)) {
                    region(from, b.start.max(from), false, false);
                }
            }
        }
        prev_end = Some(b.end.max(prev_end.unwrap_or(0)));
    }
    match prev_end {
        Some(end) => {
            if let Some(from) = next_line(end, text.len()) {
                region(from, text.len(), false, true);
            }
        }
        None => region(0, text.len(), true, true),
    }
    spots
}

/// Markdown drops spaces at the end of a paragraph, so a space just typed there would vanish from the preview and the
/// next character would land in front of it. While the caret sits in such spaces they are shown as text.
fn show_trailing_spaces(text: &str, para: &mut Para, caret: usize) {
    if !matches!(para.style.kind, ParaKind::Body | ParaKind::Heading(_)) {
        return;
    }
    let content_end = super::map::content_end(para);
    if content_end > text.len() {
        return;
    }
    let end = text[content_end..].find('\n').map_or(text.len(), |i| content_end + i);
    let start = end
        - text[content_end..end]
            .bytes()
            .rev()
            .take_while(|b| matches!(b, b' ' | b'\t'))
            .count();
    if start == end || caret < start || caret > end {
        return;
    }
    para.runs.push(Run {
        text: text[start..end].to_string(),
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

impl Builder<'_> {
    fn next_group(&mut self) -> usize {
        self.group += 1;
        self.group
    }

    fn push_para(
        &mut self,
        kind: ParaKind,
        ctx: &Ctx,
        marker: Option<(Marker, Range<usize>)>,
        src: Range<usize>,
        mut runs: Vec<Run>,
        insert_at: usize,
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
    fn set_direction(&mut self, start: usize, group: usize) {
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

    fn block(&mut self, block: &Block, ctx: &Ctx, marker: Option<(Marker, Range<usize>)>) {
        match &block.kind {
            BlockKind::Paragraph => {
                if let Some(target) = only_embed(&block.inlines)
                    && let Some((note, folder)) = self.resolver.embedded_note(target)
                {
                    self.embed(&note, &folder, block.range.clone(), ctx);
                    return;
                }
                let runs = self.inlines(&block.inlines, &RunStyle::default());
                let insert_at = block.inline_range().map_or(block.range.start, |r| r.start);
                self.push_para(ParaKind::Body, ctx, marker, block.range.clone(), runs, insert_at);
            }
            BlockKind::Heading { level, setext, .. } => {
                let runs = self.inlines(&block.inlines, &RunStyle::default());
                let insert_at = block.inline_range().map_or_else(
                    || {
                        if *setext {
                            block.range.start
                        } else {
                            self.heading_insert(&block.range)
                        }
                    },
                    |r| r.start,
                );
                self.push_para(
                    ParaKind::Heading(*level),
                    ctx,
                    marker,
                    block.range.clone(),
                    runs,
                    insert_at,
                );
            }
            BlockKind::BlockQuote => {
                let inner = Ctx {
                    quote_depth: ctx.quote_depth + 1,
                    group: self.next_group(),
                    ..ctx.clone()
                };
                let start = self.paras.len();
                let mut marker = marker;
                if block.children.is_empty() {
                    self.push_para(
                        ParaKind::Body,
                        &inner,
                        marker.take(),
                        block.range.clone(),
                        vec![],
                        block.range.end,
                    );
                }
                for child in &block.children {
                    self.block(child, &inner, marker.take());
                }
                self.set_direction(start, inner.group);
            }
            BlockKind::Alert {
                kind,
                title,
                marker: title_src,
                fold,
            } => {
                let inner = Ctx {
                    quote_depth: ctx.quote_depth + 1,
                    group: self.next_group(),
                    alert: Some(*kind),
                    ..ctx.clone()
                };
                let label = title.as_deref().unwrap_or(crate::i18n::t(kind.label()));
                let mut runs = vec![
                    atom(Atom::AlertIcon(*kind), &ATTACHMENT.to_string(), title_src.clone()),
                    atom(Atom::Text, &format!(" {label}"), title_src.clone()),
                ];
                if let Some(f) = fold {
                    runs.push(atom_styled(
                        Atom::Fold { open: f.open },
                        if f.open { " ▾" } else { " ▸" },
                        f.sign..f.sign + 1,
                        &RunStyle {
                            link: Some("splat-fold".into()),
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
                    for child in &block.children {
                        self.block(child, &inner, None);
                    }
                }
                self.set_direction(start, inner.group);
            }
            BlockKind::DefinitionList => {
                let mut marker = marker;
                for item in &block.children {
                    for part in &item.children {
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
                        for child in &part.children {
                            let start = self.paras.len();
                            self.block(child, &inner, marker.take());
                            if bold {
                                for p in &mut self.paras[start..] {
                                    for r in &mut p.runs {
                                        r.style.bold = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            BlockKind::Comment => {}
            BlockKind::List { kind, tight } => {
                let depth = ctx.list_depth;
                for (index, item) in block.children.iter().enumerate() {
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
                            symbol: t.symbol,
                        },
                        (None, ListKind::Bullet(_)) => Marker::Bullet(depth),
                        (None, ListKind::Ordered { start, .. }) => Marker::Number(start + index),
                    };
                    let marker_end = task
                        .as_ref()
                        .map_or(marker_range.end, |t| (t.symbol + 2).min(self.text.len()));
                    let item_marker = (item_marker, marker_range.start..marker_end);
                    let inner = Ctx {
                        list_depth: depth + 1,
                        tight: *tight,
                        ..ctx.clone()
                    };
                    let mut first = Some(item_marker);
                    if item.children.is_empty() {
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
                    for child in &item.children {
                        self.block(child, &inner, first.take());
                    }
                }
            }
            BlockKind::CodeBlock {
                info, literal, content, ..
            } => {
                let lang = info.split_whitespace().next().unwrap_or("").to_string();
                let group = self.next_group();
                let inner = Ctx { group, ..ctx.clone() };
                let body = literal.strip_suffix('\n').unwrap_or(literal);
                if crate::markdown::doc::is_math_fence(info) && self.resolver.math_ready(body, true) {
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
                if let Some(kind) = crate::markdown::doc::diagram_kind(info) {
                    let ready = self.resolver.diagram_ready(literal, kind);
                    if ready {
                        let run = atom(
                            Atom::Diagram {
                                source: literal.clone(),
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
                    vec![self.aligned(
                        body,
                        content.clone(),
                        RunStyle {
                            code: true,
                            ..Default::default()
                        },
                    )]
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
            BlockKind::HtmlBlock { literal } => self.html_block(literal, block.range.clone(), ctx, marker),
            BlockKind::FrontMatter { content } => {
                let keys: Vec<&str> = self.text[content.clone()]
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
            BlockKind::ThematicBreak => {
                let run = atom(Atom::Rule, "\u{00A0}", block.range.clone());
                self.push_para(
                    ParaKind::Rule,
                    ctx,
                    marker,
                    block.range.clone(),
                    vec![run],
                    block.range.start,
                );
            }
            BlockKind::MathBlock { literal } => {
                let run = if self.resolver.math_ready(literal, true) {
                    atom(
                        Atom::Math {
                            tex: literal.clone(),
                            display: true,
                        },
                        &ATTACHMENT.to_string(),
                        block.range.clone(),
                    )
                } else {
                    let shown = format!("$${}$$", literal);
                    Run {
                        text: shown,
                        style: RunStyle {
                            code: true,
                            ..Default::default()
                        },
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
            BlockKind::Table { alignments } => {
                self.table += 1;
                let table = self.table;
                let group = self.next_group();
                let inner = Ctx { group, ..ctx.clone() };
                let columns = alignments.len();
                let start = self.paras.len();
                let mut marker = marker;
                for (row_index, row) in block.children.iter().enumerate() {
                    let header = matches!(row.kind, BlockKind::TableRow { header: true });
                    for (col, &align) in alignments.iter().enumerate() {
                        let kind = ParaKind::TableCell {
                            table: table as u32,
                            row: row_index as u32,
                            col: col as u32,
                            header,
                            align: align.into(),
                            columns: columns as u32,
                            span: 1,
                        };
                        match row.children.get(col) {
                            Some(cell) => {
                                let base = RunStyle {
                                    bold: header,
                                    ..Default::default()
                                };
                                let runs = self.inlines(&cell.inlines, &base);
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
            BlockKind::FootnoteDefinition { name, label, number } => {
                let inner = Ctx {
                    group: self.next_group(),
                    ..ctx.clone()
                };
                let shown = number.map_or_else(|| name.clone(), |n| n.to_string().into());
                let mut first = Some((Marker::Footnote(name.clone(), shown), label.clone()));
                if block.children.is_empty() {
                    self.push_para(
                        ParaKind::FootnoteDef,
                        &inner,
                        first.take(),
                        block.range.clone(),
                        vec![],
                        label.end,
                    );
                }
                for child in &block.children {
                    let start = self.paras.len();
                    self.block(child, &inner, first.take());
                    for p in &mut self.paras[start..] {
                        if p.style.kind == ParaKind::Body {
                            p.style.kind = ParaKind::FootnoteDef;
                        }
                    }
                }
            }
            BlockKind::Item { .. }
            | BlockKind::TableRow { .. }
            | BlockKind::TableCell
            | BlockKind::DefinitionItem
            | BlockKind::DefinitionTerm
            | BlockKind::DefinitionDetails => {}
        }
    }

    fn heading_insert(&self, range: &Range<usize>) -> usize {
        let src = &self.text[range.clone()];
        let hashes = src.bytes().take_while(|&b| b == b'#').count();
        (range.start + hashes + 1).min(range.end)
    }

    fn inlines(&self, inlines: &[Inline], style: &RunStyle) -> Vec<Run> {
        let mut runs = Vec::new();
        for i in inlines {
            self.inline(i, style, &mut runs);
        }
        runs
    }

    fn inline(&self, i: &Inline, style: &RunStyle, runs: &mut Vec<Run>) {
        let nested = |f: &dyn Fn(&mut RunStyle), runs: &mut Vec<Run>| {
            let mut s = style.clone();
            f(&mut s);
            for c in &i.children {
                self.inline(c, &s, runs);
            }
        };
        match &i.kind {
            InlineKind::Text(t) => self.text_runs(t, i.range.clone(), style, runs),
            InlineKind::SoftBreak => runs.push(atom_styled(Atom::Text, " ", i.range.clone(), style)),
            InlineKind::LineBreak => runs.push(atom_styled(
                Atom::Text,
                &LINE_SEPARATOR.to_string(),
                i.range.clone(),
                style,
            )),
            InlineKind::Code(literal) => {
                let src = &self.text[i.range.clone()];
                let ticks = src.bytes().take_while(|&b| b == b'`').count();
                let closing = src[ticks..].bytes().rev().take_while(|&b| b == b'`').count();
                let inner = i.range.start + ticks..i.range.end - closing;
                runs.push(self.aligned(
                    literal,
                    inner,
                    RunStyle {
                        code: true,
                        ..style.clone()
                    },
                ));
            }
            InlineKind::Html(h) => runs.push(self.aligned(
                h,
                i.range.clone(),
                RunStyle {
                    html: true,
                    ..style.clone()
                },
            )),
            InlineKind::Emph => nested(&|s| s.italic = true, runs),
            InlineKind::Strong => nested(&|s| s.bold = true, runs),
            InlineKind::Strikethrough => nested(&|s| s.strike = true, runs),
            InlineKind::Underline => nested(&|s| s.underline = true, runs),
            InlineKind::Highlight => nested(&|s| s.highlight = true, runs),
            InlineKind::Superscript => nested(&|s| s.superscript = true, runs),
            InlineKind::Subscript => nested(&|s| s.subscript = true, runs),
            InlineKind::Spoiler => nested(&|s| s.spoiler = true, runs),
            InlineKind::Kbd => nested(&|s| s.code = true, runs),
            InlineKind::Span => nested(&|_| {}, runs),
            InlineKind::HtmlImage {
                url,
                alt,
                width,
                height,
            } => runs.push(atom_styled(
                Atom::Image(Box::new(ImageAtom {
                    url: url.clone(),
                    alt: alt.clone(),
                    width: *width,
                    height: *height,
                })),
                &ATTACHMENT.to_string(),
                i.range.clone(),
                style,
            )),
            InlineKind::Emoji(emoji) => runs.push(atom_styled(Atom::Text, emoji, i.range.clone(), style)),
            InlineKind::Comment => {}
            InlineKind::WikiLink { target, embed: true } if is_image_name(target) => {
                runs.push(atom_styled(
                    Atom::Image(Box::new(ImageAtom {
                        url: target.clone(),
                        alt: target.clone(),
                        width: None,
                        height: None,
                    })),
                    &ATTACHMENT.to_string(),
                    i.range.clone(),
                    style,
                ));
            }
            InlineKind::WikiLink { target, .. } => {
                let link: Box<str> = format!("splat-wiki:{target}").into();
                nested(&|s| s.link = Some(link.clone()), runs)
            }
            InlineKind::Link { url, .. } => {
                let url = url.clone();
                nested(&|s| s.link = Some(url.clone()), runs)
            }
            InlineKind::Image { url, .. } => {
                let alt = plain_text(&i.children);
                runs.push(atom_styled(
                    Atom::Image(Box::new(ImageAtom {
                        url: url.clone(),
                        alt: alt.into(),
                        width: None,
                        height: None,
                    })),
                    &ATTACHMENT.to_string(),
                    i.range.clone(),
                    style,
                ));
            }
            InlineKind::FootnoteRef { name, ix } => {
                let shown = format!("[{ix}]");
                let mut s = style.clone();
                s.superscript = true;
                s.link = Some(format!("splat-footnote:{name}").into());
                runs.push(atom_styled(
                    Atom::FootnoteRef { name: name.clone() },
                    &shown,
                    i.range.clone(),
                    &s,
                ));
            }
            InlineKind::Math { display, literal } => {
                if self.resolver.math_ready(literal, *display) {
                    runs.push(atom_styled(
                        Atom::Math {
                            tex: literal.clone(),
                            display: *display,
                        },
                        &ATTACHMENT.to_string(),
                        i.range.clone(),
                        style,
                    ));
                } else {
                    let shown = self.text[i.range.clone()].to_string();
                    runs.push(atom_styled(
                        Atom::Text,
                        &shown,
                        i.range.clone(),
                        &RunStyle {
                            code: true,
                            ..style.clone()
                        },
                    ));
                }
            }
        }
    }

    /// Running text, with typographic punctuation if wanted, split where tags, references and citations start.
    fn text_runs(&self, literal: &str, src: Range<usize>, style: &RunStyle, runs: &mut Vec<Run>) {
        let before = self.text[..src.start].chars().next_back();
        let shown = match self.resolver.quotes() {
            Some(quotes) if !style.code && !style.html => {
                let mut open = self.open.get();
                let shown = smarten(literal, before, quotes, &mut open);
                self.open.set(open);
                shown
            }
            _ => literal.to_string(),
        };
        let whole = self.aligned(&shown, src.clone(), style.clone());
        let found = if style.link.is_some() || style.code || style.html {
            Vec::new()
        } else {
            marks(&shown, before, self.resolver.references())
        };
        if found.is_empty() {
            runs.push(whole);
            return;
        }
        let RunMap::Text { chars, end } = &whole.map else {
            runs.push(whole);
            return;
        };
        let char_at = |byte: usize| shown[..byte].chars().count();
        let source_at = |byte: usize| chars.get(&shown, char_at(byte)).unwrap_or(*end);
        let piece = |bytes: Range<usize>, style: RunStyle| -> Run {
            let first = char_at(bytes.start);
            let count = shown[bytes.clone()].chars().count();
            let map = match chars {
                CharMap::Verbatim(start) => CharMap::Verbatim(start + bytes.start),
                CharMap::Offsets(base, offsets) => {
                    let head = offsets.get(first).copied().unwrap_or(0);
                    CharMap::Offsets(
                        base + head as usize,
                        offsets[first..(first + count).min(offsets.len())]
                            .iter()
                            .map(|o| o - head)
                            .collect(),
                    )
                }
            };
            Run {
                text: shown[bytes.clone()].to_string(),
                style,
                map: RunMap::Text {
                    chars: map,
                    end: source_at(bytes.end),
                },
            }
        };
        let mut at = 0;
        for (range, mark) in found {
            if range.start > at {
                runs.push(piece(at..range.start, style.clone()));
            }
            let mut marked = style.clone();
            match &mark {
                Mark::Tag(name) => {
                    marked.tag = true;
                    marked.link = Some(format!("splat-tag:#{name}").into());
                }
                Mark::Reference(target) => {
                    marked.reference = true;
                    marked.link = Some(format!("splat-ref:{target}").into());
                }
                Mark::Citation(_) => marked.citation = true,
            }
            let resolved = match &mark {
                Mark::Citation(keys) => {
                    let names: Option<Vec<String>> = keys.iter().map(|k| self.resolver.citation(k)).collect();
                    names.map(|n| format!("({})", n.join("; ")))
                }
                _ => None,
            };
            match resolved {
                Some(shown_as) => runs.push(atom_styled(
                    Atom::Text,
                    &shown_as,
                    source_at(range.start)..source_at(range.end),
                    &marked,
                )),
                None => runs.push(piece(range.clone(), marked)),
            }
            at = range.end;
        }
        if at < shown.len() {
            runs.push(piece(at..shown.len(), style.clone()));
        }
    }

    /// The paragraphs of an embedded note, framed like a quote; they show the note but edit nothing in it.
    fn embed(&mut self, note: &str, folder: &str, src: Range<usize>, ctx: &Ctx) {
        let doc = crate::markdown::doc::parse(note);
        let inner = layout(note, &doc, &WithoutEmbeds(self.resolver), None);
        let mut groups = std::collections::HashMap::new();
        let frame = self.next_group();
        for mut p in inner.blocks.into_iter().flat_map(|b| b.paras) {
            p.style.quote_depth += ctx.quote_depth + 1;
            p.style.group = match p.style.group {
                0 => frame,
                g => *groups.entry(g).or_insert_with(|| self.next_group()),
            };
            if let Some(Marker::Task { symbol, .. }) = &mut p.style.marker {
                *symbol = usize::MAX;
            }
            for run in &mut p.runs {
                let atom = match std::mem::replace(
                    &mut run.map,
                    RunMap::Text {
                        chars: CharMap::Verbatim(0),
                        end: 0,
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

    fn aligned(&self, literal: &str, src: Range<usize>, style: RunStyle) -> Run {
        let chars = align(literal, self.text, src.clone());
        Run {
            text: literal.to_string(),
            style,
            map: RunMap::Text { chars, end: src.end },
        }
    }
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

/// File names Obsidian embeds as pictures.
pub fn is_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg", ".heic", ".tif", ".tiff", ".avif",
    ]
    .iter()
    .any(|e| lower.ends_with(e))
}

fn marker_text(m: &Marker) -> String {
    match m {
        Marker::Bullet(depth) => format!("{}\t", ["•", "◦", "▪"][*depth as usize % 3]),
        Marker::Number(n) => format!("{n}.\t"),
        Marker::Task { checked, .. } => format!("{}\t", if *checked { "☑" } else { "☐" }),
        Marker::Footnote(_, shown) => format!("{shown}.\t"),
    }
}

fn cell_insert(text: &str, range: &Range<usize>) -> usize {
    let s = &text[range.clone()];
    let lead = s.len() - s.trim_start().len();
    if lead > 0 { range.start + 1 } else { range.start }
}

fn atom(atom: Atom, shown: &str, src: Range<usize>) -> Run {
    Run {
        text: shown.to_string(),
        style: RunStyle::default(),
        map: RunMap::Atom { atom, src },
    }
}

fn atom_styled(atom: Atom, shown: &str, src: Range<usize>, style: &RunStyle) -> Run {
    Run {
        text: shown.to_string(),
        style: style.clone(),
        map: RunMap::Atom { atom, src },
    }
}

fn plain_text(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match &i.kind {
            InlineKind::Text(t) | InlineKind::Code(t) => s.push_str(t),
            InlineKind::SoftBreak | InlineKind::LineBreak => s.push(' '),
            _ => s.push_str(&plain_text(&i.children)),
        }
    }
    s
}

pub fn decode_entity(s: &str) -> Option<(char, usize)> {
    let end = s[..s.floor_char_boundary(40)].find(';')?;
    let name = &s[1..end];
    let c = if let Some(num) = name.strip_prefix('#') {
        let n = if let Some(hex) = num.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            num.parse().ok()?
        };
        char::from_u32(n)?
    } else {
        match name {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "nbsp" => '\u{00A0}',
            "copy" => '©',
            "reg" => '®',
            "hellip" => '…',
            "mdash" => '—',
            "ndash" => '–',
            "euro" => '€',
            _ => return None,
        }
    };
    Some((c, end + 1))
}

/// Source offset of every char of `literal` inside `text[src]`, skipping escapes, entities and container prefixes.
pub fn align(literal: &str, text: &str, src: Range<usize>) -> CharMap {
    let source = &text[src.clone()];
    if source == literal {
        return CharMap::Verbatim(src.start);
    }
    let mut out = Vec::with_capacity(literal.chars().count());
    let mut i = 0;
    for c in literal.chars() {
        loop {
            if i >= source.len() {
                out.push(src.end);
                break;
            }
            let rest = &source[i..];
            let sc = rest.chars().next().unwrap();
            if sc == c {
                out.push(src.start + i);
                i += sc.len_utf8();
                break;
            }
            if sc == '\\'
                && let Some(next) = rest[1..].chars().next()
                && next == c
            {
                out.push(src.start + i);
                i += 1 + next.len_utf8();
                break;
            }
            if let Some(len) = typographic_source(c, rest) {
                out.push(src.start + i);
                i += len;
                break;
            }
            if sc == '&'
                && let Some((decoded, len)) = decode_entity(rest)
                && decoded == c
            {
                out.push(src.start + i);
                i += len;
                break;
            }
            if c == ' ' && sc == '\n' {
                out.push(src.start + i);
                i += 1;
                break;
            }
            i += sc.len_utf8();
        }
    }
    let base = out.first().copied().unwrap_or(src.start);
    CharMap::Offsets(base, out.iter().map(|&o| (o - base) as u32).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::doc::parse;

    fn texts(l: &Layout) -> Vec<String> {
        l.blocks
            .iter()
            .flat_map(|b| b.paras.iter().map(|p| p.runs.iter().map(|r| r.text.as_str()).collect()))
            .collect()
    }

    #[test]
    fn blocks_outside_a_partial_parse_are_reused_unless_they_held_the_caret() {
        let old = "Eins\n\nZwei\n\nDrei\n\nVier\n\nFünf\n\nSechs\n\nSieben\n";
        let mut doc = parse(old);
        let splice = crate::markdown::incremental::reparse(&mut doc, old, &old.replace("Vier", "Viertel")).unwrap();
        assert_eq!((splice.new.clone(), splice.delta), (2..5, 3));
        let reuse = reusable(&splice, &doc, None);
        let all: Vec<_> = (0..8).map(&reuse).collect();
        assert_eq!(all, [Some(0), Some(0), None, None, None, Some(3), Some(3), None]);
        let caret_in_sechs = reusable(&splice, &doc, Some(old.find("Sechs").unwrap()));
        assert_eq!((caret_in_sechs(5), caret_in_sechs(6)), (None, Some(3)));
        assert_eq!(reusable(&splice, &doc, Some(0))(0), None);
    }

    #[test]
    fn right_to_left_text_mirrors_its_quote_callout_and_table() {
        let t = "פסקה\n\n- [x] משימה\n\n> [!NOTE]\n> הערה\n\n> Quote\n\n| שם | ערך |\n|---|---|\n| 1 | 2 |\n\n```\n# הערה\n```\n";
        let d = parse(t);
        let l = layout(t, &d, &NoImages, None);
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
            let l = layout(t, &d, &NoImages, None);
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
            let l = layout(t, &d, &NoImages, None);
            let title = &l.blocks[0].paras[0];
            assert_eq!(title.style.kind, ParaKind::AlertTitle, "{t}");
            assert_eq!((title.style.rtl, title.style.mirrored), (Some(true), true), "{t}");
        }
    }

    #[test]
    fn paragraphs_lists_and_tables() {
        let t = "# Titel\n\nText **fett**\n\n- a\n- [x] b\n\n| h | i |\n|---|---|\n| 1 |   |\n";
        let d = parse(t);
        let l = layout(t, &d, &NoImages, None);
        assert_eq!(texts(&l), vec!["Titel", "Text fett", "•\ta", "☑\tb", "h", "i", "1", ""]);
        let item = &l.blocks[2].paras[1];
        assert!(matches!(item.style.marker, Some(Marker::Task { checked: true, .. })));
        let empty_cell = &l.blocks[3].paras[3];
        assert_eq!(&t[empty_cell.insert_at..empty_cell.insert_at + 1], " ");
    }

    #[test]
    fn escapes_and_entities_align_to_source() {
        let t = "a \\* b &amp; c\n";
        let d = parse(t);
        let l = layout(t, &d, &NoImages, None);
        let run = &l.blocks[0].paras[0].runs[0];
        let RunMap::Text { chars, .. } = &run.map else { panic!() };
        let chars: Vec<usize> = (0..run.text.chars().count())
            .map(|k| chars.get(&run.text, k).unwrap())
            .collect();
        let lit: Vec<char> = l.blocks[0].paras[0].runs[0].text.chars().collect();
        assert_eq!(lit[2], '*');
        assert_eq!(&t[chars[2]..chars[2] + 2], "\\*");
        assert_eq!(lit[6], '&');
        assert_eq!(&t[chars[6]..chars[6] + 5], "&amp;");
        assert_eq!(chars[8], t.find(" c").unwrap() + 1);
        assert_eq!(decode_entity(&format!("&{}😀;", "a".repeat(38))), None);
    }

    #[test]
    fn spaces_at_the_caret_stay_visible_at_a_paragraph_end() {
        let t = "Wort **fett** \n\nNächster\n";
        let d = parse(t);
        assert_eq!(texts(&layout(t, &d, &NoImages, None))[0], "Wort fett");
        assert_eq!(texts(&layout(t, &d, &NoImages, Some(14)))[0], "Wort fett ");
        assert_eq!(texts(&layout(t, &d, &NoImages, Some(3)))[0], "Wort fett");
    }

    #[test]
    fn every_second_extra_blank_line_is_an_empty_paragraph() {
        let spots = |t: &str| {
            let d = parse(t);
            let ranges: Vec<Range<usize>> = d.blocks.iter().map(|b| b.range.clone()).collect();
            empty_line_spots(t, &ranges)
        };
        assert_eq!(spots("A\n\nB\n"), Vec::<usize>::new());
        assert_eq!(spots("A\n\n\nB\n"), Vec::<usize>::new());
        assert_eq!(spots("A\n\n\n\nB\n"), vec![3]);
        assert_eq!(spots("A\n\n\n\n\n\nB\n"), vec![3, 5]);
        assert_eq!(spots("A\n\n\n"), vec![3]);
        assert_eq!(spots("\n\nA\n"), vec![0]);
        let t = "A\n\n\n\nB\n";
        let l = layout(t, &parse(t), &NoImages, None);
        assert_eq!(texts(&l), vec!["A", "", "B"]);
    }

    #[test]
    fn extended_syntax_shows_as_formatting() {
        let t = "Begriff\n\n: Erklärung\n\n> [!tip]- Zu\n> versteckt\n\n> [!tip]+ Auf\n> sichtbar\n\n\
                 A ==b== :rocket: ||c|| %%x%% [[Seite|T]] ![[bild.png]]\n";
        let d = parse(t);
        let l = layout(t, &d, &NoImages, None);
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
        assert!(paras[0].runs.iter().all(|r| r.style.bold));
        assert_eq!(paras[1].style.list_depth, 1);
        let fold = paras[2].runs.last().unwrap();
        let RunMap::Atom {
            atom: Atom::Fold { open: false },
            src,
        } = &fold.map
        else {
            panic!("{fold:?}")
        };
        assert_eq!(&t[src.clone()], "-");
        let runs = &paras[5].runs;
        assert!(runs.iter().any(|r| r.text == "b" && r.style.highlight));
        assert!(runs.iter().any(|r| r.text == "c" && r.style.spoiler));
        assert!(
            runs.iter()
                .any(|r| r.text == "T" && r.style.link.as_deref() == Some("splat-wiki:Seite"))
        );
        assert!(
            runs.iter()
                .any(|r| matches!(&r.map, RunMap::Atom { atom: Atom::Image(i), .. } if &*i.url == "bild.png"))
        );
    }

    struct Surroundings;

    impl Resolver for Surroundings {
        fn math_ready(&self, _: &str, _: bool) -> bool {
            false
        }
        fn diagram_ready(&self, _: &str, _: DiagramKind) -> bool {
            false
        }
        fn quotes(&self) -> Option<Quotes> {
            Some(Quotes {
                open: '„',
                close: '“',
                open_single: '‚',
                close_single: '‘',
            })
        }
        fn references(&self) -> bool {
            true
        }
        fn citation(&self, key: &str) -> Option<String> {
            (key == "doe").then(|| "Doe 2020".to_string())
        }
        fn embedded_note(&self, target: &str) -> Option<(String, String)> {
            (target == "Notiz").then(|| {
                (
                    "# Eingebettet\n\n- [x] erledigt\n\n![[Notiz]]\n".to_string(),
                    "/n".to_string(),
                )
            })
        }
    }

    #[test]
    fn quotes_without_spaces_pair_across_line_breaks_and_formatting() {
        let t = "彼は\"今日は\n晴れ\"です\n\n\"**強調**\"と\"言\"\n";
        let d = parse(t);
        let l = layout(t, &d, &Surroundings, None);
        let shown: Vec<String> = l.blocks[..2]
            .iter()
            .map(|b| b.paras[0].runs.iter().map(|r| r.text.as_str()).collect())
            .collect();
        assert!(
            shown[0].starts_with("彼は„今日は") && shown[0].ends_with("晴れ“です"),
            "{shown:?}"
        );
        assert_eq!(shown[1], "„強調“と„言“");
    }

    #[test]
    fn running_text_marks_and_embedded_notes() {
        let t = "Er sagte \"Ja\" -- #idee, #12 von @anna [@doe] [@fehlt]\n\n![[Notiz]]\n";
        let d = parse(t);
        let l = layout(t, &d, &Surroundings, None);
        let runs = &l.blocks[0].paras[0].runs;
        let shown: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(shown, "Er sagte „Ja“ – #idee, #12 von @anna (Doe 2020) [@fehlt]");
        let quote = runs.iter().find(|r| r.text.starts_with("Er sagte")).unwrap();
        let RunMap::Text { chars, .. } = &quote.map else {
            panic!()
        };
        assert_eq!(&t[chars.get(&quote.text, 9).unwrap()..][..1], "\"");
        let tag = runs.iter().find(|r| r.text == "#idee").unwrap();
        assert!(tag.style.tag && tag.style.link.as_deref() == Some("splat-tag:#idee"));
        assert!(
            runs.iter()
                .any(|r| r.text == "#12" && r.style.link.as_deref() == Some("splat-ref:#12"))
        );
        assert!(runs.iter().any(|r| r.text == "@anna" && r.style.reference));
        let cite = runs.iter().find(|r| r.text == "(Doe 2020)").unwrap();
        assert!(matches!(&cite.map, RunMap::Atom { src, .. } if &t[src.clone()] == "[@doe]"));
        assert!(runs.iter().any(|r| r.text == "[@fehlt]" && r.style.citation));
        let embedded: Vec<&Para> = l.blocks[1].paras.iter().collect();
        assert_eq!(embedded.len(), 3, "{embedded:?}");
        assert!(
            embedded
                .iter()
                .all(|p| p.style.quote_depth == 1 && &t[p.src.clone()] == "![[Notiz]]")
        );
        assert!(matches!(
            embedded[1].style.marker,
            Some(Marker::Task { symbol: usize::MAX, .. })
        ));
        assert!(
            embedded[2]
                .runs
                .iter()
                .any(|r| r.style.link.as_deref() == Some("splat-wiki:Notiz"))
        );
    }

    #[test]
    fn embedded_html_renders_and_points_into_its_source() {
        let t = "<p align=\"center\">\n  <img src=\"logo.png\" width=\"120\">\n  <br><b>Splat</b> &amp; <a href=\"https://x.de\">Web</a>\n</p>\n\n\
                 <details>\n<summary>Mehr</summary>\n\nVersteckt\n\n</details>\n\n<details open>\n<summary>Offen</summary>\n\nSichtbar\n\n</details>\n\n\
                 <table><tr><th colspan=2>Kopf</th></tr><tr><td>a</td><td>b</td></tr></table>\n\n\
                 Text mit <kbd>⌘</kbd> und <img src=\"x.png\" height=\"10\"> <!-- weg --> <b>fett</b>\n";
        let d = parse(t);
        let l = layout(t, &d, &NoImages, None);
        let o = ATTACHMENT;
        let shown = texts(&l);
        assert_eq!(
            shown,
            vec![
                format!("{o}\u{2028}Splat & Web"),
                "▸ Mehr".to_string(),
                "▾ Offen".into(),
                "Sichtbar".into(),
                "Kopf".into(),
                "a".into(),
                "b".into(),
                format!("Text mit ⌘ und {o}  fett"),
            ]
        );
        let paras: Vec<&Para> = l.blocks.iter().flat_map(|b| &b.paras).collect();
        assert_eq!(paras[0].style.align, AlignKey::Center);
        assert!(paras[0].style.html);
        let RunMap::Atom {
            atom: Atom::Image(img),
            src,
        } = &paras[0].runs[0].map
        else {
            panic!()
        };
        assert_eq!(
            (img.width, &t[src.clone()]),
            (Some(120), "<img src=\"logo.png\" width=\"120\">")
        );
        let web = paras[0].runs.iter().find(|r| r.text == "Web").unwrap();
        assert_eq!(web.style.link.as_deref(), Some("https://x.de"));
        let RunMap::Atom {
            atom: Atom::Details { open: false },
            src,
        } = &paras[1].runs[0].map
        else {
            panic!()
        };
        assert_eq!(&t[src.clone()], "<details>");
        assert!(matches!(
            paras[4].style.kind,
            ParaKind::TableCell {
                span: 2,
                header: true,
                ..
            }
        ));
        let last = paras[7];
        assert!(last.runs.iter().any(|r| r.text == "⌘" && r.style.code));
        assert!(
            last.runs
                .iter()
                .any(|r| matches!(&r.map, RunMap::Atom { atom: Atom::Image(i), .. } if i.height == Some(10)))
        );
        assert!(last.runs.iter().any(|r| r.text == "fett" && r.style.bold));
    }

    #[test]
    fn embedded_html_collapses_only_ascii_whitespace() {
        let t = "<p><img src=\"a.png\">&nbsp;&nbsp;&nbsp;<img src=\"b.png\"></p>\n\n<p>a  \n\t b&#8195;c</p>\n";
        let d = parse(t);
        let l = layout(t, &d, &NoImages, None);
        let o = ATTACHMENT;
        assert_eq!(
            texts(&l),
            vec![format!("{o}\u{a0}\u{a0}\u{a0}{o}"), "a b\u{2003}c".to_string()]
        );
    }

    #[test]
    fn html_blocks_become_headings_rules_code_quotes_lists_and_tables() {
        let t = "<div>\n<h3>Titel</h3>\n<hr>\n<pre>\nlet x = 1;\n</pre>\n<blockquote>Zitat</blockquote>\n\
                 <ol start=\"3\"><li>drei</li><li>vier</li></ol>\n<ul><li>Punkt</li></ul>\n\
                 <dl><dt>Begriff</dt><dd>Erklärung</dd></dl>\n\
                 <picture><source srcset=\"a.webp\"><img src=\"a.png\" alt=\"Bild\"></picture>\n\
                 <img src=\"javascript:x\" alt=\"Ersatz\">\n\
                 <table><thead><tr><th>K</th><th>L</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table>\n\
                 </div>\n";
        let d = parse(t);
        let l = layout(t, &d, &NoImages, None);
        let paras: Vec<&Para> = l.blocks.iter().flat_map(|b| &b.paras).collect();
        let text = |p: &Para| p.runs.iter().map(|r| r.text.as_str()).collect::<String>();
        let find = |s: &str| {
            *paras
                .iter()
                .find(|p| text(p).contains(s))
                .unwrap_or_else(|| panic!("{s}"))
        };
        assert_eq!(find("Titel").style.kind, ParaKind::Heading(3));
        assert!(paras.iter().any(|p| p.style.kind == ParaKind::Rule));
        assert!(matches!(find("let x = 1;").style.kind, ParaKind::Code { .. }));
        assert_eq!(find("Zitat").style.quote_depth, 1);
        assert_eq!(find("drei").style.marker, Some(Marker::Number(3)));
        assert_eq!(find("vier").style.marker, Some(Marker::Number(4)));
        assert_eq!(find("Punkt").style.marker, Some(Marker::Bullet(0)));
        assert!(find("Begriff").runs.iter().all(|r| r.style.bold));
        assert_eq!(find("Erklärung").style.list_depth, 1);
        assert!(find("Ersatz").runs.iter().any(|r| r.text == "Ersatz"));
        let images: Vec<String> = paras
            .iter()
            .flat_map(|p| &p.runs)
            .filter_map(|r| match &r.map {
                RunMap::Atom {
                    atom: Atom::Image(i), ..
                } => Some(i.url.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(images, ["a.png"]);
        let cells: Vec<(&str, bool)> = paras
            .iter()
            .filter_map(|p| match p.style.kind {
                ParaKind::TableCell { header, .. } => Some((p.runs.first().map_or("", |r| r.text.as_str()), header)),
                _ => None,
            })
            .collect();
        assert_eq!(cells, [("K", true), ("L", true), ("1", false), ("", false)]);
    }

    #[test]
    fn block_keys_change_only_for_edited_blocks() {
        let a = "Eins\n\nZwei\n\nDrei\n";
        let b = "Eins\n\nZwei!\n\nDrei\n";
        let la = layout(a, &parse(a), &NoImages, None);
        let lb = layout(b, &parse(b), &NoImages, None);
        assert_eq!(la.blocks[0].key, lb.blocks[0].key);
        assert_ne!(la.blocks[1].key, lb.blocks[1].key);
        assert_eq!(la.blocks[2].key, lb.blocks[2].key);
    }
}
