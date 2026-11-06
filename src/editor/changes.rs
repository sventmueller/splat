//! The marks beside the text for what differs from the last Git commit, in both panes. The repository is read by
//! `splat-git`, never by the `git` program, whose configuration in a foreign repository can start other programs.

use std::cell::{Cell, OnceCell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use objc2::rc::{Retained, Weak};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSBezierPath, NSColor, NSLayoutConstraint, NSScrollView, NSView};
use objc2_foundation::{NSArray, NSObjectProtocol, NSPoint, NSRect, NSSize, NSUserDefaults};
use splat_file_state::{Encoding, FileFormat};
use splat_git::Oid;
use splat_git::diff::{Change, ChangeKind, changes};

use super::EditorController;
use super::layers::ForChanges;
use super::panes::Side;
use crate::markdown::text::{Byte, Utf16Cursor};
use crate::preview::map::preview_offset;
use crate::util::{contained, guarded, lane_of, main_soon, ns, perform_later};

/// User defaults key of the switch in the View menu; without an answer the marks show.
const HIDDEN_KEY: &str = "HidesGitChanges";
/// Long enough that typing does not compare for every key.
const SETTLE_SECONDS: f64 = 0.4;
/// A document beyond this shows no marks; comparing it would take longer than a pause in typing. `MAX_OBJECT` in
/// `splat-git` reads this much text in UTF-16, and no more.
const MOST_BYTES: usize = 32 << 20;
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

/// Where a change stands in a view's text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Mark {
    pub(super) kind: ChangeKind,
    pub(super) start: usize,
    pub(super) end: usize,
}

/// The version of the document in the last commit.
#[derive(Clone)]
struct Base {
    path: PathBuf,
    commit: Oid,
    text: Arc<str>,
}

/// What a look at the repository found.
pub(in crate::editor) enum BaseLook {
    /// HEAD stands where it stood.
    Unchanged,
    /// No repository, or the file is not in the last commit, or it could not be read.
    Nothing,
    New {
        path: PathBuf,
        commit: Oid,
        text: String,
    },
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

/// Reads what the file holds in the last commit, unless HEAD still stands on `known`.
fn read_base(file: &Path, known: Option<Oid>) -> BaseLook {
    let Some(repository) = splat_git::find(file) else {
        return BaseLook::Nothing;
    };
    let Ok(Some(commit)) = repository.head_commit() else {
        return BaseLook::Nothing;
    };
    if known == Some(commit) {
        return BaseLook::Unchanged;
    }
    let Ok(Some(bytes)) = repository.head_version(file) else {
        return BaseLook::Nothing;
    };
    let Some((_, decoded)) = Encoding::read(bytes) else {
        return BaseLook::Nothing;
    };
    let (_, text) = FileFormat::of(&decoded);
    BaseLook::New {
        path: file.to_path_buf(),
        commit,
        text: text.to_string(),
    }
}

/// The marks that reach into the lines from `top` to `bottom`, in the order of the text.
fn in_view(marks: &[Mark], top: usize, bottom: usize) -> &[Mark] {
    let first = marks.partition_point(|m| m.end < top);
    let behind = marks.partition_point(|m| m.start <= bottom);
    marks.get(first..behind.max(first)).unwrap_or(&[])
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

impl ChangeMarks<'_> {
    pub(super) fn install(&self, source: &NSScrollView, preview: &NSScrollView) {
        let object = self.host.object();
        let mtm = self.host.mtm();
        let bars = |side: Side, scroll: &NSScrollView| {
            let height = scroll.bounds().size.height;
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(BAR_ROOM, height));
            let bars = ChangeBars::new(mtm, object, side, frame);
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
        };
        let _ = self
            .state
            .bars
            .set((bars(Side::Source, source), bars(Side::Preview, preview)));
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
        let me = dispatch2::MainThreadBound::new(Weak::from(self.host.object()), self.host.mtm());
        let lane = lane_of(&file);
        lane.ask(
            move || read_base(&file, known),
            move |look| {
                main_soon(move || {
                    let Some(mtm) = MainThreadMarker::new() else { return };
                    if let Some(c) = me.get(mtm).load() {
                        c.changes_answer(ChangesAnswer::Looked(look.unwrap_or(BaseLook::Nothing)));
                    }
                });
            },
        );
    }

