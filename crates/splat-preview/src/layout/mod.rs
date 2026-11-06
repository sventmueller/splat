//! The preview as paragraphs of styled runs, laid out from the document's tree block by block, each run with where it
//! comes from in the source.

use std::ops::Range;
use std::sync::Arc;

use crate::Shown;
use splat_markdown::doc::{AlertKind, Align, DiagramKind, Doc, Splice};
use splat_markdown::style::TextStyle;
use splat_markdown::text::{Byte, TextAt, line_end, line_start, shifted, utf16_len};
use splat_markdown::typography::{Open, Quotes};

mod blocks;
mod build;
mod html;
mod inlines;

pub(crate) const ATTACHMENT: char = '\u{FFFC}';
pub(crate) const LINE_SEPARATOR: char = '\u{2028}';

/// What a paragraph of the preview is, which decides how it is set.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParaKind {
    /// Running text, as in a paragraph, a list item or a quote.
    Body,
    /// A heading of the level 1 to 6.
    Heading(u8),
    /// The text of a code block.
    Code {
        /// The first word of the info string, naming the language; empty without one.
        info: Box<str>,
    },
    /// A horizontal rule.
    Rule,
    /// A formula that stands on its own, rendered or, until it is, as its TeX.
    MathBlock,
    /// A rendered diagram.
    Diagram,
    /// A cell of a table.
    TableCell {
        /// Which table the cell belongs to: the cells of one table share the number.
        table: u32,
        /// The row, counted from 0 at the top.
        row: u32,
        /// The column, counted from 0 in source order.
        col: u32,
        /// Whether it stands in the header row.
        header: bool,
        /// How its column aligns the text.
        align: Align,
        /// How many columns the table has.
        columns: u32,
        /// Columns the cell spans, more than one only in HTML tables.
        span: u32,
    },
    /// Running text of a footnote's definition.
    FootnoteDef,
    /// The title line of an alert or callout, behind its icon.
    AlertTitle,
    /// Front matter, shown as one line naming its keys.
    FrontMatter,
}

/// How a paragraph of the preview is set.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParaStyle {
    /// What the paragraph is.
    pub kind: ParaKind,
    /// How deep it stands in quotes, alerts, callouts and embedded notes.
    pub quote_depth: u8,
    /// How deep it stands in lists.
    pub list_depth: u8,
    /// What is drawn before the first paragraph of a list item or footnote definition.
    pub marker: Option<ItemMarker>,
    /// Paragraphs sharing a group id share one background or border block (code, quotes, tables).
    pub group: usize,
    /// It stands in a tight list, whose paragraphs go without spacing.
    pub tight: bool,
    /// First paragraph after a table, which cannot carry spacing below itself.
    pub space_before: bool,
    /// The kind of the alert or callout it stands in.
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

