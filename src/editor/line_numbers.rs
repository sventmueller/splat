//! Line numbers beside the source, for the lines in view only. They stand in a column of their own beside the scroll
//! view, as NSScrollView's ruler stops TextKit 2 text from drawing on macOS 27.

use std::cell::{Cell, OnceCell, RefCell};

use objc2::rc::{Retained, Weak};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSColor, NSFont, NSFontWeightRegular, NSLayoutConstraint, NSRectFill,
    NSScrollView, NSView,
};
use objc2_foundation::{NSArray, NSObjectProtocol, NSPoint, NSRect, NSSize, NSUserDefaults};
use splat_editor_state::line_numbers::{LineCounter, line_count};

use super::EditorController;
use super::attributes::{Attribute, Attributes, attributed};
use super::layers::ForLineNumbers;
use super::style::SOURCE_SIZE;
use crate::markdown::text::U16;
use crate::util::{constant, guarded, ns};

/// User defaults key of the switch in the View menu; without an answer the numbers stay hidden.
const SHOWN_KEY: &str = "ShowsLineNumbers";
/// Room before the numbers and after them; beyond, the text keeps its own inset.
const BEFORE: f64 = 10.0;
const AFTER: f64 = 2.0;
/// The numbers are this much smaller than the text.
const SCALE: f64 = 0.85;
/// A short text keeps its width when it reaches its tenth line.
const FEWEST_DIGITS: usize = 2;
/// Counting the lines takes milliseconds in a large text, so the width waits for a pause in typing.
const SETTLE_SECONDS: f64 = 0.5;

pub struct ColumnIvars {
    controller: Weak<EditorController>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDLineNumbers"]
    #[ivars = ColumnIvars]
    pub struct LineNumberColumn;

    unsafe impl NSObjectProtocol for LineNumberColumn {}

    impl LineNumberColumn {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: NSRect) {
            guarded((), || {
                NSColor::textBackgroundColor().setFill();
                NSRectFill(dirty);
                if let Some(controller) = self.ivars().controller.load() {
                    controller.draw_line_numbers(self);
                }
            });
        }
    }
);

#[derive(Default)]
pub(super) struct LineNumberState {
    column: OnceCell<(Retained<LineNumberColumn>, Retained<NSLayoutConstraint>)>,
    counter: RefCell<LineCounter>,
    /// The text version last measured and its number of lines, which the width follows.
    lines: Cell<Option<(u64, usize)>>,
    /// The numbers last drawn and their baselines in the column.
    #[cfg(feature = "selftest")]
    drawn: RefCell<Vec<(usize, f64)>>,
}

pub(super) struct LineNumbers<'a> {
    host: ForLineNumbers<'a>,
    state: &'a LineNumberState,
}

impl<'a> LineNumbers<'a> {
    pub(super) fn new(host: ForLineNumbers<'a>, state: &'a LineNumberState) -> Self {
        LineNumbers { host, state }
    }
}

/// Whether windows show line numbers.
pub(super) fn is_on() -> bool {
    NSUserDefaults::standardUserDefaults().boolForKey(&ns(SHOWN_KEY))
}

pub(super) fn set_on(on: bool) {
    NSUserDefaults::standardUserDefaults().setBool_forKey(on, &ns(SHOWN_KEY));
}

fn number_attributes(text_size: f64) -> Attributes {
    let font = NSFont::monospacedDigitSystemFontOfSize_weight(text_size * SCALE, constant!(NSFontWeightRegular));
    Attributes::new(&[
        Attribute::font(&font),
        Attribute::foreground(&NSColor::tertiaryLabelColor()),
    ])
}

/// The column's width for a text of `lines` lines at `text_size`.
fn width_for(lines: usize, text_size: f64) -> f64 {
    let digits = lines.to_string().len().max(FEWEST_DIGITS);
    let widest = attributed(&"0".repeat(digits), Some(&number_attributes(text_size)));
    (BEFORE + widest.size().width + AFTER).ceil()
}

/// The column for the source, before its scroll view in the pane, which then takes the rest of the pane. Its width is
/// held at 0 until `LineNumbers::fit` says otherwise.
pub(super) fn column_beside(
    mtm: MainThreadMarker,
    controller: &EditorController,
    scroll: &NSScrollView,
) -> (Retained<LineNumberColumn>, Retained<NSLayoutConstraint>) {
    let this = LineNumberColumn::alloc(mtm).set_ivars(ColumnIvars {
        controller: Weak::from(controller),
    });
    // SAFETY: NSView's designated initializer.
    let column: Retained<LineNumberColumn> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
    // Without it a number can spill onto what lies around the column, as a view may draw past its bounds since macOS 14.
    column.setClipsToBounds(true);
    column.setTranslatesAutoresizingMaskIntoConstraints(false);
    let width = column.widthAnchor().constraintEqualToConstant(0.0);
    // SAFETY: The result is retained on return, and a pane's scroll view stays where the window put it.
    if let Some(pane) = unsafe { scroll.superview() } {
        pane.addSubview(&column);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            column.leadingAnchor().constraintEqualToAnchor(&pane.leadingAnchor()),
            column.topAnchor().constraintEqualToAnchor(&pane.topAnchor()),
            column.bottomAnchor().constraintEqualToAnchor(&pane.bottomAnchor()),
            width.clone(),
            scroll.leadingAnchor().constraintEqualToAnchor(&column.trailingAnchor()),
            scroll.trailingAnchor().constraintEqualToAnchor(&pane.trailingAnchor()),
            scroll.topAnchor().constraintEqualToAnchor(&pane.topAnchor()),
            scroll.bottomAnchor().constraintEqualToAnchor(&pane.bottomAnchor()),
        ]));
    }
    (column, width)
}