    fn document_path(&self) -> Option<PathBuf> {
        let path = self.host.document()?.fileURL()?.path()?.to_string();
        Some(PathBuf::from(path))
    }

    pub(super) fn answer(&self, answer: ChangesAnswer) {
        match answer {
            ChangesAnswer::Looked(look) => {
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
        perform_later(self.host.object(), sel!(compareWithCommit), SETTLE_SECONDS, true);
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
        let me = dispatch2::MainThreadBound::new(Weak::from(self.host.object()), self.host.mtm());
        crate::util::BACKGROUND.ask(
            move || changes(&base.text, &text),
            move |found| {
                main_soon(move || {
                    let Some(mtm) = MainThreadMarker::new() else { return };
                    if let Some(c) = me.get(mtm).load() {
                        c.changes_answer(ChangesAnswer::Compared {
                            generation,
                            version,
                            changes: found.unwrap_or_default(),
                        });
                    }
                });
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
            contained(|| {
                let mut cursor = Utf16Cursor::new(&snap.text);
                let source: Vec<Mark> = found
                    .iter()
                    .map(|change| {
                        let start = cursor.at(Byte::new(change.bytes.start)).get();
                        let end = cursor.at(Byte::new(change.bytes.end)).get();
                        Mark {
                            kind: change.kind,
                            start,
                            end,
                        }
                    })
                    .collect();
                let preview: Vec<Mark> = found
                    .iter()
                    .map(|change| {
                        let start = preview_offset(st.layout(), st.index(), Byte::new(change.bytes.start)).get();
                        // The end of a change is the start of the next line, which in the preview is another paragraph.
                        let last = snap.text.floor_char_boundary(change.bytes.end.saturating_sub(1));
                        let end = if change.bytes.is_empty() {
                            start
                        } else {
                            preview_offset(st.layout(), st.index(), Byte::new(last))
                                .get()
                                .max(start)
                                + 1
                        };
                        Mark {
                            kind: change.kind,
                            start,
                            end,
                        }
                    })
                    .collect();
                (source, preview)
            })
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

    fn mark(start: usize, end: usize) -> Mark {
        Mark {
            kind: ChangeKind::Modified,
            start,
            end,
        }
    }

    #[test]
    fn only_marks_that_reach_into_view_are_looked_at() {
        let marks = [mark(0, 5), mark(10, 20), mark(30, 40), mark(50, 50)];
        assert_eq!(in_view(&marks, 12, 35), &marks[1..3]);
        assert_eq!(in_view(&marks, 21, 29), &[] as &[Mark]);
        assert_eq!(in_view(&marks, 50, 60), &marks[3..]);
        assert_eq!(in_view(&marks, 0, 100), &marks[..]);
        assert_eq!(in_view(&[], 0, 100), &[] as &[Mark]);
    }

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

    #[test]
    fn a_file_outside_a_repository_compares_with_nothing() {
        let dir = std::env::temp_dir().join(format!("splat-no-repository-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.md");
        std::fs::write(&file, "x").unwrap();
        assert!(matches!(read_base(&file, None), BaseLook::Nothing));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_commit_gives_the_text_once_until_head_moves() {
        let dir = std::env::temp_dir().join(format!("splat-repository-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .is_ok_and(|o| o.status.success())
        };
        if !git(&["init", "-q"]) {
            eprintln!("no git here");
            return;
        }
        let file = dir.join("a.md");
        std::fs::write(&file, "\u{FEFF}Text\n").unwrap();
        assert!(git(&["add", "a.md"]));
        assert!(git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "x"
        ]));
        let BaseLook::New { commit, text, .. } = read_base(&file, None) else {
            panic!("the file is in the commit");
        };
        assert_eq!(text, "Text\n", "without its byte order mark, as the text view holds it");
        assert!(matches!(read_base(&file, Some(commit)), BaseLook::Unchanged));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
