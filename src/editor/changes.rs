//! The marks beside the text for what differs from the last Git commit, in both panes.

use std::cell::{Cell, OnceCell, RefCell};
use std::path::PathBuf;
use std::sync::Arc;

use objc2::rc::{Retained, Weak};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSBezierPath, NSColor, NSLayoutConstraint, NSScrollView, NSView};
use objc2_foundation::{NSArray, NSObjectProtocol, NSPoint, NSRect, NSSize, NSUserDefaults};
use splat_editor_state::changes::{BaseLook, Mark, in_view, marks, read_base};
use splat_git::Oid;
use splat_git::diff::{Change, ChangeKind, changes};

use super::EditorController;
use super::layers::{ForChanges, Message};
use super::panes::Side;
use crate::util::{contained, guarded, lane_of, ns};

/// User defaults key of the switch in the View menu; without an answer the marks show.
const HIDDEN_KEY: &str = "HidesGitChanges";
/// Long enough that typing does not compare for every key.
const SETTLE_SECONDS: f64 = 0.4;
/// A document beyond this shows no marks; comparing it would take longer than a pause in typing.
const MOST_BYTES: usize = splat_git::MAX_TEXT_BYTES;
const BAR_ROOM: f64 = 9.0;
const BAR_INSET: f64 = 4.0;
const BAR_WIDTH: f64 = 3.0;
const REMOVED_HEIGHT: f64 = 6.0;

pub struct BarIvars {
    controller: Weak<EditorController>,
    side: Side,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDChangeBars"]
    #[ivars = BarIvars]
    pub struct ChangeBars;

    unsafe impl NSObjectProtocol for ChangeBars {}

    impl ChangeBars {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        // A click beside the text means the text.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            guarded((), || {
                if let Some(controller) = self.ivars().controller.load() {
                    controller.draw_change_bars(self.ivars().side, self);
                }
            });
        }
    }
);

impl ChangeBars {
    fn new(mtm: MainThreadMarker, controller: &EditorController, side: Side, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(BarIvars {
            controller: Weak::from(controller),
            side,
        });
        // SAFETY: NSView's designated initializer.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

/// The version of the document in the last commit.
#[derive(Clone)]
struct Base {
    path: PathBuf,
    commit: Oid,
    text: Arc<str>,
}

/// What came back from another thread.
pub(in crate::editor) enum ChangesAnswer {
    Looked(BaseLook),
    Compared {
        generation: u64,
        version: u64,
        changes: Vec<Change>,
    },
}

#[derive(Default)]
pub(super) struct ChangeState {
    bars: OnceCell<(Retained<ChangeBars>, Retained<ChangeBars>)>,
    base: RefCell<Option<Base>>,
    looking: Cell<bool>,
    generation: Cell<u64>,
    /// The changes and the version of the text they belong to.
    found: RefCell<Option<(u64, Arc<Vec<Change>>)>>,
    /// The text version and commit last compared, which need no second look.
    compared: Cell<Option<(u64, Oid)>>,
    source: RefCell<Vec<Mark>>,
    preview: RefCell<Vec<Mark>>,
}

pub(super) struct ChangeMarks<'a> {
    host: ForChanges<'a>,
    state: &'a ChangeState,
}

impl<'a> ChangeMarks<'a> {
    pub(super) fn new(host: ForChanges<'a>, state: &'a ChangeState) -> Self {
        ChangeMarks { host, state }
    }
}

/// Whether windows mark what changed.
pub(super) fn is_on() -> bool {
    !NSUserDefaults::standardUserDefaults().boolForKey(&ns(HIDDEN_KEY))
}

pub(super) fn set_on(on: bool) {
    NSUserDefaults::standardUserDefaults().setBool_forKey(!on, &ns(HIDDEN_KEY));
}

/// A bar's place, from where its text starts and ends in the coordinates of the bars.
fn bar_rect(kind: ChangeKind, x: f64, top: f64, bottom: f64) -> NSRect {
    let height = if kind == ChangeKind::Removed {
        REMOVED_HEIGHT
    } else {
        (bottom - top).max(2.0)
    };
    let top = if kind == ChangeKind::Removed {
        top - REMOVED_HEIGHT / 2.0
    } else {
        top
    };
    NSRect::new(NSPoint::new(x, top), NSSize::new(BAR_WIDTH, height))
}

fn color(kind: ChangeKind) -> Retained<NSColor> {
    match kind {
        ChangeKind::Added => NSColor::systemGreenColor(),
        ChangeKind::Modified => NSColor::systemBlueColor(),
        ChangeKind::Removed => NSColor::systemRedColor(),
    }
}

/// The bars of `side`, beside its scroll view in the pane: a scroll view lays out only what it knows of.
pub(super) fn bars_beside(
    mtm: MainThreadMarker,
    controller: &EditorController,
    side: Side,
    scroll: &NSScrollView,
) -> Retained<ChangeBars> {
    let height = scroll.bounds().size.height;
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(BAR_ROOM, height));
    let bars = ChangeBars::new(mtm, controller, side, frame);
    // SAFETY: The result is retained on return, and a pane's scroll view stays where the window put it.
    if let Some(pane) = unsafe { scroll.superview() } {
        pane.addSubview(&bars);
    }
    bars.setTranslatesAutoresizingMaskIntoConstraints(false);
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        bars.leadingAnchor().constraintEqualToAnchor(&scroll.leadingAnchor()),
        bars.topAnchor().constraintEqualToAnchor(&scroll.topAnchor()),
        bars.bottomAnchor().constraintEqualToAnchor(&scroll.bottomAnchor()),
        bars.widthAnchor().constraintEqualToConstant(BAR_ROOM),
    ]));
    bars
}