/// What stands in front of the first paragraph of a list item or a footnote's definition: its bullet, number,
/// checkbox or label.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ItemMarker {
    /// A bullet, whose shape changes with the list depth it gives.
    Bullet(u8),
    /// The number of an item of an ordered list.
    Number(usize),
    /// The checkbox of a task list item.
    Task {
        /// Whether the box is ticked.
        checked: bool,
        /// Where the `x` or the space stands in the source; an embedded note's checkboxes have no place in this text.
        symbol: Option<Byte>,
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
    /// A footnote reference, by the name of its footnote.
    Footnote(Box<str>),
    /// An Obsidian `#tag`, with its `#`.
    Tag(Box<str>),
    /// `@name`, `#123` or `!123` on the code host of the document's repository.
    Reference(Box<str>),
    /// The chevron of a callout or `<details>`.
    Fold,
    /// A task's checkbox.
    Task,
    /// The summary of the front matter, which opens its form.
    Metadata,
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
            Link::Metadata => "splat-metadata".to_string(),
        }
    }

    /// The link a text attribute stands for, as [`Link::attribute`] wrote it; anything else is an address.
    pub fn from_attribute(attribute: &str) -> Link {
        let named = |prefix: &str| attribute.strip_prefix(prefix).map(Into::into);
        match attribute {
            "splat-fold" => Link::Fold,
            "splat-task" => Link::Task,
            "splat-metadata" => Link::Metadata,
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

/// How a run of the preview looks and where a click on it leads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct RunStyle {
    /// The look of its text.
    pub text: TextStyle,
    /// Raw HTML shown as it stands.
    pub html: bool,
    /// A Pandoc citation.
    pub citation: bool,
    /// Where a click on it leads, if anywhere.
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

/// What a run stands for that maps to a whole range of the source, rather than character by character.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Atom {
    /// Text that represents source without being editable character by character.
    Text,
    /// The [`ItemMarker`] in front of a paragraph: a list bullet, number, checkbox or footnote label.
    Marker,
    /// Boxed, as the largest kind would otherwise size every run.
    Image(Box<ImageAtom>),
    /// A rendered formula.
    Math {
        /// The TeX without its delimiters.
        tex: Box<str>,
        /// Whether it is display math, set apart from the line.
        display: bool,
    },
    /// A rendered diagram.
    Diagram {
        /// The code of the diagram's code block.
        source: Box<str>,
        /// The language the code block names.
        kind: DiagramKind,
    },
    /// A footnote reference, shown as its number in brackets.
    FootnoteRef {
        /// The name of the footnote it points to.
        name: Box<str>,
    },
    /// A horizontal rule.
    Rule,
    /// The icon in front of an alert's title.
    AlertIcon(AlertKind),
    /// The chevron of a foldable callout.
    Fold {
        /// Whether the callout shows its body.
        open: bool,
        /// Where the `-` or `+` a click flips stands; an embedded note's chevrons have no place in this text.
        sign: Option<Byte>,
    },
    /// The chevron of an HTML `<details>` element; its source is the opening tag, whose `open` a click toggles.
    Details {
        /// Whether the `<details>` element shows its content.
        open: bool,
    },
}

/// A picture the preview shows in place.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageAtom {
    /// Where the picture is, as the document gives it.
    pub url: Box<str>,
    /// The text that stands for the picture where it cannot be seen.
    pub alt: Box<str>,
    /// The size embedded HTML asks for, in pixels.
    pub width: Option<u32>,
    /// The height embedded HTML asks for, in pixels.
    pub height: Option<u32>,
}

/// Where the text of a run comes from in the source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RunMap {
    /// Source byte offset of every char of the run text, plus the end of the last char.
    Text {
        /// The source position of each character.
        chars: CharMap,
        /// The source position behind the last character.
        end: Byte,
    },
    /// A run that stands for a whole range of the source.
    Atom {
        /// What it stands for.
        atom: Atom,
        /// The range of the source it stands for.
        src: Range<Byte>,
    },
}

/// Source positions of a run's characters. Text copied verbatim from the source needs no table, which saves four bytes
/// per character for almost all text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CharMap {
    /// Text copied from the source unchanged, from this position on.
    Verbatim(Byte),
    /// Offsets relative to the first char's: a run never spans gigabytes.
    Offsets(Byte, Box<[u32]>),
}

impl CharMap {
    /// Source position of char `k` of the run text `text`, or `None` past its end.
    pub fn get(&self, text: &str, k: usize) -> Option<Byte> {
        match self {
            CharMap::Verbatim(start) => text.char_indices().nth(k).map(|(b, _)| *start + b),
            CharMap::Offsets(base, v) => v.get(k).map(|&o| *base + o as usize),
        }
    }

    /// Number of chars of `text` whose source position lies before `src`.
    pub fn count_before(&self, text: &str, src: Byte) -> usize {
        match self {
            CharMap::Verbatim(start) => text.char_indices().take_while(|(b, _)| *start + *b < src).count(),
            CharMap::Offsets(base, v) => v.partition_point(|&c| *base + (c as usize) < src),
        }
    }
}

