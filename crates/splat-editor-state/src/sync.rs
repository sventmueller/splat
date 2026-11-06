//! Bookkeeping that keeps the editor's copies in step with its text views: the copy of the source text, patched
//! edit by edit, and the blocks the preview view shows. Free of AppKit, so it is tested here against plain
//! reference implementations.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::markdown::text::{Byte, TextAt, U16, byte_at_u16, byte_at_u16_exact, offsets, units, utf16_len};
use crate::preview::map::Shown;

/// A length as a text storage counts a change of it; no text comes near the difference.
fn signed(n: usize) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

/// The source text as the source view holds it.
#[derive(Clone)]
pub struct Snapshot {
    /// The text.
    pub text: String,
    /// The version of the source the text is, counted in edits.
    pub version: u64,
    /// The length of the text in UTF-16 units.
    pub len_u16: usize,
    /// A position and where it lies in the text; typing goes on near the last edit.
    mark: (U16, Byte),
}

impl Snapshot {
    /// A copy of `text`, which is `len_u16` units long, at `version`.
    pub fn new(text: String, len_u16: usize, version: u64) -> Self {
        Snapshot {
            text,
            version,
            len_u16,
            mark: (U16::new(0), Byte::default()),
        }
    }

    /// Applies an edit as a text storage reports it: `range` holds `inserted` in the new text and the length changed
    /// by `delta`. `None` when the edit does not fit this copy, e.g. because it splits a surrogate pair.
    pub fn patched(mut self, range: Range<U16>, delta: isize, inserted: &str, version: u64) -> Option<Snapshot> {
        let new_len = units(&range);
        let old_len = usize::try_from(signed(new_len) - delta).ok()?;
        let (mark_u16, mark_byte) = if self.mark.0 <= range.start {
            self.mark
        } else {
            (U16::new(0), Byte::default())
        };
        let start = byte_at_u16_exact(&self.text, mark_byte, range.start - mark_u16)?;
        let end = byte_at_u16_exact(&self.text, start, old_len)?;
        self.text.replace_range(offsets(&(start..end)), inserted);
        self.version = version;
        self.len_u16 = (self.len_u16 + new_len).checked_sub(old_len)?;
        // Text before the edit is unchanged.
        self.mark = (range.start, start);
        Some(self)
    }

    /// The position of `byte`; counting starts at the last edit when `byte` lies behind it.
    pub fn u16_at(&self, byte: Byte) -> U16 {
        let byte = Byte::new(self.text.floor_char_boundary(byte.get().min(self.text.len())));
        if byte >= self.mark.1 {
            self.mark.0 + utf16_len(self.text.at(self.mark.1..byte))
        } else {
            U16::new(utf16_len(self.text.upto(byte)))
        }
    }

    /// The char boundary at or after a position.
    pub fn byte_at(&self, position: U16) -> Byte {
        if position >= self.mark.0 {
            byte_at_u16(&self.text, self.mark.1, position - self.mark.0)
        } else {
            byte_at_u16(&self.text, Byte::default(), position.get())
        }
    }

    /// A range of the text as a range of positions.
    pub fn u16_range(&self, bytes: &Range<Byte>) -> Range<U16> {
        let start = self.u16_at(bytes.start);
        let from = self.text.floor_char_boundary(bytes.start.get().min(self.text.len()));
        let to = self
            .text
            .floor_char_boundary(bytes.end.get().min(self.text.len()))
            .max(from);
        start..start + utf16_len(&self.text[from..to])
    }

    /// A range of positions as a range of the text.
    pub fn byte_range(&self, range: &Range<U16>) -> Range<Byte> {
        let start = self.byte_at(range.start);
        start..byte_at_u16(&self.text, start, range.end.get().saturating_sub(range.start.get()))
    }
}

/// The source text as last read from the view, kept in step with every edit, and which update is the newest.
#[derive(Default)]
pub struct SourceCopy {
    snapshot: RefCell<Option<Rc<Snapshot>>>,
    /// Counts the character edits of the source storage.
    version: Cell<u64>,
    generation: Cell<u64>,
    /// The generation again, for a background job to look at; it stops when a newer update has started.
    latest: Arc<AtomicU64>,
    update_scheduled: Cell<bool>,
}

