//! When an update runs and which blocks of a large preview render when: around a change at once, around what shows while
//! the app is idle. Also which parts of a large source get their colours when.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ops::Range;

use crate::state::Invalidate;

/// Up to this many changed blocks render at once; more show as plain text first and render while idle.
const PROGRESSIVE_FROM: usize = 300;
/// Blocks rendered right away around the visible part when rendering progressively.
const EAGER_BLOCKS: usize = 120;
/// A whole new text of this many UTF-16 units or more is parsed in the background, so the window of a large document
/// opens before its colors and preview are ready.
const BACKGROUND_FROM: usize = 200_000;

/// Whether `invalidate` updates right away rather than through `catch_up`.
pub fn updates_at_once(why: Invalidate, len_u16: usize) -> bool {
    match why {
        Invalidate::Text => len_u16 < BACKGROUND_FROM,
        Invalidate::Preview => true,
        Invalidate::Zoom
        | Invalidate::Appearance
        | Invalidate::Arrivals
        | Invalidate::RemoteAllowed
        | Invalidate::Layout => false,
    }
}

/// Images that finished loading and wait to be shown, by resolved path or URL.
#[derive(Default)]
pub struct Arrivals {
    keys: RefCell<HashSet<String>>,
    scheduled: Cell<bool>,
}

impl Arrivals {
    /// Whether showing them still has to be scheduled.
    pub fn add(&self, key: String) -> bool {
        self.keys.borrow_mut().insert(key);
        !self.scheduled.replace(true)
    }

    /// The images to show now; the next to arrive schedules the showing again.
    pub fn take(&self) -> HashSet<String> {
        self.scheduled.set(false);
        std::mem::take(&mut *self.keys.borrow_mut())
    }

    /// They could not be shown now and have to be scheduled again.
    pub fn put_back(&self, keys: HashSet<String>) {
        self.keys.borrow_mut().extend(keys);
        self.scheduled.set(true);
    }

    /// Whether showing them is scheduled.
    #[cfg(feature = "selftest")]
    pub fn is_scheduled(&self) -> bool {
        self.scheduled.get()
    }
}

/// The changed blocks rendered right away: all of them, or for a large change those around the visible block, which
/// `visible` finds.
pub fn eager_blocks(changed: Range<usize>, visible: impl FnOnce() -> usize) -> Range<usize> {
    if changed.len() <= PROGRESSIVE_FROM {
        return changed;
    }
    let near = visible().clamp(changed.start, changed.end);
    let from = near.saturating_sub(EAGER_BLOCKS / 4).max(changed.start);
    from..(from + EAGER_BLOCKS).min(changed.end)
}

/// Placeholders this many blocks before and after the visible one get rendered while idle.
const RENDER_BEHIND: usize = 150;
const RENDER_AHEAD: usize = 600;
/// And this many paragraphs: a table has one per cell, and every one slows down all later edits of the text.
const RENDER_PARAS_BEHIND: usize = 1_500;
const RENDER_PARAS_AHEAD: usize = 4_500;

/// The blocks around `near`, of `count` in all, to render while idle, given how many paragraphs a block has.
pub fn idle_window(near: usize, count: usize, paras_of: impl Fn(usize) -> usize) -> Range<usize> {
    let fitting = |blocks: &mut dyn Iterator<Item = usize>, mut left: usize| {
        blocks
            .take_while(|&block| {
                let paras = paras_of(block);
                let fits = paras <= left;
                left = left.saturating_sub(paras);
                fits
            })
            .count()
    };
    let behind = fitting(
        &mut (near.saturating_sub(RENDER_BEHIND)..near).rev(),
        RENDER_PARAS_BEHIND,
    );
    let ahead = fitting(&mut (near..(near + RENDER_AHEAD).min(count)), RENDER_PARAS_AHEAD);
    near - behind..near + ahead
}

