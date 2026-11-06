//! The word count in the corner of the window: of the whole text, and of the selection while there is one.

use std::cell::{Cell, OnceCell, RefCell};

use objc2::rc::Retained;
use objc2::{MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBox, NSBoxType, NSColor, NSFont, NSLayoutConstraint, NSScrollView, NSTextField, NSTitlePosition, NSView,
};
use objc2_foundation::{
    NSArray, NSEdgeInsets, NSNumber, NSNumberFormatter, NSNumberFormatterStyle, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSUserDefaults,
};

use super::layers::ForCounter;
use super::panes::Side;
use crate::i18n::{t, tf};
use crate::markdown::counts::{self, Counts};
use crate::util::{contained, ns, perform_later};

/// User defaults key of the switch in the View menu; without an answer the count stays hidden.
const HIDDEN_KEY: &str = "HidesWordCount";
/// Long enough that typing and dragging a selection do not count for every step.
const SETTLE_SECONDS: f64 = 0.15;
const PILL_HEIGHT: f64 = 22.0;
const PILL_PADDING: f64 = 10.0;
const PILL_MARGIN: f64 = 12.0;

define_class!(
    #[unsafe(super(NSBox))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDWordCountPill"]
    struct Pill;

    unsafe impl NSObjectProtocol for Pill {}

    impl Pill {
        // A click on it means the text below.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

#[derive(Default)]
pub(super) struct CounterState {
    pill: OnceCell<Retained<Pill>>,
    label: OnceCell<Retained<NSTextField>>,
    /// The count of the whole text and the version of the state it belongs to.
    of_text: Cell<Option<(u64, Counts)>>,
    /// Constraints hold the pill in its corner, also when its pane changes its size: these two change with the text and the
    /// scroller.
    width: RefCell<Option<Retained<NSLayoutConstraint>>>,
    margin: RefCell<Option<Retained<NSLayoutConstraint>>>,
}

pub(super) struct WordCounter<'a> {
    host: ForCounter<'a>,
    state: &'a CounterState,
}

impl<'a> WordCounter<'a> {
    pub(super) fn new(host: ForCounter<'a>, state: &'a CounterState) -> Self {
        WordCounter { host, state }
    }
}

/// Whether windows show the count.
pub(super) fn is_on() -> bool {
    let defaults = NSUserDefaults::standardUserDefaults();
    let hides = defaults
        .objectForKey(&ns(HIDDEN_KEY))
        .map(|_| defaults.boolForKey(&ns(HIDDEN_KEY)));
    shows(hides)
}

fn shows(hides: Option<bool>) -> bool {
    hides == Some(false)
}

pub(super) fn set_on(on: bool) {
    NSUserDefaults::standardUserDefaults().setBool_forKey(!on, &ns(HIDDEN_KEY));
}

fn number(n: usize) -> String {
    NSNumberFormatter::localizedStringFromNumber_numberStyle(
        &NSNumber::new_usize(n),
        NSNumberFormatterStyle::DecimalStyle,
    )
    .to_string()
}

/// What the corner says. Labels in front of the figures spare the plural forms of 35 languages.
fn summary(of_text: Counts, of_selection: Option<Counts>, number: impl Fn(usize) -> String) -> String {
    match of_selection {
        Some(selected) => tf(
            "Auswahl – Wörter: {} von {} · Zeichen: {}",
            &[
                &number(selected.words),
                &number(of_text.words),
                &number(selected.characters),
            ],
        ),
        None => tf(
            "Wörter: {} · Zeichen: {} · Lesezeit: {} Min.",
            &[
                &number(of_text.words),
                &number(of_text.characters),
                &number(of_text.reading_minutes()),
            ],
        ),
    }
}

impl WordCounter<'_> {
    pub(super) fn install(&self) {
        let mtm = self.host.mtm();
        // SAFETY: NSBox's initializer, called once on the newly allocated object.
        let pill: Retained<Pill> = unsafe { msg_send![super(Pill::alloc(mtm).set_ivars(())), init] };
        pill.setBoxType(NSBoxType::Custom);
        pill.setTitlePosition(NSTitlePosition::NoTitle);
        pill.setCornerRadius(PILL_HEIGHT / 2.0);
        pill.setBorderWidth(0.5);
        pill.setBorderColor(&NSColor::separatorColor());
        pill.setFillColor(&NSColor::windowBackgroundColor().colorWithAlphaComponent(0.96));
        pill.setContentViewMargins(NSSize::new(0.0, 0.0));
        let label = NSTextField::labelWithString(&ns(""), mtm);
        label.setFont(Some(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())));
        label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        pill.addSubview(&label);
        let _ = self.state.pill.set(pill);
        let _ = self.state.label.set(label);
        self.place();
    }

    pub(super) fn text_changed(&self) {
        if is_on() {
            perform_later(self.host.object(), sel!(countWords), SETTLE_SECONDS, true);
        }
    }

    pub(super) fn selection_changed(&self) {
        self.text_changed();
    }

    /// The setting changed, here or in another window.
    pub(super) fn setting_changed(&self) {
        self.place();
        self.count_now();
    }

    /// Counts what the state holds. Until it has caught up with the text the next update comes back here.
    pub(super) fn count_now(&self) {
        if !is_on() {
            return;
        }
        let selection = self.host.selection().view_selection();
        let known = self.state.of_text.get();
        let found = self.host.with_text(move |snap, st| {
            if !st.is_current(snap) {
                return None;
            }
            let version = st.version();
            let of_text = match known {
                Some((at, counts)) if at == version => counts,
                _ => contained(|| counts::of_doc(st.doc()))?,
            };
            let range = selection.in_source(snap, st);
            let selected = (!range.is_empty())
                .then(|| contained(|| counts::of_range(st.doc(), range)))
                .flatten();
            Some((version, of_text, selected))
        });
        let Some((version, of_text, selected)) = found else {
            return;
        };
        self.state.of_text.set(Some((version, of_text)));
        self.show(&summary(of_text, selected, number));
    }

    fn show(&self, text: &str) {
        let (Some(pill), Some(label)) = (self.state.pill.get(), self.state.label.get()) else {
            return;
        };
        label.setStringValue(&ns(text));
        label.sizeToFit();
        let size = label.frame().size;
        label.setFrameOrigin(NSPoint::new(PILL_PADDING, (PILL_HEIGHT - size.height) / 2.0));
        let width = size.width + 2.0 * PILL_PADDING;
        pill.setFrameSize(NSSize::new(width, PILL_HEIGHT));
        self.place();
        if let Some(constraint) = &*self.state.width.borrow() {
            constraint.setConstant(width);
        }
    }

    /// In the rightmost pane that shows, which is the preview unless it is hidden.
    pub(super) fn place(&self) {
        let Some(pill) = self.state.pill.get() else { return };
        pill.setHidden(!is_on());
        let panes = self.host.panes();
        let side = if panes.preview_visible() {
            Side::Preview
        } else {
            Side::Source
        };
        let below_the_pill = if is_on() { PILL_HEIGHT + PILL_MARGIN } else { 0.0 };
        for (each, holds_it) in [
            (Side::Source, matches!(side, Side::Source)),
            (Side::Preview, matches!(side, Side::Preview)),
        ] {
            if let Some(scroll) = panes.reader(each).enclosing_scroll_view() {
                leave_room_below(&scroll, if holds_it { below_the_pill } else { 0.0 });
            }
        }
        let Some(scroll) = panes.reader(side).enclosing_scroll_view() else {
            return;
        };
        // SAFETY: The result is retained on return, and only the main thread changes this view's place.
        let parent = unsafe { pill.superview() };
        // SAFETY: The result is retained on return, and a pane's scroll view stays where the window put it.
        let Some(pane) = (unsafe { scroll.superview() }) else {
            return;
        };
        let already_there = parent.is_some_and(|parent| std::ptr::eq(&*parent, &*pane));
        if !already_there {
            pane.addSubview(pill);
            pill.setTranslatesAutoresizingMaskIntoConstraints(false);
            let width = pill.widthAnchor().constraintEqualToConstant(pill.frame().size.width);
            let margin = pill
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&scroll.trailingAnchor(), -PILL_MARGIN);
            NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
                width.clone(),
                margin.clone(),
                pill.heightAnchor().constraintEqualToConstant(PILL_HEIGHT),
                pill.bottomAnchor()
                    .constraintEqualToAnchor_constant(&scroll.bottomAnchor(), -PILL_MARGIN),
            ]));
            *self.state.width.borrow_mut() = Some(width);
            *self.state.margin.borrow_mut() = Some(margin);
        }
        if let Some(margin) = &*self.state.margin.borrow() {
            margin.setConstant(-(PILL_MARGIN + scroller_room(&scroll)));
        }
    }
}