impl SourceCopy {
    /// How many edits the source has had.
    pub fn version(&self) -> u64 {
        self.version.get()
    }

    /// An edit of the source as a text storage reports it, whose length is then `length`. A copy that went out of step
    /// would end up in every command and the preview, so it is dropped then and read anew.
    pub fn edited(&self, range: Range<U16>, delta: isize, inserted: &str, length: usize) {
        let version = self.version.get() + 1;
        self.version.set(version);
        let mut slot = self.snapshot.borrow_mut();
        *slot = slot.take().and_then(|snap| {
            let snap = Rc::try_unwrap(snap).unwrap_or_else(|shared| (*shared).clone());
            snap.patched(range, delta, inserted, version)
                .filter(|s| s.len_u16 == length)
                .map(Rc::new)
        });
    }

    /// The current text; `read` gets it from the view when edits could not patch it in place.
    pub fn current(&self, read: impl FnOnce() -> (String, usize)) -> Rc<Snapshot> {
        let version = self.version.get();
        if let Some(s) = self.snapshot.borrow().as_ref().filter(|s| s.version == version) {
            return s.clone();
        }
        let (text, len_u16) = read();
        let s = Rc::new(Snapshot::new(text, len_u16, version));
        *self.snapshot.borrow_mut() = Some(s.clone());
        s
    }

    /// An update starts: every result of an older one is outdated from now on.
    pub fn next_generation(&self) -> u64 {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.latest.store(generation, Ordering::SeqCst);
        generation
    }

    /// Whether `generation` is the update that started last.
    pub fn is_latest(&self, generation: u64) -> bool {
        self.generation.get() == generation
    }

    /// The generation as a background job can read it.
    pub fn latest_generation(&self) -> Arc<AtomicU64> {
        self.latest.clone()
    }

    /// Whether this call is the one that has to schedule the update.
    pub fn schedule_update(&self) -> bool {
        !self.update_scheduled.replace(true)
    }

    #[cfg(any(test, feature = "selftest"))]
    /// Whether an update is scheduled and has not run yet.
    pub fn update_is_scheduled(&self) -> bool {
        self.update_scheduled.get()
    }

    /// The scheduled update ran; the next edit schedules one again.
    pub fn update_ran(&self) {
        self.update_scheduled.set(false);
    }
}

/// The blocks the preview view shows, in order: their layout keys, UTF-16 lengths and whether each is still shown
/// as plain text, waiting to be rendered.
#[derive(Default)]
pub struct Blocks {
    /// The layout key of each block.
    pub keys: Vec<u64>,
    /// The UTF-16 length of each block in the preview text.
    pub lens: Vec<usize>,
    /// Whether each block still shows as plain text.
    pub placeholders: Vec<bool>,
}

/// Which blocks of the preview change when a new layout replaces the shown one.
#[derive(Debug, PartialEq, Eq)]
pub struct BlockDiff {
    /// Number of unchanged blocks at the start and at the end.
    pub prefix: usize,
    /// The unchanged blocks at the end.
    pub suffix: usize,
    /// The part of the preview text that the changed blocks replace.
    pub replace: Range<Shown>,
}

impl Blocks {
    /// Forgets what the preview shows, so the next update replaces all of it.
    pub fn forget(&mut self) {
        self.keys.clear();
        self.lens.clear();
        self.placeholders.clear();
    }

    /// Marks blocks for rendering again although their key is unchanged, e.g. because an image in them arrived.
    pub fn invalidate(&mut self, stale: impl Fn(usize) -> bool) {
        for (i, key) in self.keys.iter_mut().enumerate() {
            if stale(i) {
                *key = 0;
            }
        }
    }

