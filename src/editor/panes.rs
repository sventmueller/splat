//! The two text views and the outline beside them. Everything else in the controller reads the views through
//! `ViewReader` and changes them through `Panes::silently`, so a change of a view that the delegate callbacks are meant
//! to hear stands out as `Panes::for_appkit`.

use std::cell::{Cell, OnceCell};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSScrollView, NSSplitViewItem, NSTextInputClient, NSTextView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{NSCoder, NSRange, NSRect, NSString, NSUserDefaults};

use super::layers::ForPanes;
use super::outline::Outline;
use super::views::{PreviewTextView, SourceTextView};
use super::{scroll, string_of};
use crate::util::{ns, perform_later, storage};

/// User defaults key of the frame the last moved or resized document window had.
pub(super) const FRAME_KEY: &str = "DocumentWindowFrame";

/// A little longer than AppKit takes to collapse a pane.
const PANE_ANIMATION_SECONDS: f64 = 0.6;

/// Whether a pane was last asked to show, for as long as its animation runs.
#[derive(Default)]
struct Asked(Cell<Option<bool>>);

impl Asked {
    /// A new request goes against the last one, or against how the pane stands if none is under way: during an
    /// animation, `isCollapsed` does not say yet where the pane ends up.
    fn toggle(&self, collapsed: bool) -> bool {
        let shows = !self.0.get().unwrap_or(!collapsed);
        self.0.set(Some(shows));
        shows
    }

    /// Ends the request, and says what the pane has to become if it stands the other way.
    fn settle(&self, collapsed: bool) -> Option<bool> {
        let shows = self.0.take()?;
        (collapsed == shows).then_some(shows)
    }
}

#[derive(Default)]
pub(super) struct PaneState {
    source: OnceCell<Retained<SourceTextView>>,
    preview: OnceCell<Retained<PreviewTextView>>,
    preview_item: OnceCell<Retained<NSSplitViewItem>>,
    outline: OnceCell<Outline>,
    outline_item: OnceCell<Retained<NSSplitViewItem>>,
    /// Raised while the controller changes its own views; the delegate callbacks then ignore the changes.
    busy: Cell<bool>,
    preview_asked: Asked,
    outline_asked: Asked,
}

/// One of the two text views.
#[derive(Clone, Copy)]
pub(super) enum Side {
    Source,
    Preview,
}

/// The raw text views, for a change of them; see `Panes::silently` and `Panes::for_appkit`.
#[derive(Clone, Copy)]
pub(super) struct Views<'a> {
    pub(super) source: &'a SourceTextView,
    pub(super) preview: &'a PreviewTextView,
}

impl<'a> Views<'a> {
    pub(super) fn of(self, side: Side) -> &'a NSTextView {
        match side {
            Side::Source => self.source,
            Side::Preview => self.preview,
        }
    }
}

/// A text view that can only be asked things. Its layout queries may lay it out, which is not a change of what it holds.
#[derive(Clone, Copy)]
pub(super) struct ViewReader<'a>(&'a NSTextView);

impl<'a> ViewReader<'a> {
    pub(super) fn string(self) -> Retained<NSString> {
        self.0.string()
    }

    pub(super) fn string_and_length(self) -> (String, usize) {
        let string = self.0.string();
        (string_of(&string), string.length())
    }

    pub(super) fn length(self) -> usize {
        storage(self.0).length()
    }

    pub(super) fn selected_range(self) -> NSRange {
        self.0.selectedRange()
    }

    pub(super) fn has_marked_text(self) -> bool {
        self.0.hasMarkedText()
    }

    pub(super) fn visible_rect(self) -> NSRect {
        self.0.visibleRect()
    }

    pub(super) fn bounds(self) -> NSRect {
        self.0.bounds()
    }

    pub(super) fn window(self) -> Option<Retained<NSWindow>> {
        self.0.window()
    }

    pub(super) fn enclosing_scroll_view(self) -> Option<Retained<NSScrollView>> {
        self.0.enclosingScrollView()
    }