/// A stretch of a paragraph in one style, with where it comes from in the source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Run {
    /// What it shows.
    pub text: String,
    /// How it looks and where a click on it leads.
    pub style: RunStyle,
    /// Where its characters come from in the source.
    pub map: RunMap,
}

/// A paragraph of the preview: running text, a heading, a code block, a table cell or the like.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Para {
    /// How it is set.
    pub style: ParaStyle,
    /// Its text, run by run.
    pub runs: Vec<Run>,
    /// Source range of the leaf block this paragraph renders.
    pub src: Range<Byte>,
    /// Where typed text goes when the paragraph has no text yet.
    pub insert_at: Byte,
}

impl Para {
    /// Source position of the end of the paragraph's content.
    pub fn content_end(&self) -> Byte {
        match self.runs.last().map(|r| &r.map) {
            Some(RunMap::Text { end, .. }) => *end,
            Some(RunMap::Atom { src, .. }) => src.end,
            None => self.insert_at,
        }
    }
}

impl Para {
    /// The runs in order, each with the part of the preview text it shows when the paragraph starts at `start`.
    pub fn runs_at(&self, start: Shown) -> impl Iterator<Item = (Range<Shown>, &Run)> {
        let mut at = start;
        self.runs.iter().map(move |run| {
            let from = at;
            at += utf16_len(&run.text);
            (from..at, run)
        })
    }
}

/// What a top-level block of the document shows in the preview, with what tells whether it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopBlock {
    /// The paragraphs it shows, in order.
    pub paras: Vec<Para>,
    /// Where the block stands in the source.
    pub src: Range<Byte>,
    /// It shows the definition of a footnote.
    pub shows_definition: bool,
    /// A hash of what its paragraphs render, without their source positions, so a block that an edit in front of it
    /// moved keeps its key.
    pub key: u64,
    /// Its length in the preview text in UTF-16 units, with a separator behind each paragraph.
    pub len_u16: usize,
    /// The length of each paragraph in UTF-16 units, counted once: an index of a large document is made per keystroke.
    pub para_lens: Box<[u32]>,
    /// The HTML wrappers around the block, as `<details>` before it, which a taken-over block must share.
    pub context: u64,
    /// The notes it embeds.
    pub embeds: Embeds,
}

/// The notes a block embeds, of which only the first in a document are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Embeds {
    /// How many notes it shows embedded.
    pub shown: usize,
    /// A further one had to stay a link.
    pub refused: bool,
}

impl Embeds {
    /// Whether a block laid out anew behind `before` embedded notes would embed exactly these.
    fn hold_behind(self, before: usize) -> bool {
        let after = before + self.shown;
        after <= MAX_EMBEDS && (!self.refused || after == MAX_EMBEDS)
    }
}

/// The preview of a document as laid-out top-level blocks.
#[derive(Debug, Default)]
pub struct Layout {
    /// Shared, so an update can hand the changed blocks to the views without keeping the layout borrowed.
    pub blocks: Vec<Arc<TopBlock>>,
    /// The version of the document this was laid out from.
    pub doc_version: u64,
}

impl Layout {
    /// The layout of a document that shows nothing.
    pub fn nothing_of(doc: &Doc) -> Layout {
        Layout {
            blocks: Vec::new(),
            doc_version: doc.version(),
        }
    }
}