    /// `edited` is the range of the shown text (before the edit) that the user changed directly, with the change in
    /// length; every block it touches is replaced. `shown_len` is the current length of the preview text.
    pub fn diff(&self, new_keys: &[u64], edited: Option<(Range<Shown>, isize)>, shown_len: usize) -> BlockDiff {
        let old_count = self.keys.len();
        let mut prefix = self.keys.iter().zip(new_keys).take_while(|(a, b)| a == b).count();
        let max_suffix = old_count.min(new_keys.len()) - prefix;
        let mut suffix = self
            .keys
            .iter()
            .rev()
            .zip(new_keys.iter().rev())
            .take(max_suffix)
            .take_while(|(a, b)| a == b)
            .count();
        if old_count == 0 {
            return BlockDiff {
                prefix: 0,
                suffix: 0,
                replace: Shown::new(0)..Shown::new(shown_len),
            };
        }
        let mut delta = 0isize;
        if let Some((dirty, d)) = edited {
            delta = d;
            let mut pos = 0;
            for (i, len) in self.lens.iter().enumerate() {
                let end = pos + len;
                if dirty.start.get() <= end && dirty.end.get() >= pos {
                    prefix = prefix.min(i);
                    suffix = suffix.min(old_count - 1 - i);
                }
                pos = end;
            }
        }
        let start: usize = self.lens[..prefix].iter().sum();
        let end: usize = self.lens[..old_count - suffix].iter().sum();
        let end = usize::try_from(signed(end) + delta).unwrap_or(0).max(start);
        let start = start.min(shown_len);
        BlockDiff {
            prefix,
            suffix,
            replace: Shown::new(start)..Shown::new(end.clamp(start, shown_len)),
        }
    }

    /// Takes over the new layout's blocks after `diff` was applied; the changed blocks outside `eager` are
    /// placeholders.
    pub fn replace(&mut self, diff: &BlockDiff, new_keys: Vec<u64>, new_lens: Vec<usize>, eager: &Range<usize>) {
        let old_count = self.keys.len();
        let known = self.placeholders.len() == old_count;
        let mut placeholders = if known {
            self.placeholders[..diff.prefix].to_vec()
        } else {
            vec![false; diff.prefix]
        };
        placeholders.extend((diff.prefix..new_keys.len() - diff.suffix).map(|i| !eager.contains(&i)));
        if known {
            placeholders.extend_from_slice(&self.placeholders[old_count - diff.suffix..]);
        } else {
            placeholders.extend(std::iter::repeat_n(false, diff.suffix));
        }
        self.placeholders = placeholders;
        self.keys = new_keys;
        self.lens = new_lens;
    }

    /// The blocks of `changed` outside `eager` went into the view as plain text.
    pub fn left_plain(&mut self, changed: Range<usize>, eager: &Range<usize>) {
        for i in changed.filter(|i| !eager.contains(i)) {
            self.placeholders[i] = true;
        }
    }

    /// Whether any block waits to be rendered.
    pub fn pending(&self) -> bool {
        self.placeholders.iter().any(|p| *p)
    }
}

/// The changed part of a text, as the ranges it covers in `old` and in `new`, preferring a change that ends at
/// `caret` in `new` when the change is ambiguous, like a letter typed next to the same letter.
pub fn utf16_diff(old: &[u16], new: &[u16], caret: usize) -> (Range<usize>, Range<usize>) {
    let max_suffix = new.len().saturating_sub(caret).min(old.len()).min(new.len());
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    let max_prefix = (old.len() - suffix).min(new.len() - suffix);
    let prefix = old.iter().zip(new).take(max_prefix).take_while(|(a, b)| a == b).count();
    (prefix..old.len() - suffix, prefix..new.len() - suffix)
}

#[cfg(test)]
mod tests {
    use splat_markdown::text::bytes;
    use splat_testkit::{Rng, rounds};

    use super::*;

    fn random_text(rng: &mut Rng, max: usize) -> String {
        const PIECES: [&str; 6] = ["a", "b", " ", "ä", "😀", "\n"];
        (0..rng.below(max + 1)).map(|_| *rng.pick(&PIECES)).collect()
    }

    /// UTF-16 offsets that do not split a surrogate pair.
    fn boundaries(units: &[u16]) -> Vec<usize> {
        (0..=units.len())
            .filter(|&i| i == units.len() || !(0xDC00..0xE000).contains(&units[i]))
            .collect()
    }