    pub(super) fn effective_appearance(self) -> Retained<NSAppearance> {
        self.0.effectiveAppearance()
    }

    /// TextKit 2 lays out only what is in view; asking for `layoutManager` instead would switch the view back for good.
    pub(super) fn uses_text_kit_2(self) -> bool {
        self.0.textLayoutManager().is_some()
    }

    pub(super) fn char_rect(self, index: usize) -> NSRect {
        scroll::char_rect(self.0, index)
    }

    pub(super) fn top_offset(self) -> Option<usize> {
        scroll::top_offset(self.0)
    }

    pub(super) fn offset_at(self, y: f64) -> Option<usize> {
        scroll::offset_at(self.0, y)
    }
}

pub(super) struct Panes<'a> {
    host: ForPanes<'a>,
    state: &'a PaneState,
}

impl<'a> Panes<'a> {
    pub(super) fn new(host: ForPanes<'a>, state: &'a PaneState) -> Self {
        Panes { host, state }
    }
}

impl<'a> Panes<'a> {
    pub(super) fn install_outline(&self, outline: Outline, item: Retained<NSSplitViewItem>) {
        let _ = self.state.outline.set(outline);
        let _ = self.state.outline_item.set(item);
    }

    pub(super) fn install_views(
        &self,
        source: Retained<SourceTextView>,
        preview: Retained<PreviewTextView>,
        preview_item: Retained<NSSplitViewItem>,
    ) {
        let _ = self.state.source.set(source);
        let _ = self.state.preview.set(preview);
        let _ = self.state.preview_item.set(preview_item);
    }

    /// Whether the window is built: AppKit asks for toolbar items and validation before it is.
    pub(super) fn is_built(&self) -> bool {
        self.state.source.get().is_some()
    }

