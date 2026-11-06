//! Where the text of a view lies and how it shows a highlight, in either version of TextKit: the source view uses TextKit
//! 2, the preview TextKit 1. Only here does code ask a view of unknown TextKit for its layout manager, and only once it
//! has none of TextKit 2, since asking a TextKit 2 view for one switches it to TextKit 1 for good.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSLayoutManager, NSTextContainer, NSTextContentManager, NSTextElementProvider, NSTextLayoutManager,
    NSTextLayoutManagerSegmentOptions, NSTextLayoutManagerSegmentType, NSTextLocation, NSTextRange, NSTextView,
};
use objc2_foundation::{NSAttributedStringKey, NSPoint, NSRange, NSRect, NSSize};

use super::attributes::{Attribute, add_attribute, add_temporary_attribute};
use crate::util::storage;

/// The layout of one text view, by the TextKit it uses.
pub(super) enum TextGeometry<'a> {
    /// TextKit 2, as the source view uses it; it lays out only what is in view.
    Kit2 {
        view: &'a NSTextView,
        layout: Retained<NSTextLayoutManager>,
    },
    /// TextKit 1, as the preview and the print view use it.
    Kit1 {
        view: &'a NSTextView,
        layout: Retained<NSLayoutManager>,
        container: Retained<NSTextContainer>,
    },
}

impl<'a> TextGeometry<'a> {
    /// The layout of `view`; `None` for a view without a text system.
    pub(super) fn of(view: &'a NSTextView) -> Option<TextGeometry<'a>> {
        if let Some(layout) = view.textLayoutManager() {
            return Some(TextGeometry::Kit2 { view, layout });
        }
        // SAFETY: A text view keeps its text system alive, those made in `views.rs` through their `owner`; this view has no
        // TextKit 2 layout manager that asking could cost it.
        let layout = unsafe { view.layoutManager() }?;
        // SAFETY: As above.
        let container = unsafe { view.textContainer() }?;
        Some(TextGeometry::Kit1 {
            view,
            layout,
            container,
        })
    }

    fn view(&self) -> &'a NSTextView {
        match self {
            TextGeometry::Kit2 { view, .. } | TextGeometry::Kit1 { view, .. } => view,
        }
    }

    /// Bounds of the character at UTF-16 offset `index`, in view coordinates.
    pub(super) fn char_rect(&self, index: usize) -> NSRect {
        let view = self.view();
        let len = storage(view).length();
        let found = match self {
            TextGeometry::Kit2 { layout, .. } => segment_rect(layout, index.min(len.saturating_sub(1))),
            TextGeometry::Kit1 { .. } if len == 0 => None,
            TextGeometry::Kit1 { layout, container, .. } => {
                let glyph = layout.glyphIndexForCharacterAtIndex(index.min(len - 1));
                Some(layout.boundingRectForGlyphRange_inTextContainer(NSRange::new(glyph, 1), container))
            }
        };
        let Some(mut rect) = found else {
            return NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
        };
        let origin = view.textContainerOrigin();
        rect.origin.x += origin.x;
        rect.origin.y += origin.y;
        rect
    }

    /// Top of the line holding the character at UTF-16 offset `index`, in view coordinates.
    pub(super) fn char_top(&self, index: usize) -> f64 {
        let view = self.view();
        let len = storage(view).length();
        let top = match self {
            TextGeometry::Kit2 { layout, .. } => line_top(layout, index.min(len.saturating_sub(1))).unwrap_or(0.0),
            TextGeometry::Kit1 { .. } if len == 0 => return 0.0,
            TextGeometry::Kit1 { layout, .. } => {
                let glyph = layout.glyphIndexForCharacterAtIndex(index.min(len - 1));
                // SAFETY: A null range pointer is allowed when the effective range is not wanted.
                unsafe { layout.lineFragmentRectForGlyphAtIndex_effectiveRange(glyph, std::ptr::null_mut()) }
                    .origin
                    .y
            }
        };
        top + view.textContainerOrigin().y
    }

    /// UTF-16 offset of the line at height `y` of the view.
    pub(super) fn offset_at(&self, y: f64) -> Option<usize> {
        let top = (y - self.view().textContainerOrigin().y).max(0.0);
        match self {
            TextGeometry::Kit1 { layout, container, .. } => {
                let glyph = layout.glyphIndexForPoint_inTextContainer(NSPoint::new(0.0, top), container);
                Some(layout.characterIndexForGlyphAtIndex(glyph))
            }
            TextGeometry::Kit2 { layout, .. } => {
                layout.textViewportLayoutController().layoutViewport();
                let fragment = layout.textLayoutFragmentForPosition(NSPoint::new(0.0, top))?;
                let content = layout.textContentManager()?;
                let offset = content.offsetFromLocation_toLocation(
                    &content.documentRange().location(),
                    &fragment.rangeInElement().location(),
                );
                Some(crate::util::to_usize(offset.max(0)))
            }
        }
    }

