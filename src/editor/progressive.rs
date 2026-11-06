//! Large previews show changed blocks as plain text first; this replaces them with their rendering around the
//! visible part, while the app is idle and right away when they scroll into view.

use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use objc2::sel;
use objc2_foundation::{NSMutableAttributedString, NSRange};

use super::State;
use super::layers::ForProgressive;
use super::panes::Side;
use super::render::{Live, RenderCtx};
use crate::preview::layout::TopBlock;
use crate::preview::map::Shown;
use crate::util::storage;
use splat_editor_state::progress::{idle_window, window_around};

pub(super) struct Progressive<'a> {
    host: ForProgressive<'a>,
}

impl<'a> Progressive<'a> {
    pub(super) fn new(host: ForProgressive<'a>) -> Self {
        Progressive { host }
    }
}

impl Progressive<'_> {
    pub(super) fn schedule_pending_blocks(&self) {
        // Common modes, so rendering goes on while the scroller is dragged.
        self.host.object().perform_later(sel!(renderPendingBlocks), 0.0, true);
    }

    /// Renders the placeholder blocks in view right away, so scrolling never shows them unformatted.
    pub(super) fn render_visible_blocks(&self) {
        // Rendered blocks change height and can pull further placeholders into view.
        for _ in 0..3 {
            if self.host.under_way().defers_background_results() {
                return;
            }
            let view = self.host.panes().preview();
            let visible = view.visible_rect();
            let top = view.offset_at(visible.origin.y).unwrap_or(0);
            let bottom = view.offset_at(visible.origin.y + visible.size.height).unwrap_or(0);
            let window = self.host.model().read(move |st| {
                let placeholders = &st.blocks().placeholders;
                if placeholders.len() != st.layout().blocks.len() {
                    return None;
                }
                let block_at = |offset: usize| {
                    st.index()
                        .block_starts
                        .partition_point(|&s| s.get() <= offset)
                        .saturating_sub(1)
                };
                let window = window_around(block_at(top), block_at(bottom), placeholders.len());
                placeholders[window.clone()].iter().any(|p| *p).then_some(window)
            });
            let Some(window) = window else {
                return;
            };
            self.render_blocks(window.clone(), window.start, None);
        }
    }

    /// Replaces placeholder blocks around the visible part with their real rendering, a few milliseconds at a time.
    /// Blocks further away wait until scrolled near: in a large document, TextKit takes longer for every edit the
    /// further the rendered part reaches beyond the visible one.
    pub(super) fn render_pending_blocks(&self) {
        if self.host.under_way().defers_background_results() {
            self.schedule_pending_blocks();
            return;
        }
        let near = self.visible_preview_block();
        let window = self.host.model().read(|st| {
            let blocks = &st.layout().blocks;
            idle_window(near, st.blocks().placeholders.len(), |block| {
                blocks.get(block).map_or(0, |b| b.paras.len())
            })
        });
        if self.render_blocks(window, near, Some(Duration::from_millis(16))) {
            self.schedule_pending_blocks();
        }
    }

    /// Renders the placeholder blocks in `window`, from `from` on and then those before, until `budget` is used up;
    /// returns whether placeholders are left in `window`.
    pub(super) fn render_blocks(&self, window: Range<usize>, from: usize, budget: Option<Duration>) -> bool {
        let started = Instant::now();
        let in_time = || budget.is_none_or(|b| started.elapsed() < b);
        let dark = self.host.is_dark();
        let doc_dir = self.host.doc_dir();
        let max_width = self.host.panes().preview_image_width();
        let shown_len = self.host.panes().preview().length();
        let wanted = window.clone();
        let Some((zoom, waiting)) = self.host.model().read(move |st| waiting_blocks(st, wanted, from)) else {
            return false;
        };
        let live = Live {
            requester: Some(self.host.object()),
        };
        let ctx = RenderCtx {
            mtm: self.host.mtm(),
            zoom,
            doc_dir: doc_dir.as_deref(),
            images: self.host.pictures().store(),
            max_image_width: max_width,
            dark,
            controller: Some(self.host.object()),
            remote_images: self.host.pictures().remote_allowed(),
            leaves_the_mac: false,
            macros: self.host.model().read(|st| st.macros()),
            drawn: &live,
        };
        // Consecutive blocks go in as one replacement: every single edit of a large text storage has a fixed cost.
        let mut chunks = Vec::new();
        let mut done = Vec::new();
        let mut waiting = waiting.iter().peekable();
        while in_time() {
            let Some(&Waiting { at: first, start, .. }) = waiting.peek().copied() else {
                break;
            };
            let chunk = NSMutableAttributedString::new();
            let (mut next, mut len) = (first, 0);
            while let Some(found) =
                waiting.next_if(|w| w.at == next && in_time() && start.get() + len + w.block.len_u16 <= shown_len)
            {
                chunk.appendAttributedString(&ctx.render_block(&found.block));
                done.push(found.at);
                len += found.block.len_u16;
                next += 1;
            }
            if next == first {
                break;
            }
            chunks.push((NSRange::new(start.get(), len), chunk));
        }
        let remaining = self.host.model().change(move |st| st.rendered(&done, window));
        if !chunks.is_empty() {
            self.host.panes().silently(|views| {
                let storage = storage(views.preview);
                crate::util::editing(&storage, || {
                    for (range, chunk) in &chunks {
                        storage.replaceCharactersInRange_withAttributedString(*range, chunk);
                    }
                });
            });
        }
        // Replacing a block drops the highlight on it.
        let selection = self.host.selection();
        selection.mark(Side::Preview, selection.in_preview());
        remaining
    }

    /// Index of the first preview block in view.
    pub(super) fn visible_preview_block(&self) -> usize {
        let top = self.host.panes().preview().top_offset().unwrap_or(0);
        self.host.model().read(move |st| {
            st.index()
                .block_starts
                .partition_point(|&s| s.get() <= top)
                .saturating_sub(1)
        })
    }
}

/// A placeholder block, its number and where it starts in the preview text.
struct Waiting {
    at: usize,
    block: Arc<TopBlock>,
    start: Shown,
}

/// The zoom and the placeholder blocks of `window`, from `from` on and then those before; `None` when the layout is not
/// the one the preview view shows.
fn waiting_blocks(st: &State, window: Range<usize>, from: usize) -> Option<(f64, Vec<Waiting>)> {
    let placeholders = &st.blocks().placeholders;
    let count = placeholders.len();
    if count != st.layout().blocks.len() || window.end > count || window.start > window.end {
        return None;
    }
    let from = from.clamp(window.start, window.end);
    let waiting = (from..window.end)
        .chain(window.start..from)
        .filter(|&at| placeholders[at])
        .map(|at| Waiting {
            at,
            block: st.layout().blocks[at].clone(),
            start: st.index().block_starts[at],
        })
        .collect();
    Some((st.zoom(), waiting))
}
