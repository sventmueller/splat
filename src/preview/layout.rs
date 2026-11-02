use std::hash::{Hash, Hasher};
use std::ops::Range;

use crate::markdown::doc::{
    AlertKind, Align, Block, BlockKind, DiagramKind, Doc, Fold, Inline, InlineKind, ListKind, Piece, is_image_name,
};
use crate::markdown::incremental::Splice;
use crate::markdown::style::{Baseline, TextStyle};
use crate::markdown::text::{count32, decode_entity, line_start, right_to_left, shifted};
use crate::markdown::typography::{Mark, Open, Quotes, before_text, marks, smarten, typographic_source};

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
        align: Align,
        columns: u32,
        /// Columns the cell spans, more than one only in HTML tables.
        span: u32,
    },
    FootnoteDef,
    AlertTitle,
    /// Front matter, shown as one line naming its keys.
    FrontMatter,
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
    pub align: Align,
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
        /// Where the `x` or the space stands in the source; an embedded note's checkboxes have no place in this text.
        symbol: Option<usize>,
    },
    /// The footnote's name, which references point to, and the label shown.
    Footnote(Box<str>, Box<str>),
}

/// Where a click on a run leads.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Link {
    /// An address the document names.
    Address(Box<str>),
    /// A note `[[named]]`, wherever it lies.
    WikiPage(Box<str>),
    Footnote(Box<str>),
    /// An Obsidian `#tag`, with its `#`.
    Tag(Box<str>),
    /// `@name`, `#123` or `!123` on the code host of the document's repository.
    Reference(Box<str>),
    /// The chevron of a callout or `<details>`.
    Fold,
    /// A task's checkbox.
    Task,
}

impl Link {
    /// The link as a text attribute, the form AppKit hands back on a click. An address of the document that looks like
    /// one of Splat's own forms is marked, so a document cannot make a click toggle or fold anything.
    pub fn attribute(&self) -> String {
        match self {
            Link::Address(address) if address.starts_with("splat-") => format!("splat-address:{address}"),
            Link::Address(address) => address.to_string(),
            Link::WikiPage(name) => format!("splat-wiki:{name}"),
            Link::Footnote(name) => format!("splat-footnote:{name}"),
            Link::Tag(tag) => format!("splat-tag:{tag}"),
            Link::Reference(target) => format!("splat-ref:{target}"),
            Link::Fold => "splat-fold".to_string(),
            Link::Task => "splat-task".to_string(),
        }
    }

