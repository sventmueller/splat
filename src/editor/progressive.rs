//! Large previews show changed blocks as plain text first; this replaces them with their rendering around the
//! visible part, while the app is idle and right away when they scroll into view.

use std::ops::Range;
use std::time::{Duration, Instant};

use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadOnly, sel};
use objc2_foundation::{NSMutableAttributedString, NSNotification, NSRange};

use super::EditorController;
use super::render::RenderCtx;
use super::scroll::offset_at;
use crate::util::{perform_later, storage};

/// Placeholders this many blocks before and after the visible one get rendered while idle.
const RENDER_BEHIND: usize = 150;
const RENDER_AHEAD: usize = 600;

/// The blocks around those from `top` to `bottom`, of `count` in all. While TextKit lays out a large text, it can name a
/// line for the bottom of the view that stands above the one it names for the top.
fn window_around(top: usize, bottom: usize, count: usize) -> Range<usize> {
    let end = (top.max(bottom) + 3).min(count);
    top.min(bottom).saturating_sub(2).min(end)..end
}

impl EditorController {
    /// One of the views scrolled.
    pub(super) fn view_scrolled(&self, notification: &NSNotification) {
        let preview_clip = self.preview().enclosingScrollView().map(|s| s.contentView());
        let from_preview = preview_clip
            .zip(notification.object())
            .is_some_and(|(c, o)| std::ptr::eq(&*c as &AnyObject, &*o));
        // Rendered blocks have other heights than their placeholders, so the sync measures after rendering.
        if from_preview {
            self.render_visible_blocks();
        }
        // Not from here: the scroll may be AppKit's own, from a block it runs when layout is done, and laying out the view
        // again inside it frees that block while it runs.
        if !self.ivars().scrolling.get() {
            self.ivars().scrolled_preview.set(from_preview);
            perform_later(self, sel!(syncScroll), 0.0, true);
        }
        let pending = self.ivars().state.try_borrow().is_ok_and(|st| st.blocks().pending());
        if pending {
            self.schedule_pending_blocks();
        }
    }

    pub(super) fn schedule_pending_blocks(&self) {
        // Common modes, so rendering goes on while the scroller is dragged.
        perform_later(self, sel!(renderPendingBlocks), 0.0, true);
    }

    /// Renders the placeholder blocks in view right away, so scrolling never shows them unformatted.
    pub(super) fn render_visible_blocks(&self) {
        // Rendered blocks change height and can pull further placeholders into view.
        for _ in 0..3 {
            if self.ivars().busy.get() || self.preview_editing() {
                return;
            }
            let view = self.preview();
            let visible = view.visibleRect();
            let top = offset_at(view, visible.origin.y).unwrap_or(0);
            let bottom = offset_at(view, visible.origin.y + visible.size.height).unwrap_or(0);
            let window = {
                let Ok(st) = self.ivars().state.try_borrow() else {
                    return;
                };
                let placeholders = &st.blocks().placeholders;
                if placeholders.len() != st.layout().blocks.len() {
                    return;
                }
                let block_at = |offset: usize| {
                    st.index()
                        .block_starts
                        .partition_point(|&s| s.0 <= offset)
                        .saturating_sub(1)
                };
                let window = window_around(block_at(top), block_at(bottom), placeholders.len());
                if !placeholders[window.clone()].iter().any(|p| *p) {
                    return;
                }
                window
            };
            self.render_blocks(window.clone(), window.start, None);
        }
    }

    /// Replaces placeholder blocks around the visible part with their real rendering, a few milliseconds at a time.
    /// Blocks further away wait until scrolled near: in a large document, TextKit takes longer for every edit the
    /// further the rendered part reaches beyond the visible one.
    pub(super) fn render_pending_blocks(&self) {
        if self.ivars().busy.get() || self.preview_editing() || self.ivars().state.try_borrow().is_err() {
            self.schedule_pending_blocks();
            return;
        }
        let near = self.visible_preview_block();
        let count = self.ivars().state.borrow().blocks().placeholders.len();
        let window = near.saturating_sub(RENDER_BEHIND)..(near + RENDER_AHEAD).min(count);
        if self.render_blocks(window, near, Some(Duration::from_millis(16))) {
            self.schedule_pending_blocks();
        }
    }

    /// Renders the placeholder blocks in `window`, from `from` on and then those before, until `budget` is used up;
    /// returns whether placeholders are left in `window`.
    pub(super) fn render_blocks(&self, window: Range<usize>, from: usize, budget: Option<Duration>) -> bool {
        let started = Instant::now();
        let in_time = || budget.is_none_or(|b| started.elapsed() < b);
        let dark = self.is_dark();
        let doc_dir = self.doc_dir();
        let max_width = (self.preview().bounds().size.width - 60.0).max(200.0);
        let storage = storage(self.preview());
        let shown_len = storage.length();
        let (chunks, remaining) = {
            let Ok(mut guard) = self.ivars().state.try_borrow_mut() else {
                return true;
            };
            let zoom = guard.zoom();
            let (layout, index, placeholders) = guard.placeholders();
            let count = placeholders.len();
            if count != layout.blocks.len() || window.end > count || window.start > window.end {
                return false;
            }
            let ctx = RenderCtx {
                mtm: self.mtm(),
                zoom,
                doc_dir: doc_dir.as_deref(),
                images: &self.ivars().images,
                max_image_width: max_width,
                dark,
                controller: Some(self),
                remote_images: self.ivars().remote_allowed.get(),
            };
            let from = from.clamp(window.start, window.end);
            let mut order = (from..window.end).chain(window.start..from);
            // Consecutive blocks go in as one replacement: every single edit of a large text storage has a fixed cost.
            let mut chunks = Vec::new();
            while in_time() {
                let Some(first) = order.by_ref().find(|&i| placeholders[i]) else {
                    break;
                };
                let start = index.block_starts[first];
                let chunk = NSMutableAttributedString::new();
                let (mut last, mut len) = (first, 0);
                while last < window.end && placeholders[last] && in_time() {
                    let block = &layout.blocks[last];
                    if start.0 + len + block.len_u16 > shown_len {
                        break;
                    }
                    chunk.appendAttributedString(&ctx.render_block(block));
                    placeholders[last] = false;
                    len += block.len_u16;
                    last += 1;
                }
                if last == first {
                    break;
                }
                chunks.push((NSRange::new(start.0, len), chunk));
            }
            (chunks, placeholders[window].iter().any(|p| *p))
        };
        if !chunks.is_empty() {
            let _silent = self.silently();
            storage.beginEditing();
            for (range, chunk) in &chunks {
                storage.replaceCharactersInRange_withAttributedString(*range, chunk);
            }
            storage.endEditing();
        }
        // Replacing a block drops the highlight on it.
        self.mark(self.preview(), self.ivars().marks.in_preview());
        remaining
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_stays_inside_the_blocks_whatever_textkit_answers() {
        assert_eq!(window_around(10, 20, 100), 8..23);
        assert_eq!(window_around(0, 0, 100), 0..3);
        assert_eq!(window_around(98, 99, 100), 96..100);
        assert_eq!(window_around(35_933, 35_926, 40_000), 35_924..35_936);
        assert_eq!(window_around(500, 600, 100), 100..100);
        assert_eq!(window_around(0, 0, 0), 0..0);
    }
}
