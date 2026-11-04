use std::ops::Range;
use std::sync::Arc;

use splat_markdown::doc::{AlertKind, Align, DiagramKind, Doc, Splice};
use splat_markdown::style::TextStyle;
use splat_markdown::text::{Byte, Shown, TextAt, line_end, line_start, shifted, utf16_len};
use splat_markdown::typography::{Open, Quotes};

mod blocks;
mod build;
mod html;
mod inlines;

pub use build::{empty_line_spots, para_len_u16};
pub use inlines::align;

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
    /// The chevron of a foldable callout.
    Fold {
        open: bool,
        /// Where the `-` or `+` a click flips stands; an embedded note's chevrons have no place in this text.
        sign: Option<Byte>,
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
        end: Byte,
    },
    Atom {
        atom: Atom,
        src: Range<Byte>,
    },
}

/// Source positions of a run's characters. Text copied verbatim from the source needs no table, which saves four bytes
/// per character for almost all text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CharMap {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopBlock {
    pub paras: Vec<Para>,
    pub src: Range<Byte>,
    /// It shows the definition of a footnote.
    pub shows_definition: bool,
    pub key: u64,
    pub len_u16: usize,
    /// The length of each paragraph in UTF-16 units, counted once: an index of a large document is made per keystroke.
    pub para_lens: Box<[u32]>,
    /// The HTML wrappers around the block, as `<details>` before it, which a taken-over block must share.
    pub context: u64,
    pub embeds: Embeds,
}

/// The notes a block embeds, of which only the first in a document are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Embeds {
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
type LeadingMarker = Option<(Marker, Range<Byte>)>;

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
    build::build(doc, resolver, caret, Vec::new(), &|_| None)
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
    build::build(doc, resolver, caret, old.blocks, reuse)
}

/// The `reuse` argument of `relayout` after a partial parse of the new `doc`: blocks outside the splice keep their
/// layout, shifted behind it, unless they held the caret the old layout was made for.
pub fn reusable<'a>(splice: &'a Splice, doc: &'a Doc, old_caret: Option<Byte>) -> impl Fn(usize) -> Option<isize> + 'a {
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

/// The `reuse` argument of `relayout` when only the caret moved: all blocks keep their layout, but the one that held it.
pub fn reusable_behind_caret<'a>(doc: &'a Doc, old_caret: Option<Byte>) -> impl Fn(usize) -> Option<isize> + 'a {
    move |i| {
        let r = &doc.blocks.get(i)?.range;
        (!old_caret.is_some_and(|c| r.start <= c && c <= r.end)).then_some(0)
    }
}

/// Whether a caret there can change what a layout shows: an empty line then shows as a paragraph, and spaces at the end of
/// a line as text. Anywhere else a caret that moves leaves the layout as it is.
pub fn caret_shapes_layout(text: &str, caret: Byte) -> bool {
    let at = caret.min(text.end());
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
    fn the_caret_near_the_end_of_a_block_does_not_hold_it() {
        let old = "Eins\n\nZwei\n\nDrei\n\nVier\n\nFünf\n\nSechs\n\nSieben\n";
        let mut doc = parse(old);
        let splice = splat_markdown::incremental::reparse(&mut doc, &old.replace("Vier", "Viertel")).unwrap();
        let behind_sechs = old.find("Sechs").unwrap() + "Sechs".len() + 1;
        assert_eq!(reusable(&splice, &doc, Some(Byte::new(behind_sechs)))(5), Some(3));
    }
}
