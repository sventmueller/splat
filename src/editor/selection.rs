//! The selection lives in the focused view; the other view shows it as a highlight in the selection colour.

use std::cell::Cell;
use std::ops::Range;

use objc2::DefinedClass;
use objc2_foundation::NSRange;

use super::attributes::{Attribute, add_attribute, add_temporary_attribute};
use super::editing::bridge;
use super::panes::Side;
use super::sync::Snapshot;
use super::{EditorController, State};
use crate::markdown::text::{Byte, U16};
use crate::preview::map::{Shown, preview_offset};
use crate::util::constant;
use crate::util::{contained, from_nsrange, layout_manager, shown_from_ns, shown_to_ns, storage};

/// The selection of the focused view, shown as highlight in the other one.
#[derive(Default)]
pub(super) struct Marks {
    /// The source selection while the preview has the focus. The source then shows it as a highlight: a text view
    /// without focus draws its own selection grey, whatever colour is set for it.
    source_mirror: Cell<Option<NSRange>>,
    /// Ranges currently highlighted as the other view's selection.
    source: Cell<Option<NSRange>>,
    preview: Cell<Option<NSRange>>,
}

impl Marks {
    fn slot(&self, side: Side) -> &Cell<Option<NSRange>> {
        match side {
            Side::Source => &self.source,
            Side::Preview => &self.preview,
        }
    }

    /// What a source selection of `range` comes to: the selection of the view itself and a highlight, if it needs one.
    fn source_selection(&self, range: NSRange) -> (NSRange, Option<NSRange>) {
        if self.source_mirror.get().is_some() {
            self.source_mirror.set(Some(range));
            (NSRange::new(range.location, 0), Some(range))
        } else {
            (range, None)
        }
    }
}

/// `range` as far as a text of `len` units reaches.
fn clamped(range: NSRange, len: usize) -> NSRange {
    let start = range.location.min(len);
    NSRange::new(start, range.length.min(len - start))
}

/// Where the selection stands in the views, read before the state is lent out.
pub(super) struct ViewSelection {
    /// The preview's selection while that has the focus.
    pub preview: Option<Range<Shown>>,
    pub source: Range<U16>,
}

impl ViewSelection {
    /// The selection as a range of the source text, also while the preview has the focus.
    pub(super) fn in_source(&self, snap: &Snapshot, st: &State) -> Range<Byte> {
        self.preview
            .clone()
            .and_then(|shown| contained(|| bridge(st).map(|b| b.source_selection(shown))).flatten())
            .unwrap_or_else(|| snap.byte_range(&self.source))
    }
}

pub(super) struct Selection<'a> {
    host: &'a EditorController,
    state: &'a Marks,
}

impl EditorController {
    pub(super) fn selection(&self) -> Selection<'_> {
        Selection {
            host: self,
            state: &self.ivars().marks,
        }
    }
}

