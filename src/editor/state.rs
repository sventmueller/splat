//! What the controller knows about the document, derived from the source text, and every way that knowledge changes.
//! Its fields are private, so nothing outside this file can leave it half done; no view is touched here, which keeps it
//! testable without a window.

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use super::scroll::ScrollSpan;
use super::sync::{BlockDiff, Blocks, Snapshot};
use crate::markdown::commands::Pending;
use crate::markdown::doc::Doc;
use crate::markdown::highlight::Style;
use crate::markdown::incremental::{
    Parsed, Splice, parse_shared_and_color, reparse_and_color, reparse_part_and_color, stays_plain,
};
use crate::markdown::text::{Byte, Shown, U16};
use crate::preview::layout::{Layout, Marker, Resolver, TopBlock, layout, relayout, reusable};
use crate::preview::map::Index;
use crate::util::{contained, free_in_background};

/// Why the editor redraws although the source text did not change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Invalidate {
    /// The whole text was replaced, e.g. after loading or reverting.
    Text,
    /// The fonts changed.
    Zoom,
    /// Light and dark appearance switched.
    Appearance,
    /// The preview no longer matches what the controller knows, e.g. after an edit it could not follow.
    Preview,
    /// Rendered formulas or diagrams arrived.
    Arrivals,
    /// The user allowed images from the web.
    RemoteAllowed,
    /// A setting changed what the preview shows for the same text.
    Layout,
}

/// What the next update rebuilds although the source text is unchanged.
#[derive(Default, Clone, Copy)]
struct Stale {
    /// The syntax colors of the whole source.
    colors: bool,
    /// The layout, which may now show rendered formulas, diagrams or images.
    layout: bool,
}

type Spans = Arc<Vec<(Range<Byte>, Style)>>;

/// Up to this many bytes, typing in the source parses around the edit right away, in about 8 ms. Above, that would eat
/// into the frame the keystroke itself needs, and everything is parsed in the background.
const PARTS_ON_MAIN_UP_TO: usize = 2 << 20;

/// What the preview view shows for the text; only blocks can be edited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PreviewMode {
    #[default]
    Shows,
    /// Parsing or laying out this text panicked; the preview says so, and the text stays editable in the source.
    Failed,
    /// The text stays plain from its size on.
    TooLarge,
}

#[derive(Default)]
pub(super) struct State {
    version: u64,
    doc: Doc,
    layout: Layout,
    index: Index,
    blocks: Blocks,
    caret: Option<Byte>,
    zoom: f64,
    /// Preview range (before the change) the user edited directly, and the length delta.
    dirty_preview: Option<(Range<Shown>, isize)>,
    rendered_dark: Option<bool>,
    stale: Stale,
    /// Styles for the next typed text, valid while the source still equals the stored text.
    pending: Option<(Pending, String)>,
    /// Syntax colors currently applied to the source view.
    spans: Spans,
    /// Source and preview UTF-16 ranges of every top-level block, for scroll synchronisation; built on first use.
    scroll_spans: Option<Rc<Vec<ScrollSpan>>>,
    mode: PreviewMode,
    /// Parsing this text panicked; unlike a failed layout, only another text can end that.
    parse_failed: bool,
}

/// What the preview needs after an update.
enum PreviewWork<'a> {
    Keep,
    /// The preview is hidden; it is laid out once it shows again.
    Postpone,
    Lay(LayoutSource<'a>),
}

enum LayoutSource<'a> {
    /// The layout the background thread computed for this text, without a caret.
    Background(Layout, Index),
    /// The previous layout, with the blocks outside the changed ones taken over.
    Partial(&'a Splice),
    Full,
}

/// What `preview_work` decides on.
struct PreviewFacts<'a> {
    text_changed: bool,
    caret_moved: bool,
    /// The user edited the preview itself, so it no longer shows what its blocks say.
    preview_edited: bool,
    shows_blocks: bool,
    /// Formulas or images that arrived meanwhile change blocks the text did not touch.
    stale_layout: bool,
    visible: bool,
    has_caret: bool,
    background_layout: Option<(Layout, Index)>,
    partial_parse: Option<&'a Splice>,
}

