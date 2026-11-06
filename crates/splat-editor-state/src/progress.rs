//! When an update runs and which blocks of a large preview render when: around a change at once, around what shows while
//! the app is idle.

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
