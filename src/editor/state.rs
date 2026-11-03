//! What the controller knows about the document, derived from the source text, and every way that knowledge changes.
//! Its fields are private, so nothing outside this file can leave it half done; no view is touched here, which keeps it
//! testable without a window.

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use super::scroll::ScrollSpan;
use super::style::SourceFonts;
use super::sync::{BlockDiff, Blocks, Snapshot};
use crate::markdown::commands::Pending;
use crate::markdown::doc::Doc;
use crate::markdown::highlight::Style;
use crate::markdown::incremental::{
    Parsed, Splice, parse_and_color, reparse_and_color, reparse_part_and_color, stays_plain,
};
use crate::markdown::text::Shown;
use crate::preview::layout::{Layout, Resolver, TopBlock, layout, relayout, reusable};
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

type Spans = Arc<Vec<(Range<usize>, Style)>>;

#[derive(Default)]
pub(super) struct State {
    text: String,
    version: u64,
    doc: Doc,
    layout: Layout,
    index: Index,
    blocks: Blocks,
    caret: Option<usize>,
    zoom: f64,
    fonts: Option<SourceFonts>,
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
    /// Parsing or laying out this text panicked; the preview says so, and the text stays editable.
    preview_failed: bool,
}

/// What the preview needs after an update.
#[derive(Debug, PartialEq, Eq)]
enum PreviewWork {
    Keep,
    /// The preview is hidden; it is laid out once it shows again.
    Postpone,
    Lay(LayoutSource),
}

#[derive(Debug, PartialEq, Eq)]
enum LayoutSource {
    /// The layout the background thread computed for this text, without a caret.
    Background,
    /// The previous layout, with the blocks outside the changed ones taken over.
    Partial,
    Full,
}

/// What `preview_work` decides on.
struct PreviewFacts {
    text_changed: bool,
    caret_moved: bool,
    /// The user edited the preview itself, so it no longer shows what its blocks say.
    preview_edited: bool,
    shows_blocks: bool,
    /// Formulas or images that arrived meanwhile change blocks the text did not touch.
    stale_layout: bool,
    visible: bool,
    has_caret: bool,
    background_layout: bool,
    partial_parse: bool,
}

fn preview_work(f: &PreviewFacts) -> PreviewWork {
    if !f.text_changed && !f.caret_moved && !f.preview_edited && f.shows_blocks && !f.stale_layout {
        return PreviewWork::Keep;
    }
    if !f.visible && !f.preview_edited {
        return PreviewWork::Postpone;
    }
    PreviewWork::Lay(if f.background_layout && !f.has_caret {
        LayoutSource::Background
    } else if f.partial_parse && !f.stale_layout {
        LayoutSource::Partial
    } else {
        LayoutSource::Full
    })
}

/// A new preview, computed while the state is borrowed and shown after it is released.
pub(super) struct PreviewPlan {
    pub layout: Layout,
    pub index: Option<Index>,
    pub keys: Vec<u64>,
    pub lens: Vec<usize>,
    pub diff: BlockDiff,
    pub zoom: f64,
}

/// What an update knows about the preview and the parse it follows.
pub(super) struct PreviewInput<'a> {
    pub caret: Option<usize>,
    pub text_changed: bool,
    pub layout_done: Option<(Layout, Index)>,
    pub splice: Option<&'a Splice>,
    pub visible: bool,
    pub shown_len: usize,
}

/// The colors of the source to change after a new parse.
pub(super) struct Recolor {
    pub old_text: String,
    pub old_spans: Spans,
    pub spans: Spans,
    pub fonts: Option<SourceFonts>,
}

/// The layout of a text, which has none if it stays plain; `None` if it panicked.
pub(super) fn laid_out(text: &str, doc: &Doc, resolver: &dyn Resolver, caret: Option<usize>) -> Option<Layout> {
    if stays_plain(text) {
        return Some(Layout::default());
    }
    contained(|| layout(text, doc, resolver, caret))
}

impl State {
    pub(super) fn new() -> State {
        State {
            zoom: 1.0,
            ..State::default()
        }
    }

