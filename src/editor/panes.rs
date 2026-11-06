//! The two text views and the outline beside them. Everything else in the controller reads the views through
//! `ViewReader` and changes them through `Panes::silently`, so a change of a view that the delegate callbacks are meant
//! to hear stands out as `Panes::for_appkit`.

use std::cell::{Cell, OnceCell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSLayoutConstraint, NSLayoutXAxisAnchor, NSLayoutYAxisAnchor,
    NSScrollView, NSSplitViewItem, NSTextInputClient, NSTextView, NSView, NSWindowStyleMask,
};
use objc2_foundation::{NSArray, NSCoder, NSEdgeInsets, NSPoint, NSRange, NSRect, NSSize, NSString, NSUserDefaults};

use super::layers::ForPanes;
use super::sidebar::{Sidebar, SidebarTab};
use super::views::{PreviewTextView, SourceTextView};
use super::{geometry, string_of};
use crate::util::{ns, storage};

/// User defaults key of the frame the last moved or resized document window had.
pub(super) const FRAME_KEY: &str = "DocumentWindowFrame";

/// User defaults keys of whether the sidebar last showed and on which tab, which the next window opens with.
const SIDEBAR_SHOWN_KEY: &str = "SidebarShown";
const SIDEBAR_TAB_KEY: &str = "SidebarTab";

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
    source_item: OnceCell<Retained<NSSplitViewItem>>,
    preview_item: OnceCell<Retained<NSSplitViewItem>>,
    sidebar: OnceCell<Sidebar>,
    outline_item: OnceCell<Retained<NSSplitViewItem>>,
    /// How this window last left its sidebar in the user defaults.
    remembered: Cell<Option<(bool, SidebarTab)>>,
    /// AppKit's animation of the sidebar takes all its width from the preview; while it runs, this holds source and
    /// preview at the share they had.
    share: RefCell<Option<Retained<NSLayoutConstraint>>>,
    /// Raised while the controller changes its own views; the delegate callbacks then ignore the changes.
    busy: Cell<bool>,
    preview_asked: Asked,
    outline_asked: Asked,
}

/// One of the two text views.
#[derive(Clone, Copy, PartialEq, Eq)]
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

/// A text view that can only be asked things, as can its scroll view. Its layout queries may lay it out, which is not a
/// change of what it holds.
#[derive(Clone, Copy)]
pub(super) struct ViewReader<'a>(&'a NSTextView);