    fn views(&self) -> Views<'a> {
        Views {
            source: self.state.source.get().expect("source view"),
            preview: self.state.preview.get().expect("preview view"),
        }
    }

    pub(super) fn source(&self) -> ViewReader<'a> {
        ViewReader(self.views().source)
    }

    pub(super) fn preview(&self) -> ViewReader<'a> {
        ViewReader(self.views().preview)
    }

    pub(super) fn reader(&self, side: Side) -> ViewReader<'a> {
        ViewReader(self.views().of(side))
    }

    /// The source view once it exists, for what may run at any moment, like the save after a panic.
    pub(super) fn source_if_built(&self) -> Option<ViewReader<'a>> {
        self.state.source.get().map(|source| ViewReader(source))
    }

    /// The controller's own changes to its views go here: for the length of `f`, the delegate callbacks ignore them.
    pub(super) fn silently<R>(&self, f: impl for<'v> FnOnce(Views<'v>) -> R) -> R {
        let views = self.views();
        crate::util::with_raised(&self.state.busy, || f(views))
    }

    /// The widest an image shows in the preview.
    pub(super) fn preview_image_width(&self) -> f64 {
        (self.preview().bounds().size.width - 60.0).max(200.0)
    }

    pub(super) fn is_silent(&self) -> bool {
        self.state.busy.get()
    }

    /// The raw views with the delegate callbacks listening, for handing a view to AppKit as an argument and for
    /// scrolling, which `ScrollSync` has to hear.
    pub(super) fn for_appkit(&self) -> Views<'a> {
        self.views()
    }

    pub(super) fn is_preview(&self, object: Option<&AnyObject>) -> bool {
        let preview: &AnyObject = self.views().preview;
        object.is_some_and(|o| std::ptr::eq(o, preview))
    }

    pub(super) fn is_source(&self, object: Option<&AnyObject>) -> bool {
        let Some(source) = self.state.source.get() else {
            return false;
        };
        let source: &AnyObject = source;
        object.is_some_and(|o| std::ptr::eq(o, source))
    }

    pub(super) fn preview_focused(&self) -> bool {
        let Some(window) = self.host.window() else { return false };
        let responder = window.firstResponder();
        let preview: &AnyObject = self.views().preview;
        responder.is_some_and(|r| std::ptr::eq(&*r as &AnyObject, preview))
    }

    pub(super) fn has_text_focus(&self) -> bool {
        let Some(window) = self.host.window() else { return false };
        let Some(responder) = window.firstResponder() else {
            return false;
        };
        let r: &AnyObject = &responder;
        let views = self.views();
        let source: &AnyObject = views.source;
        let preview: &AnyObject = views.preview;
        std::ptr::eq(r, source) || std::ptr::eq(r, preview)
    }

    pub(super) fn preview_visible(&self) -> bool {
        self.state.preview_item.get().is_some_and(|i| !i.isCollapsed())
    }

    pub(super) fn outline_visible(&self) -> bool {
        self.state.outline_item.get().is_some_and(|i| !i.isCollapsed())
    }

    pub(super) fn toggle_outline(&self) {
        let Some(item) = self.state.outline_item.get() else {
            return;
        };
        if self.toggle_pane(item, &self.state.outline_asked) {
            self.show_headings();
        }
        self.host.object().invalidateRestorableState();
    }

    /// Whether the preview came back and has to catch up.
    pub(super) fn toggle_preview(&self) -> bool {
        let Some(item) = self.state.preview_item.get() else {
            return false;
        };
        let shown = self.toggle_pane(item, &self.state.preview_asked);
        if !shown && self.preview_focused() {
            self.host
                .window()
                .map(|w| w.makeFirstResponder(Some(self.views().source)));
        }
        self.host.object().invalidateRestorableState();
        shown
    }

    /// Asks a pane to go the other way than it was last asked, with its animation, and says whether it will show. Two
    /// requests in one turn of the run loop, or any on a locked screen, end up as the animation of the first one wants, so
    /// `settle` looks when it should be over.
    fn toggle_pane(&self, item: &NSSplitViewItem, asked: &Asked) -> bool {
        let shows = asked.toggle(item.isCollapsed());
        objc2_app_kit::NSAnimatablePropertyContainer::animator(item).setCollapsed(!shows);
        perform_later(self.host.object(), sel!(settlePanes), PANE_ANIMATION_SECONDS, true);
        shows
    }

    /// The animations of the panes are over: each pane is what was asked for last. Whether the preview came back and has
    /// to catch up.
    pub(super) fn settle(&self) -> bool {
        let corrected = |item: &OnceCell<Retained<NSSplitViewItem>>, asked: &Asked| {
            let item = item.get()?;
            let shows = asked.settle(item.isCollapsed())?;
            item.setCollapsed(!shows);
            Some(shows)
        };
        if corrected(&self.state.outline_item, &self.state.outline_asked) == Some(true) {
            self.show_headings();
        }
        corrected(&self.state.preview_item, &self.state.preview_asked) == Some(true)
    }

    /// Shows the current headings in the outline, if it is open; a closed one catches up when it opens.
    pub(super) fn show_headings_if_open(&self) {
        if self.outline_visible() {
            self.show_headings();
        }
    }

    fn show_headings(&self) {
        let Some(outline) = self.state.outline.get() else {
            return;
        };
        let headings: Vec<_> = self.host.model().read(|st| {
            crate::util::contained(|| crate::markdown::commands::heading_anchors(st.doc()))
                .unwrap_or_default()
                .into_iter()
                .map(|h| (h.level, h.title, h.range))
                .collect()
        });
        outline.update(&headings);
    }

    /// Remembers where the window stands and how large it is, for the next window and the next start.
    pub(super) fn remember_frame(&self) {
        let Some(window) = self.host.window() else { return };
        if window.styleMask().contains(NSWindowStyleMask::FullScreen) || !window.isVisible() {
            return;
        }
        // SAFETY: A string is a property list value, as user defaults require.
        unsafe {
            NSUserDefaults::standardUserDefaults()
                .setObject_forKey(Some(&window.stringWithSavedFrame()), &ns(FRAME_KEY))
        };
    }

    pub(super) fn encode_state(&self, coder: &NSCoder) {
        let collapsed = self.state.preview_item.get().is_some_and(|i| i.isCollapsed());
        let outline = self.outline_visible();
        // SAFETY: Window restoration always passes a keyed coder.
        unsafe {
            coder.encodeBool_forKey(collapsed, &ns("previewCollapsed"));
            coder.encodeBool_forKey(outline, &ns("outlineShown"));
            coder.encodeDouble_forKey(self.host.model().read(|st| st.zoom()), &ns("zoom"));
        }
    }

    /// The zoom the window had, if any.
    pub(super) fn restore_state(&self, coder: &NSCoder) -> Option<f64> {
        if let Some(item) = self.state.preview_item.get() {
            // SAFETY: Window restoration always passes a keyed coder.
            item.setCollapsed(unsafe { coder.decodeBoolForKey(&ns("previewCollapsed")) });
        }
        // SAFETY: Window restoration always passes a keyed coder.
        let zoom = unsafe { coder.decodeDoubleForKey(&ns("zoom")) };
        // SAFETY: Window restoration always passes a keyed coder.
        if unsafe { coder.decodeBoolForKey(&ns("outlineShown")) } && !self.outline_visible() {
            self.toggle_outline();
        }
        (zoom > 0.3).then_some(zoom)
    }
}