    /// A TextKit 1 view with non-contiguous layout knows its height only as far as it laid out, so a jump below that ends
    /// at the end it estimates: this lays out down to `y`.
    pub(super) fn make_room(&self, y: f64) {
        let TextGeometry::Kit1 {
            view,
            layout,
            container,
        } = self
        else {
            return;
        };
        let visible = view.visibleRect();
        if y + visible.size.height <= view.bounds().size.height {
            return;
        }
        let origin = view.textContainerOrigin();
        let wanted = NSRect::new(NSPoint::new(0.0, y - origin.y), visible.size);
        layout.ensureLayoutForBoundingRect_inTextContainer(wanted, container);
    }

    /// Scrolls a TextKit 2 view until the character at `index` is laid out and returns its top in view coordinates;
    /// `None` for TextKit 1, which knows every position. TextKit 2 knows positions only around the laid-out part, and
    /// `relocateViewportToTextLocation:` takes up to seconds in a 10 MB document; laying out at an estimated position and
    /// correcting it costs no more than scrolling.
    pub(super) fn reach(&self, index: usize) -> Option<f64> {
        let TextGeometry::Kit2 { view, layout } = self else {
            return None;
        };
        let viewport = layout.textViewportLayoutController();
        let content = layout.textContentManager()?;
        let start = content.documentRange().location();
        let index = index.min(storage(view).length().saturating_sub(1));
        let target = index as f64;
        let mut known: Option<(f64, f64)> = None;
        for _ in 0..8 {
            viewport.layoutViewport();
            let range = viewport.viewportRange()?;
            let from = crate::util::to_usize(content.offsetFromLocation_toLocation(&start, &range.location()).max(0));
            let to = crate::util::to_usize(
                content
                    .offsetFromLocation_toLocation(&start, &range.endLocation())
                    .max(0),
            );
            if (from..=to).contains(&index) {
                return Some(self.char_top(index));
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

    /// Draws `range` with `highlight`, which only shows. TextKit 2 resets rendering attributes whenever it lays a line out
    /// again, so there the highlight goes into the text storage, like the syntax colours: no undo step, and the next
    /// recolouring clears it as well.
    pub(super) fn highlight(&self, highlight: &Attribute, range: NSRange) {
        match self {
            TextGeometry::Kit2 { view, .. } => add_attribute(&storage(view), highlight, range),
            TextGeometry::Kit1 { layout, .. } => add_temporary_attribute(layout, highlight, range),
        }
    }

    /// Removes what `highlight` drew of `key` in `range`.
    pub(super) fn remove_highlight(&self, key: &NSAttributedStringKey, range: NSRange) {
        match self {
            TextGeometry::Kit2 { view, .. } => storage(view).removeAttribute_range(key, range),
            TextGeometry::Kit1 { layout, .. } => layout.removeTemporaryAttribute_forCharacterRange(key, range),
        }
    }
}

/// Bounds of the character at UTF-16 offset `index` of `view`; the visible part for a view without a text system.
pub(super) fn char_rect(view: &NSTextView, index: usize) -> NSRect {
    TextGeometry::of(view).map_or_else(|| view.visibleRect(), |geometry| geometry.char_rect(index))
}

/// UTF-16 offset of the line at height `y` of `view`.
pub(super) fn offset_at(view: &NSTextView, y: f64) -> Option<usize> {
    TextGeometry::of(view)?.offset_at(y)
}

/// UTF-16 offset of the line at the top of the visible part of `view`.
pub(super) fn top_offset(view: &NSTextView) -> Option<usize> {
    offset_at(view, view.visibleRect().origin.y)
}

type TextLocation = (
    Retained<NSTextContentManager>,
    Retained<ProtocolObject<dyn NSTextLocation>>,
);

fn text_location(tlm: &NSTextLayoutManager, index: usize) -> Option<TextLocation> {
    let content = tlm.textContentManager()?;
    let location =
        content.locationFromLocation_withOffset(&content.documentRange().location(), crate::util::to_isize(index))?;
    Some((content, location))
}

/// Top of the line holding the character at UTF-16 offset `index`, in text container coordinates.
fn line_top(tlm: &NSTextLayoutManager, index: usize) -> Option<f64> {
    let (content, location) = text_location(tlm, index)?;
    let fragment = tlm.textLayoutFragmentForLocation(&location)?;
    let range = fragment.rangeInElement();
    tlm.ensureLayoutForRange(&range);
    let top = fragment.layoutFragmentFrame().origin.y;
    let offset = crate::util::to_usize(
        content
            .offsetFromLocation_toLocation(&range.location(), &location)
            .max(0),
    );
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
            crate::util::guarded(objc2::runtime::Bool::NO, || {
                found.set(Some(frame));
                objc2::runtime::Bool::NO
            })
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