/// The edges of a pane's scroll view that what floats in a corner of it keeps to.
pub(super) struct Edges {
    /// The trailing edge of the scroll view.
    pub(super) trailing: Retained<NSLayoutXAxisAnchor>,
    /// The bottom edge of the scroll view.
    pub(super) bottom: Retained<NSLayoutYAxisAnchor>,
}

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

    /// Whether the view's window is the key window.
    pub(super) fn in_key_window(self) -> bool {
        self.0.window().is_some_and(|window| window.isKeyWindow())
    }

    /// Where the view is scrolled to, as the origin of what its clip view shows.
    pub(super) fn scroll_origin(self) -> Option<NSPoint> {
        Some(self.0.enclosingScrollView()?.contentView().bounds().origin)
    }

    /// Whether `object` is the clip view of the view's scroll view, as a notification of its bounds names it.
    pub(super) fn is_clip_view(self, object: &AnyObject) -> bool {
        self.0
            .enclosingScrollView()
            .is_some_and(|scroll| std::ptr::eq(&*scroll.contentView() as &AnyObject, object))
    }

    /// How much of the pane a scroller takes away from the text; an overlay scroller takes nothing.
    pub(super) fn scroller_room(self) -> f64 {
        self.0.enclosingScrollView().map_or(0.0, |scroll| {
            (scroll.bounds().size.width - scroll.documentVisibleRect().size.width).max(0.0)
        })
    }

    pub(super) fn effective_appearance(self) -> Retained<NSAppearance> {
        self.0.effectiveAppearance()
    }

    pub(super) fn char_rect(self, index: usize) -> NSRect {
        geometry::char_rect(self.0, index)
    }

    /// A rectangle of the view in the coordinates of `other`.
    pub(super) fn rect_in(self, rect: NSRect, other: &objc2_app_kit::NSView) -> NSRect {
        self.0.convertRect_toView(rect, Some(other))
    }

    pub(super) fn top_offset(self) -> Option<usize> {
        geometry::top_offset(self.0)
    }

    pub(super) fn offset_at(self, y: f64) -> Option<usize> {
        geometry::offset_at(self.0, y)
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
    /// Opens the sidebar as the last window left it.
    pub(super) fn install_sidebar(&self, sidebar: Sidebar, item: Retained<NSSplitViewItem>) {
        let _ = self.state.sidebar.set(sidebar);
        let _ = self.state.outline_item.set(item);
        let defaults = NSUserDefaults::standardUserDefaults();
        let tab = SidebarTab::of_segment(defaults.integerForKey(&ns(SIDEBAR_TAB_KEY)));
        self.put_sidebar(defaults.boolForKey(&ns(SIDEBAR_SHOWN_KEY)), tab);
    }

    pub(super) fn install_views(
        &self,
        source: Retained<SourceTextView>,
        preview: Retained<PreviewTextView>,
        items: [Retained<NSSplitViewItem>; 2],
    ) {
        let [source_item, preview_item] = items;
        let _ = self.state.source.set(source);
        let _ = self.state.preview.set(preview);
        let _ = self.state.source_item.set(source_item);
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

    /// Scrolls the view of `side` to `origin` as the controller's own change.
    pub(super) fn scroll_silently(&self, side: Side, origin: NSPoint) {
        self.silently(|views| {
            if let Some(scroll) = views.of(side).enclosingScrollView() {
                let clip = scroll.contentView();
                clip.scrollToPoint(origin);
                scroll.reflectScrolledClipView(&clip);
            }
        });
    }

    /// Lets the end of the text of `side` scroll up by `room`, clear of what floats over the bottom of its pane, while the
    /// rest still scrolls beneath it.
    pub(super) fn leave_room_below(&self, side: Side, room: f64) {
        self.silently(|views| {
            let Some(scroll) = views.of(side).enclosingScrollView() else {
                return;
            };
            if (scroll.contentInsets().bottom - room).abs() < 0.5 {
                return;
            }
            scroll.setAutomaticallyAdjustsContentInsets(false);
            scroll.setContentInsets(NSEdgeInsets {
                top: 0.0,
                left: 0.0,
                bottom: room,
                right: 0.0,
            });
        });
    }

    /// Leaves `room` above the preview's scroll view in its pane, for the bar that stands there.
    pub(super) fn leave_room_above_preview(&self, room: f64) {
        self.silently(|views| {
            let Some(scroll) = views.preview.enclosingScrollView() else {
                return;
            };
            // SAFETY: The result is retained on return, and a pane's scroll view stays where the window put it.
            let Some(pane) = (unsafe { scroll.superview() }) else {
                return;
            };
            let bounds = pane.bounds();
            let height = (bounds.size.height - room).max(0.0);
            scroll.setFrame(NSRect::new(bounds.origin, NSSize::new(bounds.size.width, height)));
        });
    }

    /// The pane of `side`, which holds its scroll view and what floats beside it.
    fn pane(&self, side: Side) -> Option<(Retained<NSScrollView>, Retained<NSView>)> {
        let scroll = self.views().of(side).enclosingScrollView()?;
        // SAFETY: The result is retained on return, and a pane's scroll view stays where the window put it.
        let pane = unsafe { scroll.superview() }?;
        Some((scroll, pane))
    }

    /// Whether `view` floats in the pane of `side`.
    pub(super) fn floats_beside(&self, side: Side, view: &NSView) -> bool {
        // SAFETY: The result is retained on return, and only the main thread changes this view's place.
        let parent = unsafe { view.superview() };
        self.pane(side)
            .zip(parent)
            .is_some_and(|((_, pane), parent)| std::ptr::eq(&*pane, &*parent))
    }

    /// Puts `view` into the pane of `side`, beside the scroll view, which lays out only what it knows of, with the
    /// constraints `place` makes from the scroll view's edges; a view that floats there already stays as it is.
    pub(super) fn float_beside(
        &self,
        side: Side,
        view: &NSView,
        place: impl FnOnce(&Edges) -> Vec<Retained<NSLayoutConstraint>>,
    ) {
        if self.floats_beside(side, view) {
            return;
        }
        let Some((scroll, pane)) = self.pane(side) else {
            return;
        };
        pane.addSubview(view);
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        let edges = Edges {
            trailing: scroll.trailingAnchor(),
            bottom: scroll.bottomAnchor(),
        };
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&place(&edges)));
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

    pub(super) fn sidebar_visible(&self) -> bool {
        self.state.outline_item.get().is_some_and(|i| !i.isCollapsed())
    }

    pub(super) fn sidebar_tab(&self) -> Option<SidebarTab> {
        self.state.sidebar.get().map(Sidebar::tab)
    }

    pub(super) fn with_sidebar(&self, f: impl FnOnce(&Sidebar)) {
        if let Some(sidebar) = self.state.sidebar.get() {
            f(sidebar);
        }
    }

    pub(super) fn outline_visible(&self) -> bool {
        self.tab_visible(SidebarTab::Outline)
    }

    /// Whether the sidebar shows, on this tab.
    pub(super) fn tab_visible(&self, tab: SidebarTab) -> bool {
        self.sidebar_visible() && self.sidebar_tab() == Some(tab)
    }

    pub(super) fn toggle_outline(&self) {
        self.toggle_tab(SidebarTab::Outline);
    }

    /// Shows the sidebar on `tab`; if it shows that already, hides it. Whether it will show.
    pub(super) fn toggle_tab(&self, tab: SidebarTab) -> bool {
        let Some(item) = self.state.outline_item.get() else {
            return false;
        };
        let shown_elsewhere = self.sidebar_visible() && self.sidebar_tab() != Some(tab);
        let shows = shown_elsewhere || self.toggle_sidebar_pane(item);
        if shows {
            self.select_tab(tab);
        }
        self.host.object().invalidate_restorable_state();
        shows
    }

    /// Shows the sidebar on `tab`, whatever it showed.
    pub(super) fn show_tab(&self, tab: SidebarTab) {
        let Some(item) = self.state.outline_item.get() else {
            return;
        };
        if !self.sidebar_visible() {
            self.toggle_sidebar_pane(item);
        }
        self.select_tab(tab);
        self.host.object().invalidate_restorable_state();
    }

    fn select_tab(&self, tab: SidebarTab) {
        self.with_sidebar(|sidebar| sidebar.select(tab));
        if tab == SidebarTab::Outline {
            self.show_headings();
        }
        self.remember_sidebar();
    }

    /// Shows or hides the sidebar on `tab` at once, as a window opens or comes back.
    fn put_sidebar(&self, shown: bool, tab: SidebarTab) {
        let Some(item) = self.state.outline_item.get() else {
            return;
        };
        self.with_sidebar(|sidebar| sidebar.select(tab));
        item.setCollapsed(!shown);
        self.state.remembered.set(Some((shown, tab)));
        self.show_headings_if_open();
    }

    /// Writes how the sidebar stands to the user defaults if that changed, for the next window and the next start.
    pub(super) fn remember_sidebar(&self) {
        let now = (self.sidebar_visible(), self.sidebar_tab().unwrap_or_default());
        if self.state.remembered.replace(Some(now)) == Some(now) {
            return;
        }
        let defaults = NSUserDefaults::standardUserDefaults();
        defaults.setBool_forKey(now.0, &ns(SIDEBAR_SHOWN_KEY));
        defaults.setInteger_forKey(now.1.segment(), &ns(SIDEBAR_TAB_KEY));
    }

    fn toggle_sidebar_pane(&self, item: &NSSplitViewItem) -> bool {
        self.hold_share();
        self.toggle_pane(item, &self.state.outline_asked)
    }

    /// Keeps source and preview at their share of the width until the panes settle.
    fn hold_share(&self) {
        let (Some(source), Some(preview)) = (self.state.source_item.get(), self.state.preview_item.get()) else {
            return;
        };
        if preview.isCollapsed() || self.state.share.borrow().is_some() {
            return;
        }
        let mtm = self.host.mtm();
        let (left, right) = (source.viewController(mtm).view(), preview.viewController(mtm).view());
        let (left_width, right_width) = (left.frame().size.width, right.frame().size.width);
        if left_width <= 0.0 || right_width <= 0.0 {
            return;
        }
        let share = left
            .widthAnchor()
            .constraintEqualToAnchor_multiplier(&right.widthAnchor(), left_width / right_width);
        share.setPriority(source.holdingPriority().max(preview.holdingPriority()) + 1.0);
        share.setActive(true);
        *self.state.share.borrow_mut() = Some(share);
    }

    fn release_share(&self) {
        if let Some(share) = self.state.share.take() {
            share.setActive(false);
        }
    }

    /// Whether the preview came back and has to catch up.
    pub(super) fn toggle_preview(&self) -> bool {
        let Some(item) = self.state.preview_item.get() else {
            return false;
        };
        // A share with a collapsed preview would squeeze the source.
        self.release_share();
        let shown = self.toggle_pane(item, &self.state.preview_asked);
        if !shown && self.preview_focused() {
            self.host
                .window()
                .map(|w| w.makeFirstResponder(Some(self.views().source)));
        }
        self.host.object().invalidate_restorable_state();
        shown
    }

    /// Asks a pane to go the other way than it was last asked, with its animation, and says whether it will show. Two
    /// requests in one turn of the run loop, or any on a locked screen, end up as the animation of the first one wants, so
    /// `settle` looks when it should be over.
    fn toggle_pane(&self, item: &NSSplitViewItem, asked: &Asked) -> bool {
        let shows = asked.toggle(item.isCollapsed());
        if crate::test_hooks::ANIMATES {
            objc2_app_kit::NSAnimatablePropertyContainer::animator(item).setCollapsed(!shows);
        } else {
            item.setCollapsed(!shows);
        }
        self.host
            .object()
            .perform_later(sel!(settlePanes), PANE_ANIMATION_SECONDS, true);
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
        if corrected(&self.state.outline_item, &self.state.outline_asked) == Some(true) && self.outline_visible() {
            self.show_headings();
        }
        self.release_share();
        self.remember_sidebar();
        corrected(&self.state.preview_item, &self.state.preview_asked) == Some(true)
    }

    /// Shows the current headings in the outline, if it is open; a closed one catches up when it opens.
    pub(super) fn show_headings_if_open(&self) {
        if self.outline_visible() {
            self.show_headings();
        }
    }

    fn show_headings(&self) {
        let Some(sidebar) = self.state.sidebar.get() else {
            return;
        };
        let headings: Vec<_> = self.host.model().read(|st| {
            crate::util::contained(|| crate::markdown::commands::heading_anchors(st.doc()))
                .unwrap_or_default()
                .into_iter()
                .map(|h| (h.level, h.title, h.range))
                .collect()
        });
        sidebar.outline.update(&headings);
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
        let sidebar = self.sidebar_visible();
        let tab = self.sidebar_tab().unwrap_or_default().segment();
        // SAFETY: Window restoration always passes a keyed coder.
        unsafe {
            coder.encodeBool_forKey(collapsed, &ns("previewCollapsed"));
            coder.encodeBool_forKey(sidebar, &ns("outlineShown"));
            coder.encodeInteger_forKey(tab, &ns("sidebarTab"));
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
        let tab = SidebarTab::of_segment(unsafe { coder.decodeIntegerForKey(&ns("sidebarTab")) });
        // SAFETY: Window restoration always passes a keyed coder.
        let shown = unsafe { coder.decodeBoolForKey(&ns("outlineShown")) };
        self.put_sidebar(shown, tab);
        (zoom > 0.3).then_some(zoom)
    }
}

/// Each document of a self-test run opens with the sidebar hidden, whatever the steps before left.
#[cfg(feature = "selftest")]
pub(super) fn forget_sidebar() {
    let defaults = NSUserDefaults::standardUserDefaults();
    defaults.removeObjectForKey(&ns(SIDEBAR_SHOWN_KEY));
    defaults.removeObjectForKey(&ns(SIDEBAR_TAB_KEY));
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

    pub(super) fn with_outline(&self, f: impl FnOnce(&objc2_app_kit::NSOutlineView)) {
        if let Some(sidebar) = self.state.sidebar.get() {
            f(&sidebar.outline.view);
        }
    }

    pub(super) fn with_files<R: Default>(&self, f: impl FnOnce(&super::sidebar::List) -> R) -> R {
        self.state
            .sidebar
            .get()
            .map(|sidebar| f(&sidebar.files))
            .unwrap_or_default()
    }

    pub(super) fn outline_rows(&self) -> Option<isize> {
        self.state.sidebar.get().map(|s| s.outline.view.numberOfRows())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