    #[test]
    fn patched_copies_match_the_edited_text() {
        let mut rng = Rng::new(0x5eed_0001);
        for _ in 0..rounds(2000) {
            let mut reference: Vec<u16> = random_text(&mut rng, 30).encode_utf16().collect();
            let mut snap = Snapshot::new(String::from_utf16(&reference).unwrap(), reference.len(), 0);
            for version in 1..6 {
                let cuts = boundaries(&reference);
                let (a, b) = (cuts[rng.below(cuts.len())], cuts[rng.below(cuts.len())]);
                let (a, b) = (a.min(b), a.max(b));
                let inserted = random_text(&mut rng, 4);
                let new: Vec<u16> = inserted.encode_utf16().collect();
                reference.splice(a..b, new.iter().copied());
                let delta = signed(new.len()) - signed(b - a);
                snap = snap
                    .patched(U16::new(a)..U16::new(a + new.len()), delta, &inserted, version)
                    .expect("edit fits");
                assert_eq!(snap.text, String::from_utf16(&reference).unwrap());
                assert_eq!(snap.len_u16, reference.len());
                for &u in &boundaries(&reference) {
                    let byte = snap.byte_at(U16::new(u));
                    assert_eq!(snap.u16_at(byte), U16::new(u), "{:?} at {u}", snap.text);
                }
            }
        }
    }

    #[test]
    fn the_copy_follows_edits_and_is_read_anew_when_it_cannot() {
        let copy = SourceCopy::default();
        let read = |text: &str| {
            let text = text.to_string();
            move || {
                let len = text.encode_utf16().count();
                (text, len)
            }
        };
        assert_eq!(copy.current(read("ab")).text, "ab");
        copy.edited(U16::new(1)..U16::new(2), 1, "x", 3);
        assert_eq!(copy.current(|| panic!("no need to read")).text, "axb");
        // A length the copy does not reach means it went out of step.
        copy.edited(U16::new(0)..U16::new(1), 1, "y", 99);
        assert_eq!(copy.current(read("neu")).text, "neu");
    }

    #[test]
    fn only_the_newest_update_counts_and_background_jobs_can_tell() {
        let copy = SourceCopy::default();
        let first = copy.next_generation();
        let latest = copy.latest_generation();
        assert!(copy.is_latest(first));
        assert_eq!(latest.load(Ordering::SeqCst), first);
        let second = copy.next_generation();
        assert!(!copy.is_latest(first) && copy.is_latest(second));
        assert_eq!(
            latest.load(Ordering::SeqCst),
            second,
            "every update stops the job of the one before"
        );
    }

    #[test]
    fn an_update_is_scheduled_once_until_it_ran() {
        let copy = SourceCopy::default();
        assert!(copy.schedule_update());
        assert!(!copy.schedule_update() && copy.update_is_scheduled());
        copy.update_ran();
        assert!(copy.schedule_update());
    }

    #[test]
    fn edits_splitting_a_surrogate_pair_are_refused() {
        let snap = Snapshot::new("a😀b".into(), 4, 0);
        assert!(snap.clone().patched(U16::new(2)..U16::new(3), 0, "x", 1).is_none());
        assert!(snap.clone().patched(U16::new(2)..U16::new(2), -1, "", 1).is_none());
        assert_eq!(snap.patched(U16::new(1)..U16::new(2), -1, "x", 1).unwrap().text, "axb");
    }

    #[test]
    fn ranges_convert_both_ways() {
        let mut snap = Snapshot::new("ä😀 b".into(), 5, 0);
        snap = snap.patched(U16::new(1)..U16::new(2), 1, "x", 1).unwrap();
        assert_eq!(snap.text, "äx😀 b");
        assert_eq!(snap.u16_range(&bytes(2..3)), U16::new(1)..U16::new(2));
        assert_eq!(snap.u16_range(&bytes(3..7)), U16::new(2)..U16::new(4));
        assert_eq!(snap.byte_range(&(U16::new(2)..U16::new(4))), bytes(3..7));
        assert_eq!(snap.u16_range(&bytes(0..100)), U16::new(0)..U16::new(6));
        // A byte offset inside a character counts up to the character's start.
        assert_eq!(snap.u16_at(Byte::new(1)), U16::new(0));
    }

    fn content(key: u64) -> String {
        let pieces = ["x", "ä", "😀", "\n"];
        (0..=(key % 5) as usize)
            .map(|i| pieces[(usize::try_from(key).unwrap_or(usize::MAX) + i) % 4])
            .collect()
    }

    fn shown(keys: &[u64]) -> Vec<u16> {
        keys.iter()
            .flat_map(|k| content(*k).encode_utf16().collect::<Vec<_>>())
            .collect()
    }

    fn blocks(keys: &[u64]) -> Blocks {
        Blocks {
            keys: keys.to_vec(),
            lens: keys.iter().map(|k| content(*k).encode_utf16().count()).collect(),
            placeholders: vec![false; keys.len()],
        }
    }