/// The blocks around those from `top` to `bottom`, of `count` in all. While TextKit lays out a large text, it can name a
/// line for the bottom of the view that stands above the one it names for the top.
pub fn window_around(top: usize, bottom: usize, count: usize) -> Range<usize> {
    let end = (top.max(bottom) + 3).min(count);
    top.min(bottom).saturating_sub(2).min(end)..end
}

/// Colouring a whole source text longer than this colours what shows at once and the rest in steps: at 10 MB, colouring it
/// all held the main thread for a quarter of a second.
pub const COLOR_AT_ONCE: usize = 1 << 20;
/// Bytes coloured in one step, a few milliseconds of work.
pub const COLOR_STEP: usize = 128 * 1024;

/// The parts of the source, in bytes of the text it holds, that do not have the colours of that text yet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Uncolored(Vec<Range<usize>>);

impl Uncolored {
    /// All of a text of `len` bytes but `done`.
    pub fn all_but(len: usize, done: Range<usize>) -> Uncolored {
        let (start, end) = (done.start.min(len), done.end.min(len));
        Uncolored([0..start, end..len].into_iter().filter(|r| !r.is_empty()).collect())
    }

    /// Whether all of the text has its colours.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Follows an edit that put `inserted` bytes in place of the bytes `removed`; the edit's own colouring covers what it
    /// wrote.
    pub fn edited(&mut self, removed: Range<usize>, inserted: usize) {
        let moved = |at: usize| {
            if at <= removed.start {
                at
            } else if at >= removed.end {
                at - removed.end + removed.start + inserted
            } else {
                removed.start + inserted
            }
        };
        self.0 = self
            .0
            .iter()
            .map(|r| moved(r.start)..moved(r.end))
            .filter(|r| !r.is_empty())
            .collect();
    }

    /// Takes the next part to colour, at most about `most` bytes of `text` from the one nearest to `near` on, in whole
    /// lines, so that no span and no line direction is cut.
    pub fn take_near(&mut self, text: &str, near: usize, most: usize) -> Option<Range<usize>> {
        self.within(text.len());
        let distance = |r: &Range<usize>| {
            if near < r.start {
                r.start - near
            } else {
                near.saturating_sub(r.end - 1)
            }
        };
        // Of two as near, the one below, where reading goes on.
        let left = self.0.iter().min_by_key(|r| (distance(r), r.end <= near))?.clone();
        let from = if near >= left.end {
            left.end.saturating_sub(most).max(left.start)
        } else {
            near.clamp(left.start, left.end)
        };
        let piece = lines_around(text, from..(from + most).min(left.end));
        self.cut(piece.clone());
        Some(piece)
    }

    /// Takes the parts within `window` of `text`, in whole lines.
    pub fn take_within(&mut self, text: &str, window: Range<usize>) -> Vec<Range<usize>> {
        self.within(text.len());
        let pieces: Vec<Range<usize>> = self
            .0
            .iter()
            .filter(|r| r.start < window.end && window.start < r.end)
            .map(|r| lines_around(text, r.start.max(window.start)..r.end.min(window.end)))
            .collect();
        for piece in &pieces {
            self.cut(piece.clone());
        }
        pieces
    }

    /// Nothing beyond the text is left to colour.
    fn within(&mut self, len: usize) {
        for r in &mut self.0 {
            r.end = r.end.min(len);
        }
        self.0.retain(|r| r.start < r.end);
    }

    fn cut(&mut self, piece: Range<usize>) {
        self.0 = self
            .0
            .iter()
            .flat_map(|r| [r.start..r.end.min(piece.start), r.start.max(piece.end)..r.end])
            .filter(|r| !r.is_empty())
            .collect();
    }
}