/// Decides whether rendered images for math and diagrams exist yet; otherwise their source is shown as text.
pub trait Resolver {
    /// Whether the formula `tex`, inline or as `display` math, is rendered, so the preview can show it as an image.
    fn math_ready(&self, tex: &str, display: bool) -> bool;
    /// Whether the diagram `source` in the language `kind` is rendered, so the preview can show it as an image.
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
type LeadingMarker = Option<(ItemMarker, Range<Byte>)>;

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
    embeds_refused: bool,
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
pub fn layout(doc: &Doc, resolver: &dyn Resolver, caret: Option<Byte>) -> Layout {
    build::build(doc, resolver, caret.map(|c| doc.fit_at(c)), Vec::new(), &|_| None)
}

/// Like `layout`, but takes over the laid-out blocks of `old` for the document blocks `reuse` returns the offset
/// shift of; their text must be unchanged and they must not hold the previous caret.
pub fn relayout(
    doc: &Doc,
    resolver: &dyn Resolver,
    caret: Option<Byte>,
    old: Layout,
    reuse: &dyn Fn(usize) -> Option<isize>,
) -> Layout {
    build::build(doc, resolver, caret.map(|c| doc.fit_at(c)), old.blocks, reuse)
}

/// The `reuse` argument of `relayout` after a partial parse of the new `doc`: blocks outside the splice keep their
/// layout, shifted behind it, unless they held the caret the old layout was made for.
pub fn reusable<'a>(splice: &'a Splice, doc: &'a Doc, old_caret: Option<Byte>) -> impl Fn(usize) -> Option<isize> + 'a {
    move |i| {
        if splice.new.contains(&i) {
            return None;
        }
        let r = &doc.blocks().get(i)?.range;
        let shift = if r.start >= splice.behind { splice.delta } else { 0 };
        let (start, end) = (shifted(r.start, -shift), shifted(r.end, -shift));
        (!old_caret.is_some_and(|c| start <= c && c <= end)).then_some(shift)
    }
}

/// The `reuse` argument of `relayout` when only the caret moved: all blocks keep their layout, but the one that held it.
pub fn reusable_behind_caret<'a>(doc: &'a Doc, old_caret: Option<Byte>) -> impl Fn(usize) -> Option<isize> + 'a {
    move |i| {
        let r = &doc.blocks().get(i)?.range;
        (!old_caret.is_some_and(|c| r.start <= c && c <= r.end)).then_some(0)
    }
}

/// Whether a caret there can change what a layout shows: an empty line then shows as a paragraph, and spaces at the end of
/// a line as text. Anywhere else a caret that moves leaves the layout as it is.
pub fn caret_shapes_layout(text: &str, caret: Byte) -> bool {
    let at = text.fit_at(caret);
    let line = text.at(line_start(text, at)..line_end(text, at));
    line.trim().is_empty() || line.ends_with([' ', '\t'])
}

#[cfg(test)]
mod tests {
    use super::*;
    use splat_markdown::reader::parse;

    pub(super) fn texts(l: &Layout) -> Vec<String> {
        l.blocks
            .iter()
            .flat_map(|b| b.paras.iter().map(|p| p.runs.iter().map(|r| r.text.as_str()).collect()))
            .collect()
    }

    #[test]
    fn blocks_outside_a_partial_parse_are_reused_unless_they_held_the_caret() {
        let old = "Eins\n\nZwei\n\nDrei\n\nVier\n\nFünf\n\nSechs\n\nSieben\n";
        let mut doc = parse(old);
        let splice = splat_markdown::incremental::reparse(&mut doc, &old.replace("Vier", "Viertel")).unwrap();
        assert_eq!((splice.new.clone(), splice.delta), (2..5, 3));
        let reuse = reusable(&splice, &doc, None);
        let all: Vec<_> = (0..8).map(&reuse).collect();
        assert_eq!(all, [Some(0), Some(0), None, None, None, Some(3), Some(3), None]);
        let caret_in_sechs = reusable(&splice, &doc, Some(Byte::new(old.find("Sechs").unwrap())));
        assert_eq!((caret_in_sechs(5), caret_in_sechs(6)), (None, Some(3)));
        assert_eq!(reusable(&splice, &doc, Some(Byte::new(0)))(0), None);
    }

