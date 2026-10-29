//! Keeps the two panes scrolled to the same place, and the TextKit 1 and 2 position queries this needs.

use std::cell::Cell;
use std::rc::Rc;

use objc2::DefinedClass;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSTextContentManager, NSTextElementProvider, NSTextLayoutManager, NSTextLayoutManagerSegmentOptions,
    NSTextLayoutManagerSegmentType, NSTextLocation, NSTextRange, NSTextView,
};
use objc2_foundation::{NSNotification, NSPoint, NSRange, NSRect, NSSize};

use super::{EditorController, Raised};
use crate::util::{layout_manager, storage};

#[derive(Clone, Copy)]
pub(super) struct ScrollSpan {
    source: (usize, usize),
    preview: (usize, usize),
}

impl EditorController {
    /// `None` while the state is in use: layout queries made then can resize a view and post a scroll.
    pub(super) fn scroll_spans(&self) -> Option<Rc<Vec<ScrollSpan>>> {
        let mut st = self.ivars().state.try_borrow_mut().ok()?;
        if let Some(spans) = &st.scroll_spans {
            return Some(spans.clone());
        }
        let mut cursor = crate::markdown::text::Utf16Cursor::new(&st.text);
        // Footnote definitions comrak moved behind the text keep no place in the source order.
        let spans: Vec<ScrollSpan> = st
            .layout
            .blocks
            .iter()
            .take(st.index.in_order)
            .zip(&st.index.block_starts)
            .map(|(b, &p)| {
                let s0 = cursor.at(b.src.start);
                let s1 = cursor.at(b.src.end.max(b.src.start));
                ScrollSpan {
                    source: (s0, s1),
                    preview: (p, p + b.len_u16),
                }
            })
            .collect();
        let spans = Rc::new(spans);
        st.scroll_spans = Some(spans.clone());
        Some(spans)
    }