impl LineNumbers<'_> {
    pub(super) fn install(&self, column: Retained<LineNumberColumn>, width: Retained<NSLayoutConstraint>) {
        let _ = self.state.column.set((column, width));
        self.fit();
    }

    fn text_size(&self) -> f64 {
        SOURCE_SIZE * self.host.model().read(|st| st.zoom())
    }

    /// Makes the column as wide as the highest number needs at the text's size, or hides it.
    pub(super) fn fit(&self) {
        let Some((column, constraint)) = self.state.column.get() else {
            return;
        };
        let width = if is_on() {
            width_for(self.lines(), self.text_size())
        } else {
            0.0
        };
        if (constraint.constant() - width).abs() > 0.5 {
            constraint.setConstant(width);
        }
        column.setNeedsDisplay(true);
    }

    /// How many lines the text has, measured once per version of it.
    fn lines(&self) -> usize {
        let version = self.host.model().text_version();
        match self.state.lines.get() {
            Some((known, lines)) if known == version => lines,
            _ => {
                let lines = self.host.on_source_text(line_count);
                self.state.lines.set(Some((version, lines)));
                lines
            }
        }
    }

    /// The source changed from UTF-16 offset `from` on.
    pub(super) fn edited(&self, from: usize) {
        self.state.counter.borrow_mut().edited(U16::new(from));
        if is_on() {
            self.redraw();
            self.host
                .object()
                .perform_later(sel!(fitLineNumbers), SETTLE_SECONDS, true);
        }
    }

    /// Draws the numbers again, after a scroll or a new layout of the text.
    pub(super) fn redraw(&self) {
        if let Some((column, _)) = self.state.column.get()
            && is_on()
        {
            column.setNeedsDisplay(true);
        }
    }

    pub(super) fn draw(&self, column: &NSView) {
        #[cfg(feature = "selftest")]
        self.state.drawn.borrow_mut().clear();
        if !is_on() || !self.host.panes().is_built() {
            return;
        }
        let source = self.host.panes().source();
        let paragraphs = source.paragraphs_in_view();
        let starts: Vec<U16> = paragraphs.iter().map(|&(start, _)| U16::new(start)).collect();
        let counter = &mut *self.state.counter.borrow_mut();
        let numbers = self
            .host
            .with_text(move |snap, _| counter.lines_at(&snap.text, &starts));
        let size = self.text_size();
        let attributes = number_attributes(size);
        let ascender =
            NSFont::monospacedDigitSystemFontOfSize_weight(size * SCALE, constant!(NSFontWeightRegular)).ascender();
        let right = column.bounds().size.width - AFTER;
        let mut last = None;
        for (&(_, baseline), &number) in paragraphs.iter().zip(&numbers) {
            if last == Some(number) {
                continue;
            }
            last = Some(number);
            let y = source
                .rect_in(NSRect::new(NSPoint::new(0.0, baseline), NSSize::ZERO), column)
                .origin
                .y;
            let label = attributed(&number.to_string(), Some(&attributes));
            label.drawAtPoint(NSPoint::new(right - label.size().width, y - ascender));
            #[cfg(feature = "selftest")]
            self.state.drawn.borrow_mut().push((number, y));
        }
    }
}

#[cfg(feature = "selftest")]
impl LineNumbers<'_> {
    /// The numbers the column showed last and their baselines in it.
    pub(super) fn drawn(&self) -> Vec<(usize, f64)> {
        self.state.drawn.borrow().clone()
    }

    /// The column's width now.
    pub(super) fn width(&self) -> f64 {
        self.state.column.get().map_or(0.0, |(_, width)| width.constant())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_column_widens_with_the_digits_and_the_text_size() {
        let two = width_for(9, SOURCE_SIZE);
        assert_eq!(
            width_for(99, SOURCE_SIZE),
            two,
            "a short text keeps room for two digits"
        );
        assert!(width_for(100, SOURCE_SIZE) > two);
        assert!(width_for(100, 2.0 * SOURCE_SIZE) > width_for(100, SOURCE_SIZE));
        assert!(two > BEFORE + AFTER);
    }
}