    #[test]
    fn a_layout_knows_the_version_it_was_made_from_also_when_part_of_it_is_taken_over() {
        let old = "Eins\n\nZwei\n\nDrei\n\nVier\n\nFünf\n\nSechs\n\nSieben\n";
        let mut doc = parse(old);
        let first = layout(&doc, &NoImages, None);
        assert_eq!(first.doc_version, doc.version());
        assert_eq!(Layout::default().doc_version, 0);
        let splice = splat_markdown::incremental::reparse(&mut doc, &old.replace("Vier", "Viertel")).unwrap();
        assert_ne!(first.doc_version, doc.version());
        let again = relayout(&doc, &NoImages, None, first, &reusable(&splice, &doc, None));
        assert_eq!(again.doc_version, doc.version());
    }

    #[test]
    fn runs_know_what_part_of_the_preview_they_show() {
        let l = laid("ä😀 **b** `c`\n");
        let para = &l.blocks[0].paras[0];
        let start = Shown::new(10);
        let parts: Vec<(Range<Shown>, &str)> = para.runs_at(start).map(|(at, r)| (at, r.text.as_str())).collect();
        assert_eq!(
            parts,
            [
                (Shown::new(10)..Shown::new(14), "ä😀 "),
                (Shown::new(14)..Shown::new(15), "b"),
                (Shown::new(15)..Shown::new(16), " "),
                (Shown::new(16)..Shown::new(17), "c"),
            ]
        );
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
            let l = layout(&parse(&t), &NoImages, None);
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

    pub(super) struct Surroundings;

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
    fn block_keys_change_only_for_edited_blocks() {
        let a = "Eins\n\nZwei\n\nDrei\n";
        let b = "Eins\n\nZwei!\n\nDrei\n";
        let la = layout(&parse(a), &NoImages, None);
        let lb = layout(&parse(b), &NoImages, None);
        assert_eq!(la.blocks[0].key, lb.blocks[0].key);
        assert_ne!(la.blocks[1].key, lb.blocks[1].key);
        assert_eq!(la.blocks[2].key, lb.blocks[2].key);
    }

    pub(super) fn paras(l: &Layout) -> Vec<&Para> {
        l.blocks.iter().flat_map(|b| &b.paras).collect()
    }

    pub(super) fn laid(t: &str) -> Layout {
        layout(&parse(t), &NoImages, None)
    }

    pub(super) fn body(p: &Para) -> String {
        p.runs
            .iter()
            .filter(|r| !matches!(r.map, RunMap::Atom { atom: Atom::Marker, .. }))
            .map(|r| r.text.as_str())
            .collect()
    }

    pub(super) fn para_with<'a>(l: &'a Layout, text: &str) -> &'a Para {
        paras(l)
            .into_iter()
            .find(|p| body(p) == text)
            .unwrap_or_else(|| panic!("no {text:?} in {:?}", texts(l)))
    }

    pub(super) struct Everything;

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

    #[test]
    fn a_stale_caret_lays_out_as_one_in_the_text() {
        let text = "Füße  \n\n\nü\n";
        let doc = parse(text);
        for caret in [2, 9, text.len() + 5] {
            let at = Some(Byte::new(caret));
            assert_eq!(
                texts(&layout(&doc, &NoImages, at)),
                texts(&layout(&doc, &NoImages, at.map(|c| doc.fit_at(c)))),
                "{caret}"
            );
            let _ = caret_shapes_layout(text, Byte::new(caret));
        }
        let old = layout(&doc, &NoImages, None);
        let again = relayout(&doc, &NoImages, Some(Byte::new(2)), old, &|_| None);
        assert_eq!(again.doc_version, doc.version());
    }

    #[test]
    fn the_caret_near_the_end_of_a_block_does_not_hold_it() {
        let old = "Eins\n\nZwei\n\nDrei\n\nVier\n\nFünf\n\nSechs\n\nSieben\n";
        let mut doc = parse(old);
        let splice = splat_markdown::incremental::reparse(&mut doc, &old.replace("Vier", "Viertel")).unwrap();
        let behind_sechs = old.find("Sechs").unwrap() + "Sechs".len() + 1;
        assert_eq!(reusable(&splice, &doc, Some(Byte::new(behind_sechs)))(5), Some(3));
    }
}
