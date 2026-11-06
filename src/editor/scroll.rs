//! Keeps the two panes scrolled to the same place.

use std::cell::Cell;
use std::rc::Rc;

use objc2::sel;
use objc2_app_kit::NSTextView;
use objc2_foundation::{NSNotification, NSPoint};

use super::geometry::{TextGeometry, top_offset};
#[cfg(feature = "selftest")]
pub(super) use super::geometry::{char_rect, offset_at};
use super::layers::ForScroll;
use super::state::ScrollSpan;

#[derive(Default)]
pub(super) struct ScrollState {
    /// Raised while the sync measures and scrolls; the bounds changes that causes are its own.
    syncing: Cell<bool>,
    /// Which view the last scroll was in, for the other one to follow.
    scrolled_preview: Cell<bool>,
    after_undo: Cell<bool>,
}

impl ScrollState {
    /// A view scrolled: whether the other one has to follow, which the sync's own scrolling never asks for.
    fn view_scrolled(&self, from_preview: bool) -> bool {
        if self.syncing.get() {
            return false;
        }
        self.scrolled_preview.set(from_preview);
        true
    }

    fn take_after_undo(&self) -> bool {
        self.after_undo.replace(false)
    }
}

pub(super) struct ScrollSync<'a> {
    host: ForScroll<'a>,
    state: &'a ScrollState,
}

impl<'a> ScrollSync<'a> {
    pub(super) fn new(host: ForScroll<'a>, state: &'a ScrollState) -> Self {
        ScrollSync { host, state }
    }
}

impl ScrollSync<'_> {
    /// An undo or redo moves the selection, and the next update brings it into view.
    pub(super) fn undo_happened(&self) {
        self.state.after_undo.set(true);
    }

    pub(super) fn take_undo_happened(&self) -> bool {
        self.state.take_after_undo()
    }

    /// One of the views scrolled.
    pub(super) fn view_scrolled(&self, notification: &NSNotification) {
        let preview = self.host.panes().preview();
        let from_preview = notification
            .object()
            .is_some_and(|object| preview.is_clip_view(&object));
        // Rendered blocks have other heights than their placeholders, so the sync measures after rendering.
        if from_preview {
            self.host.progressive().render_visible_blocks();
        }
        // Not from here: the scroll may be AppKit's own, from a block it runs when layout is done, and laying out the view
        // again inside it frees that block while it runs.
        if self.state.view_scrolled(from_preview) {
            self.host.object().perform_later(sel!(syncScroll), 0.0, true);
        }
        if self.host.model().read(|st| st.blocks().pending()) {
            self.host.progressive().schedule_pending_blocks();
        }
    }

    pub(super) fn scroll_spans(&self) -> Rc<Vec<ScrollSpan>> {
        self.host.model().change(|st| {
            if let Some(spans) = st.scroll_spans() {
                return spans;
            }
            let mut cursor = crate::markdown::text::Utf16Cursor::new(st.text());
            // Footnote definitions comrak moved behind the text keep no place in the source order.
            let spans: Vec<ScrollSpan> = st
                .layout()
                .blocks
                .iter()
                .take(st.index().in_order)
                .zip(&st.index().block_starts)
                .map(|(b, &p)| {
                    let s0 = cursor.at(b.src.start);
                    let s1 = cursor.at(b.src.end.max(b.src.start));
                    ScrollSpan {
                        source: (s0.get(), s1.get()),
                        preview: (p.get(), p.get() + b.len_u16),
                    }
                })
                .collect();
            let spans = Rc::new(spans);
            st.remember_scroll_spans(spans.clone());
            spans
        })
    }

    pub(super) fn sync_scroll(&self) {
        let panes = self.host.panes();
        if self.state.syncing.get() || panes.is_silent() || !panes.preview_visible() {
            return;
        }
        let from_source = !self.state.scrolled_preview.get();
        let views = panes.for_appkit();
        let (from, to): (&NSTextView, &NSTextView) = if from_source {
            (views.source, views.preview)
        } else {
            (views.preview, views.source)
        };
        let spans = self.scroll_spans();
        if spans.is_empty() {
            return;
        }
        // Asking either view for positions can lay it out and resize it, which posts another bounds change.
        crate::util::with_raised(&self.state.syncing, || self.follow(from, to, from_source, &spans));
    }

    fn follow(&self, from: &NSTextView, to: &NSTextView, from_source: bool, spans: &[ScrollSpan]) {
        let side = |s: &ScrollSpan, from_side: bool| if from_side == from_source { s.source } else { s.preview };
        let (Some(from_layout), Some(to_layout)) = (TextGeometry::of(from), TextGeometry::of(to)) else {
            return;
        };
        // Only positions near the visible part are cheap: elsewhere both TextKit versions have to lay out first.
        let Some(offset) = top_offset(from) else {
            return;
        };
        // Laying out the visible part can move it in TextKit 2, so it is measured afterwards.
        let visible = from.visibleRect();
        let top = visible.origin.y;
        let i = spans.partition_point(|s| side(s, true).0 <= offset).saturating_sub(1);
        let y0 = from_layout.char_top(side(&spans[i], true).0);
        let y1 = spans.get(i + 1).map_or_else(
            || from_layout.char_top(side(&spans[i], true).1) + 20.0,
            |n| from_layout.char_top(side(n, true).0),
        );
        let frac = if y1 > y0 {
            ((top - y0) / (y1 - y0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if from_source {
            let len = spans.len();
            self.host
                .progressive()
                .render_blocks(i.saturating_sub(2)..(i + 40).min(len), i, None);
        }
        let t0 = to_layout
            .reach(side(&spans[i], false).0)
            .unwrap_or_else(|| to_layout.char_top(side(&spans[i], false).0));
        let t1 = spans.get(i + 1).map_or_else(
            || to_layout.char_top(side(&spans[i], false).1) + 20.0,
            |n| to_layout.char_top(side(n, false).0),
        );
        let mut y = t0 + frac * (t1 - t0);
        let from_doc = from.bounds().size.height - visible.size.height;
        if from_doc > 0.0 && top >= from_doc - 1.0 {
            y = to.bounds().size.height;
        }
        if top <= 0.5 {
            y = 0.0;
        }
        to_layout.make_room(y);
        to.scrollPoint(NSPoint::new(0.0, y.max(0.0)));
    }
}

#[cfg(feature = "selftest")]
impl ScrollSync<'_> {
    /// Keeps the sync still while a measurement scrolls the views itself.
    pub(super) fn hold(&self, held: bool) {
        self.state.syncing.set(held);
    }

    pub(super) fn follow_source_next(&self) {
        self.state.scrolled_preview.set(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_other_view_follows_the_view_that_scrolled_last() {
        let state = ScrollState::default();
        assert!(state.view_scrolled(true));
        assert!(state.scrolled_preview.get());
        assert!(state.view_scrolled(false));
        assert!(!state.scrolled_preview.get());
    }

    #[test]
    fn the_syncs_own_scrolling_neither_asks_for_a_sync_nor_turns_it_around() {
        let state = ScrollState::default();
        assert!(state.view_scrolled(true));
        let asked = crate::util::with_raised(&state.syncing, || state.view_scrolled(false));
        assert!(!asked);
        assert!(state.scrolled_preview.get(), "the preview still leads");
        assert!(
            state.view_scrolled(false),
            "and once the sync is over, a scroll counts again"
        );
    }

    #[test]
    fn an_undo_brings_the_selection_into_view_once() {
        let state = ScrollState::default();
        assert!(!state.take_after_undo());
        state.after_undo.set(true);
        assert!(state.take_after_undo());
        assert!(!state.take_after_undo());
    }
}