/// Lets the end of the text scroll up clear of the pill, while the rest still scrolls beneath it.
fn leave_room_below(scroll: &NSScrollView, room: f64) {
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
}

/// How much of the pane a scroller takes away from the text; an overlay scroller takes nothing.
fn scroller_room(scroll: &NSScrollView) -> f64 {
    let bounds: NSRect = scroll.bounds();
    (bounds.size.width - scroll.documentVisibleRect().size.width).max(0.0)
}

#[cfg(feature = "selftest")]
impl WordCounter<'_> {
    pub(super) fn shown_text(&self) -> Option<String> {
        let pill = self.state.pill.get()?;
        (!pill.isHidden()).then(|| self.state.label.get().map(|l| l.stringValue().to_string()))?
    }

    /// Whether it stands in the lower corner at the end of the reading direction of its pane, however large that is now.
    pub(super) fn stands_in_its_corner(&self) -> bool {
        let Some(pill) = self.state.pill.get() else {
            return false;
        };
        // SAFETY: The result is retained on return, and only the main thread changes this view's place.
        let Some(pane) = (unsafe { pill.superview() }) else {
            return false;
        };
        pane.layoutSubtreeIfNeeded();
        let (frame, room) = (pill.frame(), pane.bounds().size);
        let gap_below = if pane.isFlipped() {
            room.height - frame.origin.y - frame.size.height
        } else {
            frame.origin.y
        };
        let leftward =
            pane.userInterfaceLayoutDirection() == objc2_app_kit::NSUserInterfaceLayoutDirection::RightToLeft;
        let gap_at_the_end = if leftward {
            frame.origin.x
        } else {
            room.width - frame.origin.x - frame.size.width
        };
        (gap_below - PILL_MARGIN).abs() < 1.0 && (PILL_MARGIN - 1.0..=PILL_MARGIN + 20.0).contains(&gap_at_the_end)
    }

    pub(super) fn sits_in_the_rightmost_pane(&self) -> bool {
        // SAFETY: The result is retained on return, and only the main thread changes this view's place.
        let Some(parent) = self.state.pill.get().and_then(|p| unsafe { p.superview() }) else {
            return false;
        };
        let panes = self.host.panes();
        let side = if panes.preview_visible() {
            Side::Preview
        } else {
            Side::Source
        };
        panes
            .reader(side)
            .enclosing_scroll_view()
            // SAFETY: The result is retained on return, and a pane's scroll view stays where the window put it.
            .and_then(|scroll| unsafe { scroll.superview() })
            .is_some_and(|pane| std::ptr::eq(&*pane, &*parent))
    }

    /// Whether the last line of `view` shows, above the pill, as the view is scrolled now.
    pub(super) fn end_stands_clear(&self, view: &objc2_app_kit::NSTextView) -> bool {
        let Some(pill) = self.state.pill.get() else {
            return false;
        };
        // SAFETY: The result is retained on return, and only the main thread changes this view's place.
        let Some(pane) = (unsafe { pill.superview() }) else {
            return false;
        };
        pane.layoutSubtreeIfNeeded();
        let last = crate::util::storage(view).length().saturating_sub(1);
        let line = view.convertRect_toView(super::scroll::char_rect(view, last), Some(&pane));
        let shown = view.convertRect_toView(view.visibleRect(), Some(&pane));
        let pill = pill.frame();
        let (top, bottom) = (line.origin.y, line.origin.y + line.size.height);
        let inside = top >= shown.origin.y - 0.5 && bottom <= shown.origin.y + shown.size.height + 0.5;
        let above = if pane.isFlipped() {
            bottom <= pill.origin.y + 0.5
        } else {
            top >= pill.origin.y + pill.size.height - 0.5
        };
        inside && above
    }
}

/// The menu's title for the switch, and that it speaks of the current state.
pub(super) fn menu_title() -> &'static str {
    if is_on() {
        t("Wortanzahl ausblenden")
    } else {
        t("Wortanzahl einblenden")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(words: usize, characters: usize) -> Counts {
        Counts {
            words,
            characters,
            signs: 0,
        }
    }

    #[test]
    fn the_count_shows_only_once_someone_asked_for_it() {
        assert!(!shows(None));
        assert!(!shows(Some(true)));
        assert!(shows(Some(false)));
    }

    #[test]
    fn the_corner_tells_of_the_selection_as_long_as_there_is_one() {
        let plain = |n: usize| n.to_string();
        let whole = summary(counts(460, 2000), None, plain);
        assert!(
            whole.contains("460") && whole.contains("2000") && whole.contains('2'),
            "{whole}"
        );
        let part = summary(counts(460, 2000), Some(counts(3, 11)), plain);
        assert!(
            part.contains('3') && part.contains("460") && part.contains("11"),
            "{part}"
        );
        assert!(!part.contains("2000"), "{part}");
    }
}