fn preview_work(f: PreviewFacts<'_>) -> PreviewWork<'_> {
    let PreviewFacts {
        text_changed,
        caret_moved,
        preview_edited,
        shows_blocks,
        stale_layout,
        visible,
        has_caret,
        mut background_layout,
        partial_parse,
    } = f;
    let work = if !text_changed && !caret_moved && !preview_edited && shows_blocks && !stale_layout {
        PreviewWork::Keep
    } else if !visible && !preview_edited {
        PreviewWork::Postpone
    } else {
        match (background_layout.take_if(|_| !has_caret), partial_parse) {
            (Some((layout, index)), _) => PreviewWork::Lay(LayoutSource::Background(layout, index)),
            (None, Some(splice)) if !stale_layout => PreviewWork::Lay(LayoutSource::Partial(splice)),
            _ => PreviewWork::Lay(LayoutSource::Full),
        }
    };
    if background_layout.is_some() {
        free_in_background(background_layout);
    }
    work
}

/// What the views say when an update starts.
#[derive(Clone, Copy)]
pub(super) struct ViewFacts {
    /// Where the source caret stands, while the preview has the focus and shows it.
    pub caret_at: Option<U16>,
    /// The preview is not collapsed.
    pub visible: bool,
    /// The current length of the preview text.
    pub shown_len: usize,
    pub dark: bool,
}

/// What the background thread made of a text.
pub(super) struct Background {
    pub parsed: Parsed,
    /// Laid out if the preview showed, without a caret.
    pub layout: Option<(Layout, Index)>,
    pub failed: bool,
}

/// What the views need after the state advanced.
#[must_use]
pub(super) struct Update {
    pub recolor: Option<Recolor>,
    pub preview: Option<PreviewChange>,
    pub text_changed: bool,
    /// The preview last showed the other appearance.
    pub appearance_switched: bool,
}

/// How the preview view gets to show the new layout.
pub(super) struct PreviewChange {
    /// The changed blocks, of which the first stands at `diff.prefix`.
    pub blocks: Vec<Arc<TopBlock>>,
    pub diff: BlockDiff,
    pub mode: PreviewMode,
    pub zoom: f64,
}

/// The colors of the source to change after a new parse.
pub(super) struct Recolor {
    pub old_text: Arc<str>,
    pub old_spans: Spans,
    pub text: Arc<str>,
    pub spans: Spans,
}

/// The layout of a text, which has none when it stays plain; `None` if it panicked.
pub(super) fn laid_out(doc: &Doc, resolver: &dyn Resolver, caret: Option<Byte>) -> Option<Layout> {
    if stays_plain(doc.text()) {
        return Some(Layout::nothing_of(doc));
    }
    contained(|| layout(doc, resolver, caret))
}

fn mode_of(failed: bool, text: &str) -> PreviewMode {
    if failed {
        PreviewMode::Failed
    } else if stays_plain(text) {
        PreviewMode::TooLarge
    } else {
        PreviewMode::Shows
    }
}

#[cfg(test)]
thread_local! {
    static PARSING_PANICS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A test's way into the panics that `contained` is there for.
fn panic_if_a_test_asks() {
    #[cfg(test)]
    assert!(!PARSING_PANICS.get(), "a test asked for this panic");
}

impl State {
    pub(super) fn new() -> State {
        State {
            zoom: 1.0,
            ..State::default()
        }
    }

    pub(super) fn text(&self) -> &str {
        self.doc.text()
    }

    pub(super) fn version(&self) -> u64 {
        self.version
    }

    pub(super) fn doc(&self) -> &Doc {
        &self.doc
    }

    pub(super) fn layout(&self) -> &Layout {
        &self.layout
    }

    pub(super) fn index(&self) -> &Index {
        &self.index
    }

    pub(super) fn blocks(&self) -> &Blocks {
        &self.blocks
    }

    pub(super) fn caret(&self) -> Option<Byte> {
        self.caret
    }

    pub(super) fn zoom(&self) -> f64 {
        self.zoom
    }

    pub(super) fn rendered_dark(&self) -> Option<bool> {
        self.rendered_dark
    }

    pub(super) fn pending(&self) -> Option<&(Pending, String)> {
        self.pending.as_ref()
    }

    pub(super) fn preview_mode(&self) -> PreviewMode {
        self.mode
    }

    pub(super) fn stale_layout(&self) -> bool {
        self.stale.layout
    }

    /// The state was computed from the text `snap` holds; equal versions save comparing the texts.
    /// Where the paragraph of the footnote called `name` starts in the preview, and its length.
    pub(super) fn footnote_paragraph(&self, name: &str) -> Option<(Shown, usize)> {
        self.index
            .paras
            .iter()
            .enumerate()
            .find(|(i, _)| {
                matches!(&self.index.para(&self.layout, *i).style.marker, Some(Marker::Footnote(n, _)) if &**n == name)
            })
            .map(|(_, paragraph)| (paragraph.start, paragraph.len))
    }

    pub(super) fn is_current(&self, snap: &Snapshot) -> bool {
        self.version == snap.version || self.doc.text() == snap.text
    }

    /// The layout and what the view shows are of an older text, after an edit while the preview was hidden.
    pub(super) fn preview_is_behind(&self) -> bool {
        self.layout.doc_version != self.doc.version()
    }

    /// Marks what `why` made outdated.
    pub(super) fn invalidate(&mut self, why: Invalidate) {
        match why {
            Invalidate::Text | Invalidate::Zoom | Invalidate::Appearance => {
                self.stale.colors = true;
                self.blocks.forget();
                self.dirty_preview = None;
            }
            Invalidate::Preview => {
                self.blocks.forget();
                self.dirty_preview = None;
            }
            Invalidate::Layout => {
                self.stale.layout = true;
                self.blocks.forget();
                self.dirty_preview = None;
            }
            Invalidate::Arrivals => self.stale.layout = true,
            Invalidate::RemoteAllowed => {
                self.stale.layout = true;
                // A loaded image keeps the block's text, so its key would not change on its own.
                let layout = &self.layout;
                self.blocks
                    .invalidate(|i| layout.blocks.get(i).is_some_and(|block| shows_image(block)));
            }
        }
        if why == Invalidate::Text {
            self.pending = None;
        }
    }

    /// Renders those shown blocks again that `arrived` says show a picture that just came in; whether there are any.
    pub(super) fn mark_arrived(&mut self, arrived: impl Fn(&TopBlock) -> bool) -> bool {
        if self.blocks.placeholders.len() != self.layout.blocks.len() {
            return false;
        }
        let mut any = false;
        for (i, block) in self.layout.blocks.iter().enumerate() {
            if arrived(block) {
                self.blocks.placeholders[i] = true;
                any = true;
            }
        }
        any
    }

    /// The given blocks went into the preview view rendered; whether placeholders are left in `window`.
    pub(super) fn rendered(&mut self, done: &[usize], window: Range<usize>) -> bool {
        let placeholders = &mut self.blocks.placeholders;
        for &i in done {
            placeholders[i] = false;
        }
        placeholders[window].iter().any(|p| *p)
    }

    pub(super) fn set_zoom(&mut self, zoom: f64) {
        self.zoom = zoom;
    }

    pub(super) fn clear_pending(&mut self) {
        self.pending = None;
    }

    pub(super) fn set_pending(&mut self, pending: Option<(Pending, String)>) {
        self.pending = pending;
    }

    /// The user edited the preview itself.
    pub(super) fn note_preview_edit(&mut self, dirty: (Range<Shown>, isize)) {
        self.dirty_preview = Some(dirty);
    }

    pub(super) fn scroll_spans(&self) -> Option<Rc<Vec<ScrollSpan>>> {
        self.scroll_spans.clone()
    }

    pub(super) fn remember_scroll_spans(&mut self, spans: Rc<Vec<ScrollSpan>>) {
        self.scroll_spans = Some(spans);
    }

    /// Brings everything known about the document in line with the text of `snap`, parsing it here unless the
    /// background thread did: doc, layout, index and blocks belong together again when this returns. `resolver` says
    /// what is rendered already, for the text it is given.
    pub(super) fn advance<R: Resolver>(
        &mut self,
        snap: &Snapshot,
        views: ViewFacts,
        background: Option<Background>,
        resolver: impl FnOnce(&str) -> R,
    ) -> Update {
        let (parsed, layout_done) = match background {
            Some(background) => {
                self.parse_failed = background.failed;
                (Some(background.parsed), background.layout)
            }
            None if self.is_current(snap) => (None, None),
            None => (Some(self.parse(&snap.text)), None),
        };
        self.take_over(snap, views, parsed, layout_done, resolver)
    }

    /// Like `advance`, if a text of moderate size can be parsed around its edit right away; else `None`, and the next
    /// update parses all of it.
    pub(super) fn advance_in_part<R: Resolver>(
        &mut self,
        snap: &Snapshot,
        views: ViewFacts,
        resolver: impl FnOnce(&str) -> R,
    ) -> Option<Update> {
        // Something that must be rebuilt as a whole anyway would make the part wasted work.
        let whole = self.stale.colors || (views.visible && self.stale.layout);
        if whole || snap.text.len() > PARTS_ON_MAIN_UP_TO || self.is_current(snap) {
            return None;
        }
        let parsed = self.parse_part(&snap.text)?;
        self.parse_failed = false;
        Some(self.take_over(snap, views, Some(parsed), None, resolver))
    }

    /// Parses `new_text` around the edit; `None` when its neighbours would not stay the same. A panic takes the tree
    /// apart, and commands must not find it empty, so it is read anew then.
    fn parse_part(&mut self, new_text: &str) -> Option<Parsed> {
        let parsed = contained(|| {
            panic_if_a_test_asks();
            reparse_part_and_color(&mut self.doc, &self.spans, new_text)
        });
        match parsed {
            Some(parsed) => parsed,
            None => {
                let in_step = !self.preview_is_behind();
                let text = self.doc.shared_text();
                self.doc =
                    contained(|| parse_shared_and_color(text.clone()).doc).unwrap_or_else(|| Parsed::plain(text).doc);
                if in_step {
                    self.layout.doc_version = self.doc.version();
                }
                None
            }
        }
    }

    /// Parses `new_text`, around the edit where that is possible.
    fn parse(&mut self, new_text: &str) -> Parsed {
        let parsed = contained(|| {
            panic_if_a_test_asks();
            reparse_and_color(&mut self.doc, &self.spans, new_text)
        });
        self.parse_failed = parsed.is_none();
        parsed.unwrap_or_else(|| Parsed::plain(new_text.into()))
    }

    fn take_over<R: Resolver>(
        &mut self,
        snap: &Snapshot,
        views: ViewFacts,
        parsed: Option<Parsed>,
        layout_done: Option<(Layout, Index)>,
        resolver: impl FnOnce(&str) -> R,
    ) -> Update {
        let text_changed = parsed.is_some();
        let splice = parsed.as_ref().and_then(|p| p.splice.clone());
        let recolor = self.adopt(snap, parsed);
        let appearance_switched = self.rendered_dark != Some(views.dark);
        let caret = views.caret_at.map(|at| snap.byte_at(at));
        let preview = self.lay_out(views, caret, text_changed, layout_done, splice.as_ref(), resolver);
        Update {
            recolor,
            preview,
            text_changed,
            appearance_switched,
        }
    }

    /// Takes over a new parse result for the text of `snap`; what to recolor, unless nothing changed.
    fn adopt(&mut self, snap: &Snapshot, parsed: Option<Parsed>) -> Option<Recolor> {
        self.version = snap.version;
        let all = std::mem::take(&mut self.stale.colors);
        let old = parsed.map(|parsed| {
            let old_doc = std::mem::replace(&mut self.doc, parsed.doc);
            let old_text = old_doc.shared_text();
            // Freeing a large document tree takes long enough to be felt while typing.
            free_in_background(old_doc);
            (old_text, std::mem::replace(&mut self.spans, Arc::new(parsed.spans)))
        });
        let (old_text, old_spans) = match old {
            Some(old) if !all => old,
            None if !all => return None,
            _ => (Arc::default(), Arc::default()),
        };
        Some(Recolor {
            old_text,
            old_spans,
            text: self.doc.shared_text(),
            spans: self.spans.clone(),
        })
    }

    /// Lays out the text and finds the blocks that changed; `None` when the preview stays as it is.
    fn lay_out<R: Resolver>(
        &mut self,
        views: ViewFacts,
        caret: Option<Byte>,
        text_changed: bool,
        layout_done: Option<(Layout, Index)>,
        splice: Option<&Splice>,
        resolver: impl FnOnce(&str) -> R,
    ) -> Option<PreviewChange> {
        let stale_layout = std::mem::take(&mut self.stale.layout);
        let work = preview_work(PreviewFacts {
            text_changed,
            caret_moved: caret != self.caret,
            preview_edited: self.dirty_preview.is_some(),
            shows_blocks: !self.blocks.keys.is_empty(),
            stale_layout,
            visible: views.visible,
            has_caret: caret.is_some(),
            background_layout: layout_done,
            partial_parse: splice,
        });
        let source = match work {
            PreviewWork::Keep => return None,
            PreviewWork::Postpone => {
                // Showing the preview again lays it out anew.
                self.caret = None;
                self.stale.layout = true;
                self.scroll_spans = None;
                return None;
            }
            PreviewWork::Lay(source) => source,
        };
        let old_caret = std::mem::replace(&mut self.caret, caret);
        let images = resolver(self.doc.text());
        let (layout, index) = match source {
            LayoutSource::Background(done, index) => (Some(done), Some(index)),
            LayoutSource::Partial(splice) => {
                let old = std::mem::take(&mut self.layout);
                let reuse = reusable(splice, &self.doc, old_caret);
                (contained(|| relayout(&self.doc, &images, caret, old, &reuse)), None)
            }
            LayoutSource::Full => (laid_out(&self.doc, &images, caret), None),
        };
        self.mode = mode_of(self.parse_failed || layout.is_none(), self.doc.text());
        let layout = layout.unwrap_or_else(|| Layout::nothing_of(&self.doc));
        let index = index.unwrap_or_else(|| Index::new(&layout));
        let keys: Vec<u64> = layout.blocks.iter().map(|b| b.key).collect();
        let lens: Vec<usize> = layout.blocks.iter().map(|b| b.len_u16).collect();
        let diff = self.blocks.diff(&keys, self.dirty_preview.take(), views.shown_len);
        let changed = diff.prefix..keys.len() - diff.suffix;
        let blocks = layout.blocks[changed.clone()].to_vec();
        self.blocks.replace(&diff, keys, lens, &changed);
        free_in_background((
            std::mem::replace(&mut self.index, index),
            std::mem::replace(&mut self.layout, layout),
        ));
        self.scroll_spans = None;
        self.rendered_dark = Some(views.dark);
        Some(PreviewChange {
            blocks,
            diff,
            mode: self.mode,
            zoom: self.zoom,
        })
    }

    /// The preview view now shows the last change, with the blocks of `changed` outside `eager` as plain text; whether
    /// any are left to render.
    pub(super) fn preview_shown(&mut self, changed: Range<usize>, eager: &Range<usize>) -> bool {
        self.blocks.left_plain(changed, eager);
        self.blocks.pending()
    }
}

pub(super) fn shows_image(block: &TopBlock) -> bool {
    images_of(block).next().is_some()
}

pub(super) fn images_of(block: &TopBlock) -> impl Iterator<Item = &str> {
    use crate::preview::layout::{Atom, RunMap};
    block.paras.iter().flat_map(|p| &p.runs).filter_map(|r| match &r.map {
        RunMap::Atom {
            atom: Atom::Image(image),
            ..
        } => Some(&*image.url),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::super::editing::bridge;
    use super::*;
    use crate::markdown::doc::DiagramKind;
    use crate::preview::layout::NoImages;

    fn quiet<'a>() -> PreviewFacts<'a> {
        PreviewFacts {
            text_changed: false,
            caret_moved: false,
            preview_edited: false,
            shows_blocks: true,
            stale_layout: false,
            visible: true,
            has_caret: false,
            background_layout: None,
            partial_parse: None,
        }
    }

    fn splice() -> Splice {
        Splice {
            old: 0..1,
            new: 0..1,
            delta: 0,
            window: Byte::new(0)..Byte::new(1),
            behind: Byte::new(1),
        }
    }

    fn lays_out_all(work: &PreviewWork<'_>) -> bool {
        matches!(work, PreviewWork::Lay(LayoutSource::Full))
    }

    #[test]
    fn the_preview_stays_while_nothing_it_shows_changed() {
        assert!(matches!(preview_work(quiet()), PreviewWork::Keep));
        for f in [
            PreviewFacts {
                text_changed: true,
                ..quiet()
            },
            PreviewFacts {
                caret_moved: true,
                ..quiet()
            },
            PreviewFacts {
                preview_edited: true,
                ..quiet()
            },
            PreviewFacts {
                shows_blocks: false,
                ..quiet()
            },
            PreviewFacts {
                stale_layout: true,
                ..quiet()
            },
        ] {
            assert!(lays_out_all(&preview_work(f)));
        }
    }

    #[test]
    fn a_hidden_preview_waits_unless_it_was_edited() {
        let hidden = || PreviewFacts {
            text_changed: true,
            visible: false,
            ..quiet()
        };
        assert!(matches!(preview_work(hidden()), PreviewWork::Postpone));
        let edited = PreviewFacts {
            preview_edited: true,
            ..hidden()
        };
        assert!(lays_out_all(&preview_work(edited)));
    }

    #[test]
    fn the_cheapest_layout_that_is_still_right_wins() {
        let changed = || PreviewFacts {
            text_changed: true,
            ..quiet()
        };
        let background = || PreviewFacts {
            background_layout: Some((Layout::default(), Index::default())),
            ..changed()
        };
        assert!(matches!(
            preview_work(background()),
            PreviewWork::Lay(LayoutSource::Background(..))
        ));
        let caret = PreviewFacts {
            has_caret: true,
            ..background()
        };
        assert!(lays_out_all(&preview_work(caret)));
        let edit = splice();
        let partial = || PreviewFacts {
            partial_parse: Some(&edit),
            ..changed()
        };
        assert!(matches!(
            preview_work(partial()),
            PreviewWork::Lay(LayoutSource::Partial(_))
        ));
        let arrived = PreviewFacts {
            stale_layout: true,
            ..partial()
        };
        assert!(
            lays_out_all(&preview_work(arrived)),
            "a splice knows nothing of arrivals"
        );
    }

    fn snapshot(text: &str, version: u64) -> Snapshot {
        Snapshot::new(text.to_string(), text.encode_utf16().count(), version)
    }

    fn views(state: &State, visible: bool) -> ViewFacts {
        ViewFacts {
            caret_at: None,
            visible,
            shown_len: state.blocks.lens.iter().sum(),
            dark: false,
        }
    }

    fn update(state: &mut State, text: &str, version: u64) -> Update {
        let facts = views(state, true);
        state.advance(&snapshot(text, version), facts, None, |_| NoImages)
    }

    /// The state as an update leaves it: `text` parsed and its preview shown.
    fn shown(text: &str) -> State {
        let mut state = State::default();
        let _ = update(&mut state, text, 1);
        state
    }

    fn belongs_together(state: &State) {
        assert_eq!(state.layout.doc_version, state.doc.version());
        assert_eq!(state.index.block_starts.len(), state.layout.blocks.len());
        assert_eq!(state.blocks.keys.len(), state.layout.blocks.len());
    }

    #[test]
    fn an_update_of_nothing_keeps_the_preview() {
        let mut state = shown("# Titel\n\nText\n");
        assert_eq!(state.layout().blocks.len(), 2);
        assert!(update(&mut state, "# Titel\n\nText\n", 2).preview.is_none());
    }

    #[test]
    fn only_the_changed_block_is_replaced() {
        let mut state = shown("# Titel\n\nText\n\nEnde\n");
        let change = update(&mut state, "# Titel\n\nTexte\n\nEnde\n", 2)
            .preview
            .expect("a new preview");
        assert_eq!((change.diff.prefix, change.diff.suffix), (1, 1));
        assert_eq!(change.blocks.len(), 1);
        assert_eq!(change.blocks[0].paras[0].runs[0].text, "Texte");
        assert_eq!(state.text(), "# Titel\n\nTexte\n\nEnde\n");
        belongs_together(&state);
    }

    #[test]
    fn an_update_in_part_leaves_everything_belonging_together() {
        let old = "Eins\n\nZwei\n\nDrei\n\nVier\n\nFünf\n\nSechs\n\nSieben\n";
        let mut state = shown(old);
        let new = old.replace("Vier", "Viertel");
        let facts = views(&state, true);
        let done = state.advance_in_part(&snapshot(&new, 2), facts, |_| NoImages);
        let change = done.expect("a part").preview.expect("a new preview");
        assert!(change.blocks.iter().any(|b| b.paras[0].runs[0].text == "Viertel"));
        assert_eq!(state.text(), new);
        belongs_together(&state);
        let facts = views(&state, true);
        assert!(
            state.advance_in_part(&snapshot(&new, 3), facts, |_| NoImages).is_none(),
            "no edit to parse"
        );
    }

    #[test]
    fn eager_blocks_are_decided_after_the_update() {
        let mut state = shown("Eins\n");
        let change = update(&mut state, "Zwei\n\nDrei\n\nVier\n", 2)
            .preview
            .expect("a new preview");
        assert_eq!(state.blocks.placeholders, [false, false, false]);
        let changed = change.diff.prefix..change.diff.prefix + change.blocks.len();
        assert!(state.preview_shown(changed, &(0..1)));
        assert_eq!(state.blocks.placeholders, [false, true, true]);
        assert!(state.rendered(&[], 0..2));
        assert!(!state.rendered(&[1], 0..2));
        assert!(!state.rendered(&[2], 0..3));
        assert!(!state.blocks.pending());
    }

    #[test]
    fn invalidating_marks_what_each_reason_changes() {
        let mut state = shown("Text\n");
        state.set_pending(Some((
            Pending {
                at: Byte::new(0),
                styles: Vec::new(),
            },
            "Text\n".into(),
        )));
        state.invalidate(Invalidate::Arrivals);
        assert!(state.stale.layout && !state.stale.colors);
        assert!(!state.blocks.keys.is_empty(), "arrivals keep what is shown");
        assert!(state.pending().is_some());
        state.invalidate(Invalidate::Preview);
        assert!(state.blocks.keys.is_empty());
        assert!(state.pending().is_some(), "a repaired preview keeps waiting styles");
        state.invalidate(Invalidate::Zoom);
        assert!(state.stale.colors);
        state.invalidate(Invalidate::Text);
        assert!(state.pending().is_none(), "a new text has no waiting styles");
    }

    #[test]
    fn an_allowed_web_image_renders_again_though_its_text_is_unchanged() {
        let mut state = shown("Text\n\n![b](https://a.de/b.png)\n");
        let before = state.blocks.keys.clone();
        state.invalidate(Invalidate::RemoteAllowed);
        assert_eq!(state.blocks.keys[0], before[0]);
        assert_ne!(state.blocks.keys[1], before[1]);
    }

    #[test]
    fn arrived_pictures_mark_only_their_blocks() {
        let mut state = shown("Text\n\n![b](b.png)\n");
        assert!(state.mark_arrived(|block| images_of(block).any(|url| url == "b.png")));
        assert_eq!(state.blocks.placeholders, [false, true]);
        assert!(!state.mark_arrived(|_| false));
    }

    #[test]
    fn a_hidden_preview_catches_up_when_it_shows_again() {
        let mut state = shown("Text\n");
        let facts = views(&state, false);
        let hidden = state.advance(&snapshot("Neu\n", 2), facts, None, |_| NoImages);
        assert!(hidden.preview.is_none());
        assert!(hidden.text_changed);
        assert!(state.stale_layout(), "it must not forget");
        assert!(state.preview_is_behind());
        assert_eq!(state.index.block_starts.len(), state.layout.blocks.len());
        let shown_again = update(&mut state, "Neu\n", 2).preview.expect("a preview");
        assert_eq!(shown_again.blocks[0].paras[0].runs[0].text, "Neu");
        belongs_together(&state);
    }

    #[test]
    fn a_picture_arriving_for_a_hidden_preview_marks_only_its_block() {
        let mut state = shown("Text\n\n![b](b.png)\n");
        let facts = views(&state, false);
        let _ = state.advance(&snapshot("Neu\n\n![b](b.png)\n", 2), facts, None, |_| NoImages);
        assert!(state.mark_arrived(|block| images_of(block).any(|url| url == "b.png")));
        assert_eq!(state.blocks.keys.len(), 2, "what the view shows stays known");
        assert_eq!(state.blocks.placeholders, [false, true]);
    }

    #[test]
    fn a_failed_layout_fails_the_preview_only_until_a_layout_succeeds() {
        let mut state = shown("Formel $x$\n");
        let facts = views(&state, true);
        state.invalidate(Invalidate::Layout);
        let failed = state.advance(&snapshot("Formel $x$\n", 1), facts, None, |_| PanicsAtFormulas);
        assert_eq!(failed.preview.expect("the preview says so").mode, PreviewMode::Failed);
        state.invalidate(Invalidate::Zoom);
        let facts = views(&state, true);
        let good = state.advance(&snapshot("Formel $x$\n", 1), facts, None, |_| NoImages);
        assert_eq!(good.preview.expect("a preview").mode, PreviewMode::Shows);
        assert_eq!(state.preview_mode(), PreviewMode::Shows);
    }

    #[test]
    fn the_bridge_opens_only_while_the_layout_belongs_to_the_document() {
        let opens =
            |state: &State| crate::preview::bridge::Bridge::new(state.doc(), state.layout(), state.index()).is_some();
        let mut state = shown("Text\n");
        assert!(opens(&state));
        let facts = views(&state, false);
        let _ = state.advance(&snapshot("Neu\n", 2), facts, None, |_| NoImages);
        assert!(!opens(&state), "the hidden preview has not caught up");
        let _ = update(&mut state, "Neu\n", 2);
        assert!(opens(&state));
    }

    #[test]
    fn a_failed_parse_keeps_the_text_of_the_snapshot() {
        let mut state = shown("Text\n");
        let snap = snapshot("Neu\n", 2);
        state.adopt(&snap, Some(Parsed::plain("Neu\n".into())));
        assert_eq!(state.text(), "Neu\n");
        assert!(state.doc().blocks.is_empty());
        assert!(state.is_current(&snapshot("Neu\n", 9)));
    }

    #[test]
    fn colors_change_where_the_text_did_unless_all_are_stale() {
        let mut state = shown("Text\n");
        let snap = snapshot("Neu\n", 2);
        let parsed = state.parse("Neu\n");
        let recolor = state.adopt(&snap, Some(parsed)).expect("a change");
        assert_eq!(&*recolor.old_text, "Text\n");
        assert_eq!(&*recolor.text, "Neu\n");
        assert!(state.adopt(&snap, None).is_none(), "nothing changed");
        state.invalidate(Invalidate::Zoom);
        let recolor = state.adopt(&snap, None).expect("all are stale");
        assert_eq!(&*recolor.old_text, "", "from nothing, so every color applies");
    }

    #[test]
    fn a_text_that_stays_plain_has_no_layout() {
        let doc = Parsed::plain("|\n".repeat(8 << 20).into()).doc;
        assert!(laid_out(&doc, &NoImages, None).unwrap().blocks.is_empty());
    }

    /// Answers like `NoImages`, except that the layout panics at the first formula.
    struct PanicsAtFormulas;

    impl Resolver for PanicsAtFormulas {
        fn math_ready(&self, _: &str, _: bool) -> bool {
            panic!("a test asked for this panic")
        }
        fn diagram_ready(&self, _: &str, _: DiagramKind) -> bool {
            false
        }
    }

    fn with_panicking_parser<R>(f: impl FnOnce() -> R) -> R {
        PARSING_PANICS.set(true);
        let result = f();
        PARSING_PANICS.set(false);
        result
    }

    #[test]
    fn a_panicking_parse_fails_the_preview_until_good_text_comes() {
        let mut state = shown("Text\n");
        let failed = with_panicking_parser(|| update(&mut state, "Neu\n", 2));
        assert_eq!(state.preview_mode(), PreviewMode::Failed);
        assert_eq!(state.text(), "Neu\n", "the text stays");
        assert!(state.doc().blocks.is_empty());
        let change = failed.preview.expect("the preview says so");
        assert_eq!(change.mode, PreviewMode::Failed);
        belongs_together(&state);
        assert!(bridge(&state).is_none(), "nothing shown stands for the text");
        let good = update(&mut state, "Gut\n", 3).preview.expect("a preview");
        assert_eq!(good.mode, PreviewMode::Shows);
        assert_eq!(state.preview_mode(), PreviewMode::Shows);
        assert!(bridge(&state).is_some());
        assert_eq!(good.blocks[0].paras[0].runs[0].text, "Gut");
        belongs_together(&state);
    }

    #[test]
    fn a_panicking_part_leaves_the_state_whole() {
        let old = "Eins\n\nZwei\n\nDrei\n\nVier\n\nFünf\n\nSechs\n\nSieben\n";
        let mut state = shown(old);
        let blocks = state.doc().blocks.len();
        let new = old.replace("Vier", "Viertel");
        let facts = views(&state, true);
        let part = with_panicking_parser(|| state.advance_in_part(&snapshot(&new, 2), facts, |_| NoImages));
        assert!(part.is_none());
        assert_eq!(state.text(), old);
        assert_eq!(state.doc().blocks.len(), blocks, "commands must not find it empty");
        belongs_together(&state);
    }

    #[test]
    fn a_panicking_layout_leaves_nothing_behind() {
        let mut state = shown("Eins\n\nZwei\n\nDrei\n\nVier\n\nFünf\n\nSechs\n\nSieben\n");
        let facts = views(&state, true);
        let new = "Eins\n\nZwei\n\nDrei\n\n$x$\n\nFünf\n\nSechs\n\nSieben\n";
        let update = state.advance(&snapshot(new, 2), facts, None, |_| PanicsAtFormulas);
        assert!(!update.preview.expect("the preview says so").diff.replace.is_empty());
        assert_eq!(state.preview_mode(), PreviewMode::Failed);
        assert!(state.layout.blocks.is_empty());
        assert!(state.index.paras.is_empty() && state.index.block_starts.is_empty());
        assert_eq!(state.text(), new);
        belongs_together(&state);
    }

    #[test]
    fn a_text_that_stays_plain_is_too_large_for_the_preview() {
        let mut state = shown("Text\n");
        let huge = "|\n".repeat(8 << 20);
        let change = update(&mut state, &huge, 2).preview.expect("the preview says so");
        assert_eq!(change.mode, PreviewMode::TooLarge);
        assert_eq!(state.preview_mode(), PreviewMode::TooLarge);
        assert!(change.blocks.is_empty());
        belongs_together(&state);
        let back = update(&mut state, "Text\n", 3).preview.expect("a preview");
        assert_eq!(back.mode, PreviewMode::Shows);
    }

    #[test]
    fn a_footnote_is_found_by_its_name_where_the_preview_shows_it() {
        let state = shown("Text[^a] und[^b].\n\n[^a]: Erste.\n\n[^b]: Zweite.\n");
        let (first, first_len) = state.footnote_paragraph("a").expect("a footnote");
        let (second, _) = state.footnote_paragraph("b").expect("a footnote");
        assert!(first < second);
        assert!(first_len > "Erste.".len(), "with its label");
        assert_eq!(state.footnote_paragraph("c"), None);
    }

    #[test]
    fn a_background_result_takes_the_failure_with_it() {
        let mut state = shown("Text\n");
        let parsed = Parsed::plain("Neu\n".into());
        let layout = Layout::nothing_of(&parsed.doc);
        let index = Index::new(&layout);
        let facts = views(&state, true);
        let background = Background {
            parsed,
            layout: Some((layout, index)),
            failed: true,
        };
        let done = state.advance(&snapshot("Neu\n", 2), facts, Some(background), |_| NoImages);
        assert_eq!(done.preview.expect("the preview says so").mode, PreviewMode::Failed);
        belongs_together(&state);
    }
}