#[cfg(feature = "selftest")]
impl Panes<'_> {
    pub(super) fn set_outline_collapsed(&self, collapsed: bool) {
        if let Some(item) = self.state.outline_item.get() {
            item.setCollapsed(collapsed);
        }
    }

    pub(super) fn set_preview_collapsed(&self, collapsed: bool) {
        if let Some(item) = self.state.preview_item.get() {
            item.setCollapsed(collapsed);
        }
    }

    /// A pane was asked to move and `settle` has not looked yet.
    pub(super) fn is_moving(&self) -> bool {
        self.state.preview_asked.0.get().is_some() || self.state.outline_asked.0.get().is_some()
    }

    pub(super) fn outline_rows(&self) -> Option<isize> {
        self.state.outline.get().map(|o| o.view.numberOfRows())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_raised_flag_goes_back_to_what_it_was() {
        let flag = Cell::new(false);
        crate::util::with_raised(&flag, || {
            assert!(flag.get());
            crate::util::with_raised(&flag, || assert!(flag.get()));
            assert!(flag.get(), "the inner one must not lower it for the outer one");
        });
        assert!(!flag.get());
    }

    #[test]
    fn a_raised_flag_comes_down_after_a_panic() {
        let flag = Cell::new(false);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::util::with_raised(&flag, || panic!("contained"));
        }));
        assert!(outcome.is_err());
        assert!(!flag.get(), "the callbacks would ignore the user from then on");
    }

    #[test]
    fn requests_during_an_animation_count_against_each_other_not_against_the_pane() {
        let asked = Asked::default();
        assert!(asked.toggle(true), "a collapsed pane is asked to show");
        assert!(
            !asked.toggle(true),
            "still collapsed in its animation, the second request hides it"
        );
        assert!(asked.toggle(false), "and the third shows it, however it stands by then");
    }

    #[test]
    fn a_pane_that_ended_up_as_asked_stays_alone() {
        let asked = Asked::default();
        asked.toggle(true);
        assert_eq!(asked.settle(false), None);
        assert_eq!(asked.settle(true), None, "the request is over");
    }

    #[test]
    fn a_pane_that_ended_up_the_other_way_is_corrected_once() {
        let shown = Asked::default();
        shown.toggle(true);
        assert_eq!(shown.settle(true), Some(true));
        assert_eq!(shown.settle(true), None);

        let hidden = Asked::default();
        hidden.toggle(false);
        assert_eq!(hidden.settle(false), Some(false));
    }

    #[test]
    fn after_settling_a_request_starts_from_how_the_pane_stands() {
        let asked = Asked::default();
        asked.toggle(true);
        asked.settle(false);
        assert!(!asked.toggle(false), "an open pane is asked to hide");
    }
}