impl ChangeMarks<'_> {
    pub(super) fn install(&self, source: Retained<ChangeBars>, preview: Retained<ChangeBars>) {
        let _ = self.state.bars.set((source, preview));
    }

    /// Asks the repository whether HEAD moved, or the file did. One look at a time, as a volume can hang.
    pub(super) fn look(&self) {
        if !is_on() {
            self.forget();
            return;
        }
        if self.state.looking.get() {
            return;
        }
        let Some(file) = self.document_path() else {
            self.forget();
            return;
        };
        let known = self
            .state
            .base
            .borrow()
            .as_ref()
            .filter(|base| base.path == file)
            .map(|base| base.commit);
        self.state.looking.set(true);
        let messenger = self.host.object().messenger();
        let lane = lane_of(&file);
        lane.ask(
            move || read_base(&file, known),
            move |look| {
                messenger.send_soon(Message::Changes(ChangesAnswer::Looked(
                    look.unwrap_or(BaseLook::Nothing),
                )));
            },
        );
    }

    fn document_path(&self) -> Option<PathBuf> {
        let path = self.host.document()?.fileURL()?.path()?.to_string();
        Some(PathBuf::from(path))
    }

    pub(super) fn answer(&self, answer: ChangesAnswer) {
        match answer {
            ChangesAnswer::Looked(look) => self.looked(look),
            ChangesAnswer::Compared {
                generation,
                version,
                changes,
            } => {
                if generation == self.state.generation.get() {
                    *self.state.found.borrow_mut() = Some((version, Arc::new(changes)));
                    self.place_marks();
                }
            }
        }
    }

    /// The look `look` started came back, and the next may start.
    fn looked(&self, look: BaseLook) {
        self.state.looking.set(false);
        match look {
            BaseLook::Unchanged => {}
            BaseLook::Nothing => self.forget(),
            BaseLook::New { path, commit, text } => {
                *self.state.base.borrow_mut() = Some(Base {
                    path,
                    commit,
                    text: text.into(),
                });
                self.compare_now();
            }
        }
    }

    fn forget(&self) {
        let had_any = self.state.base.borrow_mut().take().is_some() | self.state.found.borrow_mut().take().is_some();
        self.state.generation.set(self.state.generation.get() + 1);
        self.state.compared.set(None);
        if had_any {
            self.state.source.borrow_mut().clear();
            self.state.preview.borrow_mut().clear();
            self.redraw();
        }
    }

    /// The text changed, or the preview shows another layout of it.
    pub(super) fn text_changed(&self) {
        if self.state.base.borrow().is_none() {
            return;
        }
        self.place_marks();
        self.host
            .object()
            .perform_later(sel!(compareWithCommit), SETTLE_SECONDS, true);
    }

    /// Compares the text with the commit on another thread, unless it is the text that was last compared.
    pub(super) fn compare_now(&self) {
        let Some(base) = self.state.base.borrow().clone() else {
            return;
        };
        let version = self.host.model().text_version();
        if self.state.compared.get() == Some((version, base.commit)) {
            return;
        }
        self.state.compared.set(Some((version, base.commit)));
        let text = self.host.source_text();
        if text.len() > MOST_BYTES || base.text.len() > MOST_BYTES {
            self.forget();
            return;
        }
        let generation = self.state.generation.get() + 1;
        self.state.generation.set(generation);
        let messenger = self.host.object().messenger();
        crate::util::BACKGROUND.ask(
            move || changes(&base.text, &text),
            move |found| {
                messenger.send_soon(Message::Changes(ChangesAnswer::Compared {
                    generation,
                    version,
                    changes: found.unwrap_or_default(),
                }));
            },
        );
    }

    /// Where the changes stand in the two views. Offsets of another text version would point anywhere, so those marks
    /// stay until the next comparison.
    fn place_marks(&self) {
        let Some((version, found)) = self.state.found.borrow().clone() else {
            return;
        };
        let placed = self.host.with_text(move |snap, st| {
            if snap.version != version || !st.is_current(snap) {
                return None;
            }
            contained(|| marks(&found, &snap.text, st))
        });
        if let Some((source, preview)) = placed {
            *self.state.source.borrow_mut() = source;
            *self.state.preview.borrow_mut() = preview;
            self.redraw();
        }
    }

    /// Redraws the marks, after a scroll or a change of them.
    pub(super) fn redraw(&self) {
        if let Some((source, preview)) = self.state.bars.get() {
            source.setNeedsDisplay(true);
            preview.setNeedsDisplay(true);
        }
    }

    /// Where the marks of what shows go, asking the view only for places in what shows: anywhere else, a text view would
    /// have to lay the text out up to there. Below the end of a short text a view knows no place, and the text ends there.
    fn bars_in_view(&self, side: Side, bars: &NSView) -> Vec<(ChangeKind, NSRect)> {
        let marks = match side {
            Side::Source => self.state.source.borrow(),
            Side::Preview => self.state.preview.borrow(),
        };
        if marks.is_empty() {
            return Vec::new();
        }
        let panes = self.host.panes();
        let view = panes.reader(side);
        let visible = view.visible_rect();
        let top = view.offset_at(visible.origin.y).unwrap_or(0);
        let bottom = view
            .offset_at(visible.origin.y + visible.size.height)
            .unwrap_or_else(|| view.length());
        in_view(&marks, top, bottom)
            .iter()
            .map(|mark| {
                let start = mark.start.clamp(top, bottom);
                let end = mark.end.clamp(start, bottom);
                let first = view.char_rect(start);
                let last = view.char_rect(end.saturating_sub(1).max(start));
                let in_bars = view.rect_in(
                    NSRect::new(
                        first.origin,
                        NSSize::new(first.size.width, last.origin.y + last.size.height - first.origin.y),
                    ),
                    bars,
                );
                let (from, to) = (in_bars.origin.y, in_bars.origin.y + in_bars.size.height);
                (mark.kind, bar_rect(mark.kind, BAR_INSET, from.min(to), from.max(to)))
            })
            .collect()
    }

    pub(super) fn draw(&self, side: Side, bars: &NSView) {
        for (kind, rect) in self.bars_in_view(side, bars) {
            color(kind).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, 1.5, 1.5).fill();
        }
    }
}