impl Selection<'_> {
    /// The preview has the focus, and the source shows its selection as a highlight.
    pub(super) fn mirrors_source(&self) -> bool {
        self.state.source_mirror.get().is_some()
    }

    pub(super) fn in_preview(&self) -> Option<NSRange> {
        self.state.preview.get()
    }

    /// The source selection: the real one, or its highlight while the preview has the focus.
    pub(super) fn source_range(&self) -> Range<U16> {
        let r = self
            .state
            .source_mirror
            .get()
            .unwrap_or_else(|| self.host.panes().source().selected_range());
        from_nsrange(r)
    }

    pub(super) fn view_selection(&self) -> ViewSelection {
        ViewSelection {
            preview: self
                .host
                .panes()
                .preview_focused()
                .then(|| shown_from_ns(self.host.panes().preview().selected_range())),
            source: self.source_range(),
        }
    }

    /// Sets the logical source selection: the real one while the source has the focus, otherwise a highlight.
    pub(super) fn set_source_selection(&self, range: NSRange) {
        self.host.panes().silently(|views| {
            let (selected, highlight) = self.state.source_selection(range);
            views.source.setSelectedRange(selected);
            if highlight.is_some() {
                self.mark(Side::Source, highlight);
            }
        });
    }

    /// The preview takes the focus: the source selection turns into a highlight.
    pub(super) fn preview_became_focused(&self) {
        self.mark(Side::Preview, None);
        if self.state.source_mirror.get().is_none() {
            let range = self.host.panes().source().selected_range();
            self.state.source_mirror.set(Some(range));
            self.set_source_selection(range);
        }
    }

    /// The source takes the focus: its highlight becomes the real selection again.
    pub(super) fn source_became_focused(&self) {
        if let Some(range) = self.state.source_mirror.take() {
            self.mark(Side::Source, None);
            self.host.panes().silently(|views| views.source.setSelectedRange(range));
        }
        self.mark_source_selection_in_preview();
    }

    /// Highlights the source selection in the preview, which has no selection of its own meanwhile.
    pub(super) fn mark_source_selection_in_preview(&self) {
        if !self.host.panes().preview_visible() {
            self.mark(Side::Preview, None);
            return;
        }
        let selection = self.source_range();
        let range = self.host.with_text(move |snap, st| {
            if !st.is_current(snap) {
                return None;
            }
            let sel = snap.byte_range(&selection);
            let start = preview_offset(st.layout(), st.index(), sel.start);
            let end = preview_offset(st.layout(), st.index(), sel.end).max(start);
            Some(shown_to_ns(start..end))
        });
        let Some(range) = range else {
            return;
        };
        self.host
            .panes()
            .silently(|views| views.preview.setSelectedRange(NSRange::new(range.location, 0)));
        self.mark(Side::Preview, Some(range));
    }

    pub(super) fn show_marks(&self) {
        if let Some(range) = self.state.source_mirror.get() {
            self.mark(Side::Source, Some(range));
        } else if !self.host.panes().preview_focused() {
            self.mark_source_selection_in_preview();
        }
    }

    pub(super) fn recolor_marks(&self) {
        self.mark(Side::Source, self.state.source.get());
        self.mark(Side::Preview, self.state.preview.get());
    }

    /// Draws `range` of a view like a selection; `None` removes the highlight.
    pub(super) fn mark(&self, side: Side, range: Option<NSRange>) {
        let panes = self.host.panes();
        let slot = self.state.slot(side);
        let view = panes.reader(side);
        let len = view.length();
        let clamp = |r: NSRange| clamped(r, len);
        let key = constant!(objc2_app_kit::NSBackgroundColorAttributeName);
        // TextKit 2 resets rendering attributes whenever it lays a line out again, so the source gets the colour in
        // its text storage, like the syntax colours: no undo step, and the next recolouring clears it as well.
        let in_storage = view.uses_text_kit_2();
        if let Some(old) = slot.take() {
            let old = clamp(old);
            panes.silently(|views| {
                let view = views.of(side);
                if in_storage {
                    storage(view).removeAttribute_range(key, old);
                } else if let Some(lm) = layout_manager(view) {
                    lm.removeTemporaryAttribute_forCharacterRange(key, old);
                }
            });
        }
        let Some(range) = range.map(clamp).filter(|r| r.length > 0) else {
            return;
        };
        let key_window = view.window().is_some_and(|w| w.isKeyWindow());
        let color = if key_window {
            objc2_app_kit::NSColor::selectedTextBackgroundColor()
        } else {
            objc2_app_kit::NSColor::unemphasizedSelectedTextBackgroundColor()
        };
        let highlight = Attribute::background(&color);
        panes.silently(|views| {
            let view = views.of(side);
            if in_storage {
                add_attribute(&storage(view), &highlight, range);
            } else if let Some(lm) = layout_manager(view) {
                add_temporary_attribute(&lm, &highlight, range);
            }
        });
        slot.set(Some(range));
    }

    pub(super) fn place_preview_selection(&self) {
        let selection = self.source_range();
        let (start, end) = self.host.with_text(move |snap, st| {
            let sel = snap.byte_range(&selection);
            let start = preview_offset(st.layout(), st.index(), sel.start);
            let end = if sel.is_empty() {
                start
            } else {
                preview_offset(st.layout(), st.index(), sel.end).max(start)
            };
            (start, end)
        });
        let len = self.host.panes().preview().length();
        let range = shown_to_ns(start.min(Shown::new(len))..end.min(Shown::new(len)));
        self.host.panes().silently(|views| {
            views.preview.setSelectedRange(range);
            views.preview.scrollRangeToVisible(range);
        });
    }

    pub(super) fn reveal_selection_in_preview(&self) {
        if !self.host.panes().preview_visible() {
            return;
        }
        let selection = self.source_range();
        let p = self
            .host
            .with_text(move |snap, st| preview_offset(st.layout(), st.index(), snap.byte_range(&selection).start));
        self.host
            .panes()
            .for_appkit()
            .preview
            .scrollRangeToVisible(NSRange::new(p.get(), 0));
    }
}

#[cfg(feature = "selftest")]
impl Selection<'_> {
    pub(super) fn in_source(&self) -> Option<NSRange> {
        self.state.source.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_highlight_never_reaches_beyond_the_text() {
        assert_eq!(clamped(NSRange::new(2, 3), 10), NSRange::new(2, 3));
        assert_eq!(clamped(NSRange::new(8, 5), 10), NSRange::new(8, 2));
        assert_eq!(clamped(NSRange::new(12, 5), 10), NSRange::new(10, 0));
    }

    #[test]
    fn the_source_selection_is_the_views_own_unless_the_preview_has_the_focus() {
        let marks = Marks::default();
        let range = NSRange::new(4, 3);
        assert_eq!(marks.source_selection(range), (range, None));

        marks.source_mirror.set(Some(NSRange::new(0, 0)));
        assert_eq!(marks.source_selection(range), (NSRange::new(4, 0), Some(range)));
        assert_eq!(
            marks.source_mirror.get(),
            Some(range),
            "the highlight stands for the selection from now on"
        );
    }
}