/// `range` widened to the start of its first line and the end of its last, within `text`.
pub fn lines_around(text: &str, range: Range<usize>) -> Range<usize> {
    let bytes = text.as_bytes();
    let end = range.end.min(bytes.len());
    let start = range.start.min(end);
    let start = bytes[..start].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    let end = if end == 0 || bytes.get(end - 1) == Some(&b'\n') {
        end
    } else {
        bytes[end..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |i| end + i + 1)
    };
    start..end
}

/// Where the background parse of a document stands. Each one holds a copy of the text, so while keys come faster than a
/// parse only the newest waits.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Parsing {
    /// No parse runs.
    #[default]
    Idle,
    /// One runs, and none is due behind it.
    Running,
    /// Another is due when this one ends.
    RunningWithAnotherDue,
}

impl Parsing {
    /// What asking for a parse comes to, and whether one starts now.
    pub fn asked(self) -> (Parsing, bool) {
        match self {
            Parsing::Idle => (Parsing::Running, true),
            Parsing::Running | Parsing::RunningWithAnotherDue => (Parsing::RunningWithAnotherDue, false),
        }
    }

    /// What the end of a parse comes to, and whether another one is due.
    pub fn ended(self) -> (Parsing, bool) {
        debug_assert_ne!(self, Parsing::Idle, "a parse ended that never started");
        (Parsing::Idle, self == Parsing::RunningWithAnotherDue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_source_is_coloured_from_what_shows_outwards() {
        let text = "eins\nzwei\ndrei\nvier\nfünf\nsechs\n";
        let mut left = Uncolored::all_but(text.len(), 10..15);
        assert_eq!(left, Uncolored(vec![0..10, 15..text.len()]));
        let mut order = Vec::new();
        while let Some(piece) = left.take_near(text, 12, 4) {
            assert!(text.is_char_boundary(piece.start) && text.is_char_boundary(piece.end));
            assert!(
                piece.start == 0 || text.as_bytes()[piece.start - 1] == b'\n',
                "{piece:?} starts a line"
            );
            order.push(piece);
        }
        assert!(left.is_empty());
        assert_eq!(order.first(), Some(&(15..20)), "the line after what shows comes first");
        let covered: usize = order.iter().map(|p| p.len()).sum();
        assert!(covered >= text.len() - 5, "{order:?}");
        assert!(
            order.iter().any(|p| p.start == 0),
            "the part above comes last: {order:?}"
        );
    }

    #[test]
    fn what_is_left_to_colour_moves_with_an_edit() {
        let mut left = Uncolored(vec![0..4, 10..20, 30..40]);
        left.edited(12..15, 1);
        assert_eq!(left, Uncolored(vec![0..4, 10..18, 28..38]));
        left.edited(0..4, 0);
        assert_eq!(
            left,
            Uncolored(vec![6..14, 24..34]),
            "the removed part went with the edit"
        );
        let mut beyond = Uncolored::all_but(50, 0..2);
        assert_eq!(beyond.take_near("ab\ncd", 0, 100), Some(0..5));
        assert!(beyond.is_empty(), "nothing is left past the end of the text");
        assert_eq!(beyond.take_near("ab", 0, 10), None);
    }

    #[test]
    fn what_scrolls_into_view_is_taken_in_whole_lines() {
        let text = "aa\nbb\ncc\ndd\n";
        let mut left = Uncolored::all_but(text.len(), 0..3);
        assert_eq!(left.take_within(text, 4..7), vec![3..9]);
        assert_eq!(left, Uncolored::all_but(text.len(), 0..9));
        assert!(left.take_within(text, 0..2).is_empty());
        assert_eq!(lines_around(text, 0..0), 0..0);
        assert_eq!(lines_around(text, 4..4), 3..6);
        assert_eq!(lines_around("ohne Ende", 2..4), 0..9);
    }

    #[test]
    fn only_the_newest_parse_waits_behind_the_one_running() {
        let (running, starts) = Parsing::Idle.asked();
        assert_eq!((running, starts), (Parsing::Running, true));
        let (due, starts) = running.asked();
        assert_eq!((due, starts), (Parsing::RunningWithAnotherDue, false));
        assert_eq!(due.asked(), (Parsing::RunningWithAnotherDue, false), "still one");
        assert_eq!(due.ended(), (Parsing::Idle, true));
        assert_eq!(running.ended(), (Parsing::Idle, false));
    }

    #[test]
    fn small_changes_render_at_once_large_ones_around_the_visible_block() {
        assert_eq!(eager_blocks(3..40, || unreachable!()), 3..40);
        let changed = 10..10 + 3 * PROGRESSIVE_FROM;
        for visible in [0, 10, 11, 500, changed.end - 1, changed.end, changed.end + 50] {
            let eager = eager_blocks(changed.clone(), || visible);
            assert!(eager.len() <= EAGER_BLOCKS, "{eager:?} for {visible}");
            assert!(
                changed.start <= eager.start && eager.end <= changed.end,
                "{eager:?} for {visible}"
            );
            let seen = visible.clamp(changed.start, changed.end - 1);
            assert!(eager.contains(&seen), "{eager:?} misses {seen}");
        }
    }

    #[test]
    fn images_that_arrive_together_are_shown_together() {
        let arrivals = Arrivals::default();
        assert!(arrivals.add("a".into()), "the first one schedules the showing");
        assert!(!arrivals.add("b".into()), "the next ones wait for it");
        assert_eq!(arrivals.take().len(), 2);
        assert!(arrivals.take().is_empty());
        assert!(
            arrivals.add("c".into()),
            "the next arrival after the showing schedules anew"
        );
    }

    #[test]
    fn images_that_could_not_be_shown_wait_with_the_ones_arriving_meanwhile() {
        let arrivals = Arrivals::default();
        arrivals.add("a".into());
        arrivals.put_back(arrivals.take());
        assert!(
            !arrivals.add("b".into()),
            "putting them back comes with its own schedule"
        );
        assert_eq!(arrivals.take().len(), 2);
    }

    #[test]
    fn only_a_large_new_text_waits_for_the_background() {
        assert!(updates_at_once(Invalidate::Text, 0));
        assert!(updates_at_once(Invalidate::Text, BACKGROUND_FROM - 1));
        assert!(!updates_at_once(Invalidate::Text, BACKGROUND_FROM));
        assert!(updates_at_once(Invalidate::Preview, BACKGROUND_FROM));
        for why in [
            Invalidate::Zoom,
            Invalidate::Appearance,
            Invalidate::Arrivals,
            Invalidate::RemoteAllowed,
            Invalidate::Layout,
        ] {
            assert!(!updates_at_once(why, 0), "{why:?}");
        }
    }

    #[test]
    fn the_window_stays_inside_the_blocks_whatever_textkit_answers() {
        assert_eq!(window_around(10, 20, 100), 8..23);
        assert_eq!(window_around(0, 0, 100), 0..3);
        assert_eq!(window_around(98, 99, 100), 96..100);
        assert_eq!(window_around(35_933, 35_926, 40_000), 35_924..35_936);
        assert_eq!(window_around(500, 600, 100), 100..100);
        assert_eq!(window_around(0, 0, 0), 0..0);
    }

    #[test]
    fn idle_rendering_stops_at_as_many_blocks_as_ever_for_ordinary_text() {
        assert_eq!(idle_window(1_000, 10_000, |_| 1), 850..1_600);
        assert_eq!(idle_window(0, 10_000, |_| 1), 0..600);
        assert_eq!(idle_window(9_990, 10_000, |_| 1), 9_840..10_000);
        assert_eq!(idle_window(0, 0, |_| 1), 0..0);
    }

    #[test]
    fn idle_rendering_counts_the_cells_of_tables() {
        assert_eq!(idle_window(1_000, 10_000, |_| 153), 991..1_029);
        assert_eq!(idle_window(1_000, 10_000, |_| 10_000), 1_000..1_000);
    }
}