    /// Replacing the range a diff names with the changed new blocks must show exactly the new blocks, also after the
    /// user typed into the preview, which changes the shown text without changing a key.
    #[test]
    fn block_diffs_replace_exactly_what_changed() {
        let mut rng = Rng::new(0x5eed_0002);
        for _ in 0..rounds(2000) {
            let old: Vec<u64> = (0..rng.below(8)).map(|_| 1 + rng.below(6) as u64).collect();
            let mut new = old.clone();
            for _ in 0..rng.below(4) {
                match rng.below(3) {
                    0 => new.insert(rng.below(new.len() + 1), 1 + rng.below(6) as u64),
                    1 if !new.is_empty() => {
                        new.remove(rng.below(new.len()));
                    }
                    _ if !new.is_empty() => {
                        let i = rng.below(new.len());
                        new[i] = 1 + rng.below(6) as u64;
                    }
                    _ => {}
                }
            }
            let state = blocks(&old);
            let mut text = shown(&old);
            let mut edited = None;
            if rng.below(2) == 0 {
                let cuts = boundaries(&text);
                let (a, b) = (cuts[rng.below(cuts.len())], cuts[rng.below(cuts.len())]);
                let (a, b) = (a.min(b), a.max(b));
                let typed: Vec<u16> = random_text(&mut rng, 2).encode_utf16().collect();
                edited = Some((a..b, signed(typed.len()) - signed(b - a)));
                text.splice(a..b, typed);
            }
            let diff = state.diff(
                &new,
                edited.clone().map(|(r, d)| (Shown::new(r.start)..Shown::new(r.end), d)),
                text.len(),
            );
            text.splice(
                diff.replace.start.get()..diff.replace.end.get(),
                shown(&new[diff.prefix..new.len() - diff.suffix]),
            );
            assert_eq!(text, shown(&new), "{old:?} -> {new:?}, edited {edited:?}: {diff:?}");
        }
    }

    #[test]
    fn placeholders_follow_their_blocks() {
        let mut state = blocks(&[1, 2, 3, 4]);
        state.placeholders = vec![false, true, false, true];
        let new = vec![1, 5, 6, 3, 4];
        let diff = state.diff(&new, None, shown(&[1, 2, 3, 4]).len());
        assert_eq!((diff.prefix, diff.suffix), (1, 2));
        let lens = new.iter().map(|k| content(*k).encode_utf16().count()).collect();
        state.replace(&diff, new, lens, &(1..3));
        assert_eq!(state.placeholders, [false, false, false, false, true]);
        state.left_plain(1..3, &(1..2));
        assert_eq!(state.placeholders, [false, false, true, false, true]);
        assert!(state.pending());
        state.forget();
        assert_eq!(
            state.diff(&[1], None, 7),
            BlockDiff {
                prefix: 0,
                suffix: 0,
                replace: Shown::new(0)..Shown::new(7)
            }
        );
    }

    #[test]
    fn text_diffs_reproduce_the_new_text() {
        let mut rng = Rng::new(0x5eed_0003);
        for _ in 0..rounds(2000) {
            let old: Vec<u16> = random_text(&mut rng, 12).encode_utf16().collect();
            let cuts = boundaries(&old);
            let (a, b) = (cuts[rng.below(cuts.len())], cuts[rng.below(cuts.len())]);
            let (a, b) = (a.min(b), a.max(b));
            let typed: Vec<u16> = random_text(&mut rng, 3).encode_utf16().collect();
            let mut new = old.clone();
            new.splice(a..b, typed.iter().copied());
            let caret = a + typed.len();
            let (o, n) = utf16_diff(&old, &new, caret);
            let mut rebuilt = old.clone();
            rebuilt.splice(o, new[n].iter().copied());
            assert_eq!(rebuilt, new);
        }
    }

    #[test]
    fn diff_anchors_at_caret() {
        let old: Vec<u16> = "aab".encode_utf16().collect();
        let new: Vec<u16> = "aaab".encode_utf16().collect();
        assert_eq!(utf16_diff(&old, &new, 2), (1..1, 1..2));
        assert_eq!(utf16_diff(&old, &new, 3), (2..2, 2..3));
    }
}