    pub(super) fn text(&self) -> &str {
        &self.text
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

    pub(super) fn caret(&self) -> Option<usize> {
        self.caret
    }

    pub(super) fn zoom(&self) -> f64 {
        self.zoom
    }

    pub(super) fn fonts(&self) -> Option<&SourceFonts> {
        self.fonts.as_ref()
    }

    pub(super) fn rendered_dark(&self) -> Option<bool> {
        self.rendered_dark
    }

    pub(super) fn pending(&self) -> Option<&(Pending, String)> {
        self.pending.as_ref()
    }

    pub(super) fn preview_failed(&self) -> bool {
        self.preview_failed
    }

    pub(super) fn stale_layout(&self) -> bool {
        self.stale.layout
    }

    /// The state was computed from the text `snap` holds; equal versions save comparing the texts.
    pub(super) fn is_current(&self, snap: &Snapshot) -> bool {
        self.version == snap.version || self.text == snap.text
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
                    .invalidate(|i| layout.blocks.get(i).is_some_and(shows_image));
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

    /// For rendering placeholders step by step: the view of the layout and the marks to clear, apart.
    pub(super) fn placeholders(&mut self) -> (&Layout, &Index, &mut Vec<bool>) {
        (&self.layout, &self.index, &mut self.blocks.placeholders)
    }

    pub(super) fn set_fonts(&mut self, zoom: f64, fonts: SourceFonts) {
        self.zoom = zoom;
        self.fonts = Some(fonts);
    }

    pub(super) fn set_preview_failed(&mut self, failed: bool) {
        self.preview_failed = failed;
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

    /// Whether an edit can be parsed in place, right away: not when something else must be rebuilt as a whole anyway.
    pub(super) fn wants_whole_parse(&self, visible: bool) -> bool {
        self.stale.colors || (visible && self.stale.layout)
    }

    /// Parses `new_text` around the edit; `None` when its neighbours would not stay the same. A panic takes the tree
    /// apart, and commands must not find it empty, so it is read anew then.
    pub(super) fn parse_part(&mut self, new_text: &str) -> Option<Parsed> {
        let parsed = contained(|| reparse_part_and_color(&mut self.doc, &self.text, &self.spans, new_text));
        match parsed {
            Some(parsed) => parsed,
            None => {
                self.doc = contained(|| parse_and_color(&self.text).doc).unwrap_or_default();
                None
            }
        }
    }

    /// Parses `new_text`, around the edit where that is possible.
    pub(super) fn parse(&mut self, new_text: &str) -> Parsed {
        let parsed = contained(|| reparse_and_color(&mut self.doc, &self.text, &self.spans, new_text));
        self.preview_failed = parsed.is_none();
        parsed.unwrap_or_else(Parsed::plain)
    }

    /// Takes over a new parse result for the text of `snap`; what to recolor, unless nothing changed.
    pub(super) fn adopt(&mut self, snap: &Snapshot, parsed: Option<Parsed>) -> Option<Recolor> {
        self.version = snap.version;
        let all = std::mem::take(&mut self.stale.colors);
        let old = parsed.map(|parsed| {
            // Freeing a large document tree takes long enough to be felt while typing.
            free_in_background(std::mem::replace(&mut self.doc, parsed.doc));
            (
                std::mem::replace(&mut self.text, snap.text.clone()),
                std::mem::replace(&mut self.spans, Arc::new(parsed.spans)),
            )
        });
        let (old_text, old_spans) = match old {
            Some(old) if !all => old,
            None if !all => return None,
            _ => (String::new(), Arc::default()),
        };
        Some(Recolor {
            old_text,
            old_spans,
            spans: self.spans.clone(),
            fonts: self.fonts.clone(),
        })
    }

    /// Lays out the text and finds the blocks that changed; `None` when the preview stays as it is. `resolver` says what
    /// is rendered already, for the text it is given.
    pub(super) fn plan_preview<R: Resolver>(
        &mut self,
        input: PreviewInput,
        resolver: impl FnOnce(&str) -> R,
    ) -> Option<PreviewPlan> {
        let PreviewInput {
            caret,
            text_changed,
            layout_done,
            splice,
            visible,
            shown_len,
        } = input;
        let stale_layout = std::mem::take(&mut self.stale.layout);
        let work = preview_work(&PreviewFacts {
            text_changed,
            caret_moved: caret != self.caret,
            preview_edited: self.dirty_preview.is_some(),
            shows_blocks: !self.blocks.keys.is_empty(),
            stale_layout,
            visible,
            has_caret: caret.is_some(),
            background_layout: layout_done.is_some(),
            partial_parse: splice.is_some(),
        });
        let source = match work {
            PreviewWork::Keep => return None,
            PreviewWork::Postpone => {
                // The layout no longer matches the text; showing the preview again lays it out anew.
                self.caret = None;
                self.stale.layout = true;
                self.scroll_spans = None;
                return None;
            }
            PreviewWork::Lay(source) => source,
        };
        let old_caret = std::mem::replace(&mut self.caret, caret);
        let images = resolver(&self.text);
        let (layout, index) = match (source, layout_done, splice) {
            (LayoutSource::Background, Some((done, index)), _) => (Some(done), Some(index)),
            (LayoutSource::Partial, _, Some(splice)) => {
                let old = std::mem::take(&mut self.layout);
                let reuse = reusable(splice, &self.doc, old_caret);
                (
                    contained(|| relayout(&self.text, &self.doc, &images, caret, old, &reuse)),
                    None,
                )
            }
            _ => (laid_out(&self.text, &self.doc, &images, caret), None),
        };
        self.preview_failed |= layout.is_none();
        let layout = layout.unwrap_or_default();
        let keys: Vec<u64> = layout.blocks.iter().map(|b| b.key).collect();
        let lens: Vec<usize> = layout.blocks.iter().map(|b| b.len_u16).collect();
        let diff = self.blocks.diff(&keys, self.dirty_preview.take(), shown_len);
        Some(PreviewPlan {
            layout,
            index,
            keys,
            lens,
            diff,
            zoom: self.zoom,
        })
    }

    /// The preview view now shows `plan`, with the blocks outside `eager` as placeholders; whether there are any.
    pub(super) fn adopt_preview(&mut self, plan: PreviewPlan, eager: &Range<usize>, dark: bool) -> bool {
        let index = plan.index.unwrap_or_else(|| Index::new(&plan.layout));
        free_in_background((
            std::mem::replace(&mut self.index, index),
            std::mem::replace(&mut self.layout, plan.layout),
        ));
        self.blocks.replace(&plan.diff, plan.keys, plan.lens, eager);
        self.scroll_spans = None;
        self.rendered_dark = Some(dark);
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
    use super::*;
    use crate::preview::layout::NoImages;

    fn quiet() -> PreviewFacts {
        PreviewFacts {
            text_changed: false,
            caret_moved: false,
            preview_edited: false,
            shows_blocks: true,
            stale_layout: false,
            visible: true,
            has_caret: false,
            background_layout: false,
            partial_parse: false,
        }
    }

    #[test]
    fn the_preview_stays_while_nothing_it_shows_changed() {
        assert_eq!(preview_work(&quiet()), PreviewWork::Keep);
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
            assert_eq!(preview_work(&f), PreviewWork::Lay(LayoutSource::Full));
        }
    }

    #[test]
    fn a_hidden_preview_waits_unless_it_was_edited() {
        let hidden = PreviewFacts {
            text_changed: true,
            visible: false,
            ..quiet()
        };
        assert_eq!(preview_work(&hidden), PreviewWork::Postpone);
        let edited = PreviewFacts {
            preview_edited: true,
            ..hidden
        };
        assert_eq!(preview_work(&edited), PreviewWork::Lay(LayoutSource::Full));
    }

    #[test]
    fn the_cheapest_layout_that_is_still_right_wins() {
        let changed = || PreviewFacts {
            text_changed: true,
            ..quiet()
        };
        let background = PreviewFacts {
            background_layout: true,
            ..changed()
        };
        assert_eq!(preview_work(&background), PreviewWork::Lay(LayoutSource::Background));
        let caret = PreviewFacts {
            has_caret: true,
            ..background
        };
        assert_eq!(preview_work(&caret), PreviewWork::Lay(LayoutSource::Full));
        let partial = PreviewFacts {
            partial_parse: true,
            ..changed()
        };
        assert_eq!(preview_work(&partial), PreviewWork::Lay(LayoutSource::Partial));
        let arrived = PreviewFacts {
            stale_layout: true,
            ..partial
        };
        assert_eq!(
            preview_work(&arrived),
            PreviewWork::Lay(LayoutSource::Full),
            "a splice knows nothing of arrivals"
        );
    }

    /// The state as an update leaves it: `text` parsed and its preview shown.
    fn shown(text: &str) -> State {
        let mut state = State::default();
        update(&mut state, text, 1);
        state
    }

    fn update(state: &mut State, text: &str, version: u64) -> Option<PreviewPlan> {
        let snap = Snapshot::new(text.to_string(), text.encode_utf16().count(), version);
        let parsed = (!state.is_current(&snap) || state.text.is_empty()).then(|| state.parse(text));
        let changed = parsed.is_some();
        let splice = parsed.as_ref().and_then(|p| p.splice.clone());
        state.adopt(&snap, parsed);
        let shown_len = state.blocks.lens.iter().sum();
        let plan = state.plan_preview(
            PreviewInput {
                caret: None,
                text_changed: changed,
                layout_done: None,
                splice: splice.as_ref(),
                visible: true,
                shown_len,
            },
            |_| NoImages,
        )?;
        let changed_blocks = plan.diff.prefix..plan.keys.len() - plan.diff.suffix;
        let keep = PreviewPlan {
            layout: Layout::default(),
            index: None,
            keys: plan.keys.clone(),
            lens: plan.lens.clone(),
            diff: BlockDiff {
                prefix: plan.diff.prefix,
                suffix: plan.diff.suffix,
                replace: plan.diff.replace.clone(),
            },
            zoom: plan.zoom,
        };
        state.adopt_preview(plan, &changed_blocks, false);
        Some(keep)
    }

    #[test]
    fn an_update_of_nothing_keeps_the_preview() {
        let mut state = shown("# Titel\n\nText\n");
        assert_eq!(state.layout().blocks.len(), 2);
        assert!(update(&mut state, "# Titel\n\nText\n", 2).is_none());
    }

    #[test]
    fn only_the_changed_block_is_replaced() {
        let mut state = shown("# Titel\n\nText\n\nEnde\n");
        let plan = update(&mut state, "# Titel\n\nTexte\n\nEnde\n", 2).expect("a new preview");
        assert_eq!((plan.diff.prefix, plan.diff.suffix), (1, 1));
        assert_eq!(state.text(), "# Titel\n\nTexte\n\nEnde\n");
    }

    #[test]
    fn invalidating_marks_what_each_reason_changes() {
        let mut state = shown("Text\n");
        state.set_pending(Some((
            Pending {
                at: 0,
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
        let snap = Snapshot::new("Neu\n".to_string(), 4, 2);
        let parsed = state.parse("Neu\n");
        state.adopt(&snap, Some(parsed));
        let input = |visible, text_changed| PreviewInput {
            caret: None,
            text_changed,
            layout_done: None,
            splice: None,
            visible,
            shown_len: 4,
        };
        assert!(state.plan_preview(input(false, true), |_| NoImages).is_none());
        assert!(state.stale_layout(), "it must not forget");
        let plan = state.plan_preview(input(true, false), |_| NoImages).expect("a preview");
        assert_eq!(plan.layout.blocks[0].paras[0].runs[0].text, "Neu");
    }

    #[test]
    fn colors_change_where_the_text_did_unless_all_are_stale() {
        let mut state = shown("Text\n");
        let snap = Snapshot::new("Neu\n".to_string(), 4, 2);
        let parsed = state.parse("Neu\n");
        let recolor = state.adopt(&snap, Some(parsed)).expect("a change");
        assert_eq!(recolor.old_text, "Text\n");
        assert!(state.adopt(&snap, None).is_none(), "nothing changed");
        state.invalidate(Invalidate::Zoom);
        let recolor = state.adopt(&snap, None).expect("all are stale");
        assert_eq!(recolor.old_text, "", "from nothing, so every color applies");
    }

    #[test]
    fn a_text_that_stays_plain_has_no_layout() {
        let huge = "|\n".repeat(8 << 20);
        let doc = Doc::default();
        assert!(laid_out(&huge, &doc, &NoImages, None).unwrap().blocks.is_empty());
    }
}
