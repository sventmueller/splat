//! The selection lives in the focused view; the other view shows it as a highlight in the selection colour.

use std::ops::Range;

use objc2::DefinedClass;
use objc2_app_kit::NSTextView;
use objc2_foundation::NSRange;

use super::EditorController;
use super::sync::Snapshot;
use crate::preview::map::preview_offset;
use crate::util::constant;
use crate::util::{from_nsrange, layout_manager, storage, to_nsrange};

impl EditorController {
    /// The source selection as byte range of `snap`: the real selection, or its highlight while the preview has the
    /// focus.
    pub(super) fn source_selection(&self, snap: &Snapshot) -> Range<usize> {
        let r = self
            .ivars()
            .marks
            .source_mirror
            .get()
            .unwrap_or_else(|| self.source().selectedRange());
        snap.byte_range(&from_nsrange(r))
    }

    /// Sets the logical source selection: the real one while the source has the focus, otherwise a highlight.
    pub(super) fn set_source_selection(&self, range: NSRange) {
        let _silent = self.silently();
        if self.ivars().marks.source_mirror.get().is_some() {
            self.ivars().marks.source_mirror.set(Some(range));
            self.source().setSelectedRange(NSRange::new(range.location, 0));
            self.mark(self.source(), Some(range));
        } else {
            self.source().setSelectedRange(range);
        }
    }

    /// The preview takes the focus: the source selection turns into a highlight.
    pub fn preview_became_focused(&self) {
        self.mark(self.preview(), None);
        if self.ivars().marks.source_mirror.get().is_none() {
            let range = self.source().selectedRange();
            self.ivars().marks.source_mirror.set(Some(range));
            self.set_source_selection(range);
        }
    }

    /// The source takes the focus: its highlight becomes the real selection again.
    pub fn source_became_focused(&self) {
        if let Some(range) = self.ivars().marks.source_mirror.take() {
            self.mark(self.source(), None);
            let _silent = self.silently();
            self.source().setSelectedRange(range);
        }
        self.mark_source_selection_in_preview();
    }

    /// Highlights the source selection in the preview, which has no selection of its own meanwhile.
    pub(super) fn mark_source_selection_in_preview(&self) {
        if !self.preview_visible() {
            self.mark(self.preview(), None);
            return;
        }
        let snap = self.snapshot();
        let sel = self.source_selection(&snap);
        let range = {
            let Ok(st) = self.ivars().state.try_borrow() else {
                return;
            };
            if !st.is_current(&snap) {
                return;
            }
            let start = preview_offset(&st.layout, &st.index, sel.start);
            let end = preview_offset(&st.layout, &st.index, sel.end).max(start);
            NSRange::new(start, end - start)
        };
        {
            let _silent = self.silently();
            self.preview().setSelectedRange(NSRange::new(range.location, 0));
        }
        self.mark(self.preview(), Some(range));
    }

    pub(super) fn refresh_marks(&self) {
        if let Some(range) = self.ivars().marks.source_mirror.get() {
            self.mark(self.source(), Some(range));
        } else if !self.preview_focused() {
            self.mark_source_selection_in_preview();
        }
    }

    pub(super) fn recolor_marks(&self) {
        self.mark(self.source(), self.ivars().marks.source.get());
        self.mark(self.preview(), self.ivars().marks.preview.get());
    }

    /// Draws `range` of a view like a selection; `None` removes the highlight.
    pub(super) fn mark(&self, view: &NSTextView, range: Option<NSRange>) {
        let is_source = std::ptr::eq(view, self.source() as &NSTextView);
        let marks = &self.ivars().marks;
        let slot = if is_source { &marks.source } else { &marks.preview };
        let len = storage(view).length();
        let clamp = |r: NSRange| {
            let start = r.location.min(len);
            NSRange::new(start, r.length.min(len - start))
        };
        let key = constant!(objc2_app_kit::NSBackgroundColorAttributeName);
        // TextKit 2 resets rendering attributes whenever it lays a line out again, so the source gets the colour in
        // its text storage, like the syntax colours: no undo step, and the next recolouring clears it as well.
        let in_storage = view.textLayoutManager().is_some();
        if let Some(old) = slot.take() {
            let old = clamp(old);
            if in_storage {
                let _silent = self.silently();
                storage(view).removeAttribute_range(key, old);
            } else if let Some(lm) = layout_manager(view) {
                lm.removeTemporaryAttribute_forCharacterRange(key, old);
            }
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
        if in_storage {
            let _silent = self.silently();
            // SAFETY: A color for the background color key, within the text.
            unsafe { storage(view).addAttribute_value_range(key, &color, range) };
        } else if let Some(lm) = layout_manager(view) {
            // SAFETY: A color for the background color key, within the text.
            unsafe { lm.addTemporaryAttribute_value_forCharacterRange(key, &color, range) };
        }
        slot.set(Some(range));
    }

    pub(super) fn place_preview_selection(&self) {
        let snap = self.snapshot();
        let sel = self.source_selection(&snap);
        let (start, end) = {
            let st = self.ivars().state.borrow();
            let start = preview_offset(&st.layout, &st.index, sel.start);
            let end = if sel.is_empty() {
                start
            } else {
                preview_offset(&st.layout, &st.index, sel.end).max(start)
            };
            (start, end)
        };
        let len = storage(self.preview()).length();
        let range = to_nsrange(start.min(len)..end.min(len));
        let _silent = self.silently();
        self.preview().setSelectedRange(range);
        self.preview().scrollRangeToVisible(range);
    }

    pub(super) fn reveal_selection_in_preview(&self) {
        if !self.preview_visible() {
            return;
        }
        let snap = self.snapshot();
        let sel = self.source_selection(&snap);
        let p = {
            let st = self.ivars().state.borrow();
            preview_offset(&st.layout, &st.index, sel.start)
        };
        self.preview().scrollRangeToVisible(NSRange::new(p, 0));
    }
}