    pub fn from_attribute(attribute: &str) -> Link {
        let named = |prefix: &str| attribute.strip_prefix(prefix).map(Into::into);
        match attribute {
            "splat-fold" => Link::Fold,
            "splat-task" => Link::Task,
            _ => named("splat-address:")
                .map(Link::Address)
                .or_else(|| named("splat-wiki:").map(Link::WikiPage))
                .or_else(|| named("splat-footnote:").map(Link::Footnote))
                .or_else(|| named("splat-tag:").map(Link::Tag))
                .or_else(|| named("splat-ref:").map(Link::Reference))
                .unwrap_or_else(|| Link::Address(attribute.into())),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct RunStyle {
    pub text: TextStyle,
    /// Raw HTML shown as it stands.
    pub html: bool,
    /// A Pandoc citation.
    pub citation: bool,
    pub link: Option<Link>,
}

impl RunStyle {
    fn with_text(&self, change: impl FnOnce(TextStyle) -> TextStyle) -> RunStyle {
        RunStyle {
            text: change(self.text),
            ..self.clone()
        }
    }

    fn code(&self) -> RunStyle {
        self.with_text(|text| TextStyle { code: true, ..text })
    }
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

/// The list or footnote marker the next paragraph starts with.
type Lead = Option<(Marker, Range<usize>)>;

struct Builder<'a> {
    text: &'a str,
    resolver: &'a dyn Resolver,
    group: usize,
    table: usize,
    paras: Vec<Para>,
    /// Quotes the runs of the next paragraph left open so far.
    open: std::cell::Cell<Open>,
    /// Notes embedded so far: each is parsed and laid out for its place, and a file can ask for one a hundred thousand times.
    embeds: usize,
}

/// Further `![[note]]` show as links.
const MAX_EMBEDS: usize = 64;

#[derive(Clone, Default)]
struct Ctx {
    quote_depth: u8,
    list_depth: u8,
    group: usize,
    tight: bool,
    alert: Option<AlertKind>,
    align: Align,
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
        if splice.new.contains(&i) {
            return None;
        }
        let r = &doc.blocks.get(i)?.range;
        let shift = if r.start >= splice.behind { splice.delta } else { 0 };
        let (start, end) = (shifted(r.start, -shift), shifted(r.end, -shift));
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
        embeds: 0,
    };
    // Footnote definitions comrak moved behind the text break the source order of the blocks.
    let mut old_starts: Vec<(usize, usize)> = old.iter().enumerate().map(|(k, o)| (o.src.start, k)).collect();
    old_starts.sort_unstable();
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
            let at = shifted(block.range.start, -shift);
            // An inline note's definition can start where the block with its reference starts, which may be hidden now.
            let definition = matches!(block.kind, BlockKind::FootnoteDefinition { .. });
            let found = old_starts[old_starts.partition_point(|s| s.0 < at)..]
                .iter()
                .take_while(|s| s.0 == at)
                .map(|s| s.1)
                .find(|&k| old[k].as_ref().is_some_and(|o| shows_definition(o) == definition))
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
        show_spaces_at_the_caret(text, &mut parts, c);
    }
    let mut parts = with_empty_lines(text, doc, parts, caret);
    mark_paragraphs_behind_tables(&mut parts);
    Layout {
        blocks: parts.into_iter().map(Part::finished).collect(),
    }
}

fn shows_definition(block: &TopBlock) -> bool {
    block
        .paras
        .first()
        .is_some_and(|p| matches!(p.style.marker, Some(Marker::Footnote(..))))
}

/// Only a paragraph ending on the caret's line can show trailing spaces.
fn show_spaces_at_the_caret(text: &str, parts: &mut [Part], c: usize) {
    let line = line_start(text, c.min(text.len()))..text[c.min(text.len())..].find('\n').map_or(text.len(), |i| c + i);
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
fn with_empty_lines(text: &str, doc: &Doc, parts: Vec<Part>, caret: Option<usize>) -> Vec<Part> {
    let ranges: Vec<Range<usize>> = parts.iter().map(|p| p.src.clone()).collect();
    let spots = empty_line_spots(text, &ranges);
    let virtual_at = caret.filter(|&c| {
        // Footnote definitions comrak moved behind the text break the source order of the blocks.
        let inside = doc.blocks.iter().any(|b| b.range.start <= c && c <= b.range.end);
        let line_begin = line_start(text, c);
        let line_end = text[c..].find('\n').map_or(text.len(), |i| c + i);
        !inside && text[line_begin..line_end].trim().is_empty() && spots.binary_search(&line_begin).is_err()
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
    fn empty(at: usize) -> Part {
        Part {
            paras: vec![empty_para(at)],
            src: at..at,
            kept: None,
            context: 0,
        }
    }

    fn finished(self) -> TopBlock {
        let Part {
            mut paras,
            src,
            kept,
            context,
        } = self;
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
    let at = |o: usize| shifted(o, delta);
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
        marker: Lead,
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

    fn block(&mut self, block: &Block, ctx: &Ctx, marker: Lead) {
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
        if let Some(target) = only_embed(block.inlines()).filter(|_| self.embeds < MAX_EMBEDS)
            && let Some((note, folder)) = self.resolver.embedded_note(target)
        {
            self.embeds += 1;
            self.embed(&note, &folder, block.range.clone(), ctx);
            return;
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
        title_src: &Range<usize>,
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
                .map_or(marker_range.end, |t| (t.symbol + 2).min(self.text.len()));
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

    fn code_block(
        &mut self,
        block: &Block,
        ctx: &Ctx,
        marker: Lead,
        info: &str,
        literal: &str,
        content: &Range<usize>,
    ) {
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

    fn front_matter(&mut self, block: &Block, ctx: &Ctx, marker: Lead, content: &Range<usize>) {
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

    fn footnote_definition(&mut self, block: &Block, ctx: &Ctx, name: &str, label: &Range<usize>, number: Option<u32>) {
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
        let nested = |s: RunStyle, runs: &mut Vec<Run>| {
            for c in &i.children {
                self.inline(c, &s, runs);
            }
        };
        let at = i.range.clone();
        match i.piece() {
            Piece::Text(t) => self.text_runs(t, at, style, runs),
            Piece::SoftBreak => runs.push(atom_styled(Atom::Text, " ", at, style)),
            Piece::LineBreak => runs.push(atom_styled(Atom::Text, &LINE_SEPARATOR.to_string(), at, style)),
            Piece::Code(literal) => runs.push(self.aligned(literal, i.code_content(self.text), style.code())),
            Piece::Html(h) => runs.push(self.aligned(
                h,
                at,
                RunStyle {
                    html: true,
                    ..style.clone()
                },
            )),
            Piece::Styled => nested(style.with_text(|text| text.inside(&i.kind)), runs),
            Piece::Picture(picture) => runs.push(atom_styled(
                Atom::Image(Box::new(ImageAtom {
                    url: picture.url.into(),
                    alt: picture.alt.as_ref().into(),
                    width: picture.width,
                    height: picture.height,
                })),
                &ATTACHMENT.to_string(),
                at,
                style,
            )),
            Piece::Emoji(emoji) => runs.push(atom_styled(Atom::Text, emoji, at, style)),
            Piece::Hidden => {}
            Piece::WikiPage(target) => nested(
                RunStyle {
                    link: Some(Link::WikiPage(target.into())),
                    ..style.clone()
                },
                runs,
            ),
            Piece::Link { url, .. } => nested(
                RunStyle {
                    link: Some(Link::Address(url.into())),
                    ..style.clone()
                },
                runs,
            ),
            Piece::FootnoteRef { name, ix } => {
                let mut s = style.with_text(|text| TextStyle {
                    baseline: Baseline::Super,
                    ..text
                });
                s.link = Some(Link::Footnote(name.into()));
                let atom = Atom::FootnoteRef { name: name.into() };
                runs.push(atom_styled(atom, &format!("[{ix}]"), at, &s));
            }
            Piece::Math { literal, display } => {
                if self.resolver.math_ready(literal, display) {
                    let atom = Atom::Math {
                        tex: literal.into(),
                        display,
                    };
                    runs.push(atom_styled(atom, &ATTACHMENT.to_string(), at, style));
                } else {
                    let shown = self.text[at.clone()].to_string();
                    runs.push(atom_styled(Atom::Text, &shown, at, &style.code()));
                }
            }
        }
    }

    /// Running text, with typographic punctuation if wanted, split where tags, references and citations start.
    fn text_runs(&self, literal: &str, src: Range<usize>, style: &RunStyle, runs: &mut Vec<Run>) {
        if literal.is_empty() {
            return;
        }
        let before = before_text(self.text, src.start);
        let shown = match self.resolver.quotes() {
            Some(quotes) if !style.text.code && !style.html => {
                let mut open = self.open.get();
                let shown = smarten(literal, before, quotes, &mut open);
                self.open.set(open);
                shown
            }
            _ => literal.to_string(),
        };
        let whole = self.aligned(&shown, src.clone(), style.clone());
        let found = if style.link.is_some() || style.text.code || style.html {
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
        self.marked_runs(&shown, chars, *end, found, style, runs);
    }

    /// `shown`, mapped to the source by `chars`, as runs of its own for what `found` marks in it and for what lies between.
    fn marked_runs(
        &self,
        shown: &str,
        chars: &CharMap,
        end: usize,
        found: Vec<(Range<usize>, Mark)>,
        style: &RunStyle,
        runs: &mut Vec<Run>,
    ) {
        // Marks come in text order, and counting from the start each time would take quadratic time in a long paragraph.
        let counted = std::cell::Cell::new((0, 0));
        let char_at = |byte: usize| {
            let (from, before) = Some(counted.get()).filter(|(from, _)| *from <= byte).unwrap_or((0, 0));
            let at = before + shown[from..byte].chars().count();
            counted.set((byte, at));
            at
        };
        let source_at = |byte: usize| match chars {
            CharMap::Verbatim(start) if byte < shown.len() => start + byte,
            CharMap::Verbatim(_) => end,
            CharMap::Offsets(..) => chars.get(shown, char_at(byte)).unwrap_or(end),
        };
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
                Mark::Tag(name) => marked.link = Some(Link::Tag(format!("#{name}").into())),
                Mark::Reference(target) => marked.link = Some(Link::Reference(target.as_str().into())),
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
                *symbol = None;
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
        match align(literal, self.text, src.clone()) {
            Some(chars) => Run {
                text: literal.to_string(),
                style,
                map: RunMap::Text { chars, end: src.end },
            },
            None => atom_styled(Atom::Text, literal, src, &style),
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

/// Source offset of every char of `literal` inside `text[src]`, past what only the source has: escapes, entities and
/// container prefixes. `None` if the source does not spell the literal, as a position would then be a guess, and an
/// edit made from it would land in the wrong place.
pub fn align(literal: &str, text: &str, src: Range<usize>) -> Option<CharMap> {
    let source = &text[src.clone()];
    if source == literal {
        return Some(CharMap::Verbatim(src.start));
    }
    let mut out = Vec::with_capacity(literal.chars().count());
    let mut i = 0;
    let mut shown = literal;
    // Between the start of a source line and its text, where the prefix of a quote stands.
    let mut in_prefix = src.start == 0 || text.as_bytes()[src.start - 1] == b'\n';
    while let Some(c) = shown.chars().next() {
        let mut used = c.len_utf8();
        loop {
            let at = src.start + i;
            let rest = &source[i..];
            let Some(sc) = rest.chars().next() else {
                if !c.is_whitespace() {
                    return None;
                }
                out.push(src.end);
                break;
            };
            // Code spans and code blocks keep a reference as it stands; everywhere else the literal has what it means.
            if sc == '&'
                && let Some((decoded, len)) = decode_entity(rest)
                && !shown.starts_with(&rest[..len])
            {
                let same = shown.starts_with(&*decoded);
                if same || typographic_source(c, &decoded).is_some() {
                    let count = if same { decoded.chars().count() } else { 1 };
                    out.extend(std::iter::repeat_n(at, count));
                    used = if same { decoded.len() } else { used };
                    i += len;
                    in_prefix = false;
                    break;
                }
            }
            if sc == c || (c == ' ' && sc == '\n') {
                out.push(at);
                i += sc.len_utf8();
                in_prefix = sc == '\n';
                break;
            }
            let escaped = rest
                .strip_prefix('\\')
                .and_then(|r| r.chars().next())
                .filter(char::is_ascii_punctuation);
            if escaped == Some(c) {
                out.push(at);
                i += 1 + c.len_utf8();
                in_prefix = false;
                break;
            }
            if let Some(len) = typographic_source(c, rest) {
                out.push(at);
                i += len;
                in_prefix = false;
                break;
            }
            if sc == '\n' {
                in_prefix = true;
                i += 1;
            } else if sc.is_whitespace() || escaped.is_some() || (in_prefix && sc == '>') {
                i += sc.len_utf8();
            } else if c.is_whitespace() {
                // The indentation of an HTML block's first line stands before its range.
                out.push(at);
                break;
            } else {
                return None;
            }
        }
        shown = &shown[used..];
    }
    let base = out.first().copied().unwrap_or(src.start);
    Some(CharMap::Offsets(base, out.iter().map(|&o| count32(o - base)).collect()))
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
    fn marks_in_a_mapped_text_find_their_place_in_the_source() {
        struct Marks;
        impl Resolver for Marks {
            fn math_ready(&self, _: &str, _: bool) -> bool {
                false
            }
            fn diagram_ready(&self, _: &str, _: DiagramKind) -> bool {
                false
            }
        }
        let text = "ä \\* #eins und ö #zwei und #drei";
        let doc = crate::markdown::doc::parse(text);
        let laid = layout(text, &doc, &Marks, None);
        let tags: Vec<&str> = laid.blocks[0].paras[0]
            .runs
            .iter()
            .filter(|r| matches!(r.style.link, Some(Link::Tag(_))))
            .map(|r| match &r.map {
                RunMap::Text { chars, end } => &text[chars.get(&r.text, 0).unwrap()..*end],
                RunMap::Atom { .. } => "",
            })
            .collect();
        assert_eq!(tags, ["#eins", "#zwei", "#drei"]);
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
    fn every_named_entity_finds_its_place_in_the_source() {
        let t = "Caf&eacute; &NotEqualTilde; und &rarr; ende\n";
        let (chars, shown) = map_of(t);
        assert_eq!(shown, "Café ≂\u{338} und → ende");
        let at = |c: char| chars[shown.chars().position(|s| s == c).unwrap()];
        assert_eq!(&t[at('é')..at('é') + 8], "&eacute;");
        assert_eq!(chars[5], chars[6], "one reference, two characters");
        assert_eq!(&t[at('→')..at('→') + 6], "&rarr;");
        assert_eq!(&t[at('n')..at('n') + 2], "nd");
    }

    #[test]
    fn a_literal_the_source_does_not_spell_is_no_map() {
        assert!(align("xyz", "abc", 0..3).is_none());
        assert!(align("Café", "Caf&unbekannt;", 0..14).is_none());
        assert!(align("ab cd", "ab", 0..2).is_none());
        // Whitespace is what both sides may have without the other.
        assert!(align("  a\n", "a", 0..1).is_some());
        assert!(align("a  b", "a\n> b", 0..5).is_some());
        // Inside code a reference stays as it stands.
        let code = "&amp;\n> x";
        let Some(CharMap::Offsets(base, offsets)) = align("&amp;\nx", code, 0..code.len()) else {
            panic!()
        };
        assert_eq!(base, 0);
        assert_eq!(offsets.last().map(|&o| o as usize), code.find('x'));
    }

    #[test]
    fn text_without_a_place_in_the_source_edits_as_a_whole() {
        let src = "abc\n";
        let b = Builder {
            text: src,
            resolver: &NoImages,
            group: 0,
            table: 0,
            paras: Vec::new(),
            open: Default::default(),
            embeds: 0,
        };
        let run = b.aligned("xyz", 0..3, RunStyle::default());
        assert_eq!(
            run.map,
            RunMap::Atom {
                atom: Atom::Text,
                src: 0..3
            }
        );
    }

    #[test]
    fn a_quoted_entity_turns_typographic_in_its_place() {
        struct German;
        impl Resolver for German {
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
        }
        let t = "a &quot;b&quot; \\\"c\\\" d\n";
        let l = layout(t, &parse(t), &German, None);
        let run = &l.blocks[0].paras[0].runs[0];
        assert_eq!(run.text, "a „b“ „c“ d");
        assert!(matches!(run.map, RunMap::Text { .. }), "{run:?}");
    }

    fn map_of(t: &str) -> (Vec<usize>, String) {
        let l = layout(t, &parse(t), &NoImages, None);
        let run = &l.blocks[0].paras[0].runs[0];
        let RunMap::Text { chars, .. } = &run.map else {
            panic!("{run:?}")
        };
        let offsets = (0..run.text.chars().count())
            .map(|k| chars.get(&run.text, k).unwrap())
            .collect();
        (offsets, run.text.clone())
    }

    #[test]
    fn a_document_cannot_name_splats_own_links() {
        for forged in [
            "splat-task",
            "splat-fold",
            "splat-footnote:a",
            "splat-wiki:a",
            "splat-address:b",
        ] {
            let t = format!("[x]({forged})\n");
            let l = layout(&t, &parse(&t), &NoImages, None);
            let link = l.blocks[0].paras[0].runs[0].style.link.clone().unwrap();
            assert_eq!(link, Link::Address(forged.into()));
            assert_eq!(Link::from_attribute(&link.attribute()), link, "{forged}");
        }
        for link in [
            Link::Address("https://a.de".into()),
            Link::WikiPage("Seite".into()),
            Link::Footnote("1".into()),
            Link::Tag("#t".into()),
            Link::Reference("#12".into()),
            Link::Fold,
            Link::Task,
        ] {
            assert_eq!(Link::from_attribute(&link.attribute()), link);
        }
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
        assert_eq!(&t[src.clone()], "-");
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
        assert_eq!(tag.style.link, Some(Link::Tag("#idee".into())));
        assert!(
            runs.iter()
                .any(|r| r.text == "#12" && r.style.link == Some(Link::Reference("#12".into())))
        );
        assert!(
            runs.iter()
                .any(|r| r.text == "@anna" && matches!(r.style.link, Some(Link::Reference(_))))
        );
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
            Some(Marker::Task { symbol: None, .. })
        ));
        assert!(
            embedded[2]
                .runs
                .iter()
                .any(|r| r.style.link == Some(Link::WikiPage("Notiz".into())))
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
        assert_eq!(paras[0].style.align, Align::Center);
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
        assert_eq!(web.style.link, Some(Link::Address("https://x.de".into())));
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
        assert!(last.runs.iter().any(|r| r.text == "⌘" && r.style.text.code));
        assert!(
            last.runs
                .iter()
                .any(|r| matches!(&r.map, RunMap::Atom { atom: Atom::Image(i), .. } if i.height == Some(10)))
        );
        assert!(last.runs.iter().any(|r| r.text == "fett" && r.style.text.bold));
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
        assert!(find("Begriff").runs.iter().all(|r| r.style.text.bold));
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

    fn paras(l: &Layout) -> Vec<&Para> {
        l.blocks.iter().flat_map(|b| &b.paras).collect()
    }

    fn laid(t: &str) -> Layout {
        layout(t, &parse(t), &NoImages, None)
    }

    fn body(p: &Para) -> String {
        p.runs
            .iter()
            .filter(|r| !matches!(r.map, RunMap::Atom { atom: Atom::Marker, .. }))
            .map(|r| r.text.as_str())
            .collect()
    }

    fn para_with<'a>(l: &'a Layout, text: &str) -> &'a Para {
        paras(l)
            .into_iter()
            .find(|p| body(p) == text)
            .unwrap_or_else(|| panic!("no {text:?} in {:?}", texts(l)))
    }

    struct Everything;

    impl Resolver for Everything {
        fn math_ready(&self, _: &str, _: bool) -> bool {
            true
        }
        fn diagram_ready(&self, _: &str, _: DiagramKind) -> bool {
            true
        }
        fn quotes(&self) -> Option<Quotes> {
            Surroundings.quotes()
        }
        fn references(&self) -> bool {
            true
        }
        fn citation(&self, key: &str) -> Option<String> {
            (key == "doe").then(|| "Doe 2020".to_string())
        }
        fn embedded_note(&self, target: &str) -> Option<(String, String)> {
            (target == "Bilder").then(|| {
                (
                    "![a](a.png) ![b](https://x.de/b.png) ![c](/abs/c.png)\n".to_string(),
                    "Ordner".to_string(),
                )
            })
        }
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
    fn an_ordinary_address_stays_as_it_is() {
        assert_eq!(Link::Address("https://x.de".into()).attribute(), "https://x.de");
    }

    #[test]
    fn without_a_repository_and_a_bibliography_nothing_is_resolved() {
        let l = laid("a @name [@doe]\n");
        let p = para_with(&l, "a @name [@doe]");
        assert!(p.runs.iter().all(|r| r.style.link.is_none()));
        assert!(!NoImages.diagram_ready("a", DiagramKind::Mermaid));
    }

    #[test]
    fn an_embedded_note_asks_the_resolver_around_it() {
        struct Picky;
        impl Resolver for Picky {
            fn math_ready(&self, tex: &str, _: bool) -> bool {
                tex == "yes"
            }
            fn diagram_ready(&self, source: &str, _: DiagramKind) -> bool {
                source == "yes"
            }
            fn quotes(&self) -> Option<Quotes> {
                Surroundings.quotes()
            }
            fn references(&self) -> bool {
                true
            }
            fn citation(&self, key: &str) -> Option<String> {
                (key == "doe").then(|| "Doe".to_string())
            }
        }
        let inner = WithoutEmbeds(&Picky);
        assert!(inner.math_ready("yes", true) && !inner.math_ready("no", true));
        assert!(inner.diagram_ready("yes", DiagramKind::Mermaid) && !inner.diagram_ready("no", DiagramKind::Mermaid));
        assert!(inner.quotes().is_some());
        assert!(inner.references());
        assert_eq!(inner.citation("doe").as_deref(), Some("Doe"));
        assert_eq!(inner.citation("x"), None);
        let none = WithoutEmbeds(&NoImages);
        assert!(none.quotes().is_none() && !none.references());
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
        first.blocks[last].paras[0].runs[0].text = "TAKEN".to_string();
        let mut doc = parse(old);
        let splice = crate::markdown::incremental::reparse(&mut doc, old, &new).unwrap();
        let again = relayout(&new, &doc, &NoImages, None, first, &reusable(&splice, &doc, None));
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
            block.paras[0].runs[1].text = "TAKEN".to_string();
        }
        let mut doc = parse(old);
        let splice = crate::markdown::incremental::reparse(&mut doc, old, &new).unwrap();
        let again = relayout(&new, &doc, &NoImages, None, first, &reusable(&splice, &doc, None));
        let shown: Vec<_> = again.blocks.iter().filter(|b| shows_definition(b)).collect();
        assert_eq!(shown.len(), 3);
        assert!(shown.iter().all(|b| b.paras[0].runs[1].text == "TAKEN"), "{shown:?}");
        para_with(&again, "[3]");
    }

    #[test]
    fn a_taken_over_block_that_shows_its_spaces_counts_as_changed() {
        let old = "aaa  \n\nbbb\n\nccc\n\nddd\n\neee\n\nfff\n";
        let new = old.replace("eee", "eeee");
        let first = laid(old);
        let key = first.blocks[0].key;
        let mut doc = parse(old);
        let splice = crate::markdown::incremental::reparse(&mut doc, old, &new).unwrap();
        let again = relayout(&new, &doc, &NoImages, Some(4), first, &reusable(&splice, &doc, None));
        assert_eq!(texts(&again)[0], "aaa  ");
        assert_ne!(again.blocks[0].key, key);
    }

    #[test]
    fn the_caret_near_the_end_of_a_block_does_not_hold_it() {
        let old = "Eins\n\nZwei\n\nDrei\n\nVier\n\nFünf\n\nSechs\n\nSieben\n";
        let mut doc = parse(old);
        let splice = crate::markdown::incremental::reparse(&mut doc, old, &old.replace("Vier", "Viertel")).unwrap();
        let behind_sechs = old.find("Sechs").unwrap() + "Sechs".len() + 1;
        assert_eq!(reusable(&splice, &doc, Some(behind_sechs))(5), Some(3));
    }

    #[test]
    fn spaces_show_at_the_caret_from_their_first_to_their_last_position() {
        let t = "a  \n\nb  \n";
        let shown = |caret: usize| texts(&layout(t, &parse(t), &NoImages, Some(caret)));
        assert_eq!(shown(6), ["a", "b  "], "right behind the letter");
        assert_eq!(shown(7), ["a", "b  "], "between the spaces");
        assert_eq!(shown(8), ["a", "b  "], "behind them");
        assert_eq!(shown(1), ["a  ", "b"]);
        assert_eq!(shown(0), ["a", "b"], "in front of the letter");
    }

    #[test]
    fn a_blank_line_with_the_caret_gets_a_paragraph_of_its_own() {
        let shown = |t: &str, caret: usize| texts(&layout(t, &parse(t), &NoImages, Some(caret)));
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
            let ranges: Vec<Range<usize>> = d.blocks.iter().map(|b| b.range.clone()).collect();
            empty_line_spots(t, &ranges)
        };
        assert_eq!(
            spots("a  \n\nb\n"),
            Vec::<usize>::new(),
            "spaces behind a paragraph are no blank line"
        );
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
            } => &t[src.clone()],
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
        assert_eq!(at("x\n\n-\n\ny\n"), 4);
        assert_eq!(at("-\n"), 1);
        assert_eq!(at("-   \n\ny\n"), 2);
    }

    #[test]
    fn a_formula_needs_a_math_fence_and_its_picture() {
        let code = |r: &dyn Resolver, t: &str| {
            matches!(
                paras(&layout(t, &parse(t), r, None))[0].style.kind,
                ParaKind::Code { .. }
            )
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
        assert_eq!(at("##\n"), 2, "not behind the end");
        assert_eq!(at("## \n"), 3);
        let t = "x\n\n## \n";
        assert_eq!(paras(&laid(t))[1].insert_at, 6);
    }

    #[test]
    fn inline_html_is_marked_and_a_footnote_reference_raised() {
        let l = laid("a <foo> b[^x]\n\n[^x]: c\n");
        let runs = &paras(&l)[0].runs;
        assert!(runs.iter().find(|r| r.text == "<foo>").unwrap().style.html);
        assert!(runs.iter().find(|r| r.text == "[1]").unwrap().style.text.baseline == Baseline::Super);
    }

    #[test]
    fn code_html_and_links_keep_their_text_as_it_is() {
        let shown = |t: &str| {
            let l = layout(t, &parse(t), &Everything, None);
            let runs: Vec<Run> = paras(&l)[0].runs.clone();
            runs
        };
        let kbd = shown("<kbd>\"a\"</kbd>\n");
        assert!(kbd.iter().any(|r| r.text == "\"a\""), "{kbd:?}");
        let html = shown("<p>\"a\"</p>\n");
        assert!(html.iter().any(|r| r.text.contains("\"a\"")), "{html:?}");
        assert!(
            shown("[#tag](https://x.de)\n")
                .iter()
                .all(|r| !matches!(r.style.link, Some(Link::Tag(_))))
        );
        assert!(
            shown("<kbd>#tag</kbd>\n")
                .iter()
                .all(|r| !matches!(r.style.link, Some(Link::Tag(_))))
        );
        assert!(
            shown("<p>#tag</p>\n")
                .iter()
                .all(|r| !matches!(r.style.link, Some(Link::Tag(_))))
        );
    }

    #[test]
    fn a_mark_at_either_end_of_a_text_leaves_no_empty_run() {
        for t in ["#tag x\n", "x #tag\n", "#tag\n"] {
            let l = layout(t, &parse(t), &Everything, None);
            assert!(paras(&l)[0].runs.iter().all(|r| !r.text.is_empty()), "{t}");
        }
    }

    #[test]
    fn pictures_of_an_embedded_note_are_found_from_the_note_folder() {
        let t = "![[Bilder]]\n";
        let l = layout(t, &parse(t), &Everything, None);
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
            range: 0..0,
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
        assert_eq!(cell_insert("|  |", &(1..3)), 2);
        assert_eq!(cell_insert("x|a", &(2..3)), 2);
    }

    #[test]
    fn a_line_break_collapsed_to_a_space_still_finds_its_place() {
        let t = "x `a\nb` y\n";
        let l = laid(t);
        let run = paras(&l)[0].runs.iter().find(|r| r.text.contains('b')).unwrap();
        let RunMap::Text { chars, .. } = &run.map else {
            panic!("{run:?}");
        };
        let b = run.text.chars().position(|c| c == 'b').unwrap();
        assert_eq!(chars.get(&run.text, b), Some(5));
        assert_eq!(
            chars.get(&run.text, b - 1),
            Some(4),
            "the space stands for the line break"
        );
    }

    #[test]
    fn a_picture_element_shows_its_image_and_a_refused_one_its_text() {
        let image = |t: &str| {
            let l = laid(t);
            paras(&l).iter().any(|p| {
                p.runs.iter().any(|r| {
                    matches!(
                        r.map,
                        RunMap::Atom {
                            atom: Atom::Image(_),
                            ..
                        }
                    )
                })
            })
        };
        assert!(image(
            "<picture><source srcset=\"b.png\"><img src=\"a.png\"></picture>\n"
        ));
        let l = laid("<p><img src=\"javascript:x\" alt=\"A\"></p>\n");
        assert_eq!(texts(&l), ["A"]);
        assert!(
            texts(&laid("<p><img src=\"javascript:x\"> b</p>\n"))
                .iter()
                .all(|t| t.trim() == "b")
        );
        let empty = laid("<p><img src=\"javascript:x\">b</p>\n");
        assert!(paras(&empty)[0].runs.iter().all(|r| !r.text.is_empty()));
    }

    #[test]
    fn only_a_center_element_centers_itself() {
        assert_eq!(para_with(&laid("<center>a</center>\n"), "a").style.align, Align::Center);
        assert_eq!(para_with(&laid("<p>a</p>\n"), "a").style.align, Align::None);
    }

    #[test]
    fn a_list_in_html_goes_deeper_with_its_nesting() {
        let l = laid("<ul><li>a<ul><li>b</li></ul></li></ul>\n");
        assert_eq!(para_with(&l, "a").style.list_depth, 1);
        let b = para_with(&l, "b");
        assert_eq!(b.style.list_depth, 2);
        assert_eq!(b.style.marker, Some(Marker::Bullet(1)));
    }

    #[test]
    fn tables_in_html_count_up_and_keep_bold_of_their_head_and_their_text() {
        let l =
            laid("<table><tr><th>a</th><td>b</td><td><b>c</b></td></tr></table><table><tr><td>d</td></tr></table>\n");
        assert!(para_with(&l, "a").runs[0].style.text.bold);
        assert!(!para_with(&l, "b").runs[0].style.text.bold);
        assert!(para_with(&l, "c").runs[0].style.text.bold);
        let table = |text: &str| match para_with(&l, text).style.kind {
            ParaKind::TableCell { table, .. } => table,
            _ => panic!("no cell"),
        };
        assert_eq!((table("a"), table("d")), (1, 2));
    }

    #[test]
    fn details_in_html_fold_by_a_click_and_their_summary_is_bold() {
        let t = "<details><summary>a</summary>b</details>\n";
        let l = laid(t);
        let p = paras(&l)[0];
        assert_eq!(p.runs[0].style.link, Some(Link::Fold));
        let summary = p.runs.iter().find(|r| r.text == "a").unwrap();
        assert!(summary.style.text.bold && summary.style.link.is_none());
    }

    #[test]
    fn spaces_behind_a_paragraph_do_not_count_as_a_blank_line() {
        let d = parse("a \n\n\nb\n");
        let ranges: Vec<Range<usize>> = d.blocks.iter().map(|b| b.range.clone()).collect();
        assert_eq!(empty_line_spots("a \n\n\nb\n", &ranges), Vec::<usize>::new());
    }

    #[test]
    fn text_typed_into_an_empty_task_lands_behind_its_box() {
        let t = "- [ ]\n\nx\n";
        let l = laid(t);
        let item = paras(&l).iter().find(|p| p.style.marker.is_some()).copied().unwrap();
        assert_eq!(item.insert_at, 5);
    }

    #[test]
    fn a_refused_picture_without_a_text_leaves_nothing_behind() {
        assert!(
            paras(&laid("<p><img src=\"javascript:x\"></p>\n"))
                .iter()
                .all(|p| p.runs.is_empty())
        );
    }

    #[test]
    fn blocks_in_a_table_cell_of_html_become_its_lines() {
        let cell = |content: &str| {
            let t = format!("<table><tr><td>{content}</td><td>z</td></tr></table>\n\n<p>behind</p>\n");
            let l = laid(&t);
            let shown = texts(&l);
            assert_eq!(
                shown.len(),
                3,
                "the cell, its neighbour and the paragraph behind: {shown:?}"
            );
            assert_eq!(shown[1], "z");
            assert_eq!(
                para_with(&l, "behind").style.kind,
                ParaKind::Body,
                "it ends with the table"
            );
            assert!(matches!(paras(&l)[0].style.kind, ParaKind::TableCell { .. }));
            shown[0].clone()
        };
        let line = LINE_SEPARATOR;
        assert_eq!(cell("a"), "a");
        assert_eq!(cell("<p>a</p>"), "a");
        assert_eq!(cell("<p>a</p><p>b</p>"), format!("a{line}b"));
        assert_eq!(cell("a<p>b</p>c"), format!("a{line}b{line}c"));
        assert_eq!(cell("<div><p>a</p></div>"), "a");
        assert_eq!(cell("<ul><li>a</li><li>b</li></ul>"), format!("• a{line}• b"));
        assert_eq!(cell("x<ol start=\"3\"><li>a</li></ol>"), format!("x{line}3. a"));
        assert_eq!(cell("<hr>a"), "a");
        assert_eq!(cell("<pre>x\ny</pre>"), format!("x{line}y"));
        assert_eq!(cell("<table><tr><td>a</td><td>b</td></tr></table>"), "a b");
        assert_eq!(cell("<h2>a</h2>b"), format!("a{line}b"));
    }
}