    pub(super) fn sync_scroll(&self, notification: &NSNotification) {
        if self.ivars().scrolling.get() || self.ivars().busy.get() || !self.preview_visible() {
            return;
        }
        let Some(object) = notification.object() else { return };
        let source_clip = self.source().enclosingScrollView().map(|s| s.contentView());
        let from_source = source_clip.is_some_and(|c| std::ptr::eq(&*c as &AnyObject, &*object));
        let (from, to): (&NSTextView, &NSTextView) = if from_source {
            (self.source(), self.preview())
        } else {
            (self.preview(), self.source())
        };
        let Some(spans) = self.scroll_spans().filter(|s| !s.is_empty()) else {
            return;
        };
        // Asking either view for positions can lay it out and resize it, which posts another bounds change.
        let _scrolling = Raised::new(&self.ivars().scrolling);
        let side = |s: &ScrollSpan, from_side: bool| if from_side == from_source { s.source } else { s.preview };
        // Only positions near the visible part are cheap: elsewhere both TextKit versions have to lay out first.
        let Some(offset) = top_offset(from) else {
            return;
        };
        // Laying out the visible part can move it in TextKit 2, so it is measured afterwards.
        let visible = from.visibleRect();
        let top = visible.origin.y;
        let i = spans.partition_point(|s| side(s, true).0 <= offset).saturating_sub(1);
        let y0 = char_top(from, side(&spans[i], true).0);
        let y1 = spans.get(i + 1).map_or_else(
            || char_top(from, side(&spans[i], true).1) + 20.0,
            |n| char_top(from, side(n, true).0),
        );
        let frac = if y1 > y0 {
            ((top - y0) / (y1 - y0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if from_source {
            let len = spans.len();
            self.render_blocks(i.saturating_sub(2)..(i + 40).min(len), i, None);
        }
        let t0 = reach(to, side(&spans[i], false).0).unwrap_or_else(|| char_top(to, side(&spans[i], false).0));
        let t1 = spans.get(i + 1).map_or_else(
            || char_top(to, side(&spans[i], false).1) + 20.0,
            |n| char_top(to, side(n, false).0),
        );
        let mut y = t0 + frac * (t1 - t0);
        let from_doc = from.bounds().size.height - visible.size.height;
        if from_doc > 0.0 && top >= from_doc - 1.0 {
            y = to.bounds().size.height;
        }
        if top <= 0.5 {
            y = 0.0;
        }
        to.scrollPoint(NSPoint::new(0.0, y.max(0.0)));
    }
}

pub(super) fn char_rect(view: &NSTextView, index: usize) -> NSRect {
    if let Some(tlm) = view.textLayoutManager() {
        let len = storage(view).length();
        let Some(mut rect) = segment_rect(&tlm, index.min(len.saturating_sub(1))) else {
            return NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
        };
        let origin = view.textContainerOrigin();
        rect.origin.x += origin.x;
        rect.origin.y += origin.y;
        return rect;
    }
    let Some(lm) = layout_manager(view) else {
        return view.visibleRect();
    };
    let len = storage(view).length();
    if len == 0 {
        return NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
    }
    let glyph = lm.glyphIndexForCharacterAtIndex(index.min(len - 1));
    let Some(tc) = crate::util::text_container(view) else {
        return view.visibleRect();
    };
    let mut rect = lm.boundingRectForGlyphRange_inTextContainer(NSRange::new(glyph, 1), &tc);
    let origin = view.textContainerOrigin();
    rect.origin.x += origin.x;
    rect.origin.y += origin.y;
    rect
}

fn char_top(view: &NSTextView, index: usize) -> f64 {
    if let Some(tlm) = view.textLayoutManager() {
        let len = storage(view).length();
        return line_top(&tlm, index.min(len.saturating_sub(1))).unwrap_or(0.0) + view.textContainerOrigin().y;
    }
    let Some(lm) = layout_manager(view) else { return 0.0 };
    let len = storage(view).length();
    if len == 0 {
        return 0.0;
    }
    let index = index.min(len - 1);
    let glyph = lm.glyphIndexForCharacterAtIndex(index);
    // SAFETY: A null range pointer is allowed when the effective range is not wanted.
    let rect = unsafe { lm.lineFragmentRectForGlyphAtIndex_effectiveRange(glyph, std::ptr::null_mut()) };
    rect.origin.y + view.textContainerOrigin().y
}

type TextLocation = (
    Retained<NSTextContentManager>,
    Retained<ProtocolObject<dyn NSTextLocation>>,
);

fn text_location(tlm: &NSTextLayoutManager, index: usize) -> Option<TextLocation> {
    let content = tlm.textContentManager()?;
    let location = content.locationFromLocation_withOffset(&content.documentRange().location(), index as isize)?;
    Some((content, location))
}

/// UTF-16 offset of the line at the top of the visible part of a TextKit 2 view.
pub(super) fn top_offset(view: &NSTextView) -> Option<usize> {
    offset_at(view, view.visibleRect().origin.y)
}

/// UTF-16 offset of the line at height `y` of a text view.
pub(super) fn offset_at(view: &NSTextView, y: f64) -> Option<usize> {
    let top = (y - view.textContainerOrigin().y).max(0.0);
    let Some(tlm) = view.textLayoutManager() else {
        let (lm, tc) = (layout_manager(view)?, crate::util::text_container(view)?);
        let glyph = lm.glyphIndexForPoint_inTextContainer(NSPoint::new(0.0, top), &tc);
        return Some(lm.characterIndexForGlyphAtIndex(glyph));
    };
    tlm.textViewportLayoutController().layoutViewport();
    let fragment = tlm.textLayoutFragmentForPosition(NSPoint::new(0.0, top))?;
    let content = tlm.textContentManager()?;
    let offset = content.offsetFromLocation_toLocation(
        &content.documentRange().location(),
        &fragment.rangeInElement().location(),
    );
    Some(offset.max(0) as usize)
}

/// Scrolls a TextKit 2 view until the character at `index` is laid out and returns its top in view coordinates.
/// TextKit 2 knows positions only around the laid-out part, and `relocateViewportToTextLocation:` takes up to
/// seconds in a 10 MB document; laying out at an estimated position and correcting it costs no more than scrolling.
fn reach(view: &NSTextView, index: usize) -> Option<f64> {
    let tlm = view.textLayoutManager()?;
    let viewport = tlm.textViewportLayoutController();
    let content = tlm.textContentManager()?;
    let start = content.documentRange().location();
    let index = index.min(storage(view).length().saturating_sub(1));
    let target = index as f64;
    let mut known: Option<(f64, f64)> = None;
    for _ in 0..8 {
        viewport.layoutViewport();
        let range = viewport.viewportRange()?;
        let from = content.offsetFromLocation_toLocation(&start, &range.location()).max(0) as usize;
        let to = content
            .offsetFromLocation_toLocation(&start, &range.endLocation())
            .max(0) as usize;
        if (from..=to).contains(&index) {
            return Some(char_top(view, index));
        }
        let y = view.visibleRect().origin.y;
        let here = (y, from as f64);
        let height = view.bounds().size.height;
        // Secant step between the last two positions; the first guess assumes evenly spread text.
        let next = match known {
            Some((y0, o0)) if (here.1 - o0).abs() > 0.5 => y + (target - here.1) * (y - y0) / (here.1 - o0),
            _ => height * target / storage(view).length().max(1) as f64,
        };
        known = Some(here);
        view.scrollPoint(NSPoint::new(0.0, next.clamp(0.0, height)));
    }
    None
}

/// Top of the line holding the character at UTF-16 offset `index`, in text container coordinates.
fn line_top(tlm: &NSTextLayoutManager, index: usize) -> Option<f64> {
    let (content, location) = text_location(tlm, index)?;
    let fragment = tlm.textLayoutFragmentForLocation(&location)?;
    let range = fragment.rangeInElement();
    tlm.ensureLayoutForRange(&range);
    let top = fragment.layoutFragmentFrame().origin.y;
    let offset = content
        .offsetFromLocation_toLocation(&range.location(), &location)
        .max(0) as usize;
    let lines = fragment.textLineFragments();
    let line = lines
        .iter()
        .find(|l| {
            let r = l.characterRange();
            offset < r.location + r.length
        })
        .or_else(|| lines.lastObject());
    Some(top + line.map_or(0.0, |l| l.typographicBounds().origin.y))
}

/// Bounds of the character at UTF-16 offset `index`, in text container coordinates.
fn segment_rect(tlm: &NSTextLayoutManager, index: usize) -> Option<NSRect> {
    let (content, start) = text_location(tlm, index)?;
    let end = content.locationFromLocation_withOffset(&start, 1);
    let range =
        NSTextRange::initWithLocation_endLocation(<NSTextRange as objc2::AnyThread>::alloc(), &start, end.as_deref())?;
    tlm.ensureLayoutForRange(&range);
    let found = Cell::new(None);
    let block = block2::StackBlock::new(
        |_: *mut NSTextRange, frame: NSRect, _: f64, _: std::ptr::NonNull<objc2_app_kit::NSTextContainer>| {
            found.set(Some(frame));
            objc2::runtime::Bool::NO
        },
    );
    tlm.enumerateTextSegmentsInRange_type_options_usingBlock(
        &range,
        NSTextLayoutManagerSegmentType::Standard,
        NSTextLayoutManagerSegmentOptions::empty(),
        &block,
    );
    found.get()
}