#[cfg(feature = "selftest")]
impl ChangeMarks<'_> {
    pub(super) fn marks(&self, side: Side) -> Vec<Mark> {
        match side {
            Side::Source => self.state.source.borrow().clone(),
            Side::Preview => self.state.preview.borrow().clone(),
        }
    }

    /// What a pane draws right now.
    pub(super) fn drawn(&self, side: Side) -> Vec<(ChangeKind, NSRect)> {
        let Some((source, preview)) = self.state.bars.get() else {
            return Vec::new();
        };
        match side {
            Side::Source => self.bars_in_view(side, source),
            Side::Preview => self.bars_in_view(side, preview),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_covers_its_lines_and_a_removal_marks_the_place() {
        assert_eq!(
            bar_rect(ChangeKind::Added, 4.0, 10.0, 50.0),
            NSRect::new(NSPoint::new(4.0, 10.0), NSSize::new(BAR_WIDTH, 40.0))
        );
        assert_eq!(
            bar_rect(ChangeKind::Removed, 4.0, 10.0, 10.0),
            NSRect::new(NSPoint::new(4.0, 7.0), NSSize::new(BAR_WIDTH, REMOVED_HEIGHT))
        );
        assert_eq!(
            bar_rect(ChangeKind::Modified, 4.0, 10.0, 10.0).size.height,
            2.0,
            "still to be seen"
        );
    }
}
