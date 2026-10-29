//! Bookkeeping that keeps the editor's copies in step with its text views: the copy of the source text, patched
//! edit by edit, and the blocks the preview view shows. Free of AppKit, so it is tested here against plain
//! reference implementations.

use std::ops::Range;

use crate::markdown::text::{byte_at_u16, byte_at_u16_exact, utf16_len};

/// The source text as the source view holds it.
#[derive(Clone)]
pub struct Snapshot {
    pub text: String,
    pub version: u64,
    pub len_u16: usize,
    /// A UTF-16 offset and the byte offset it maps to; typing goes on near the last edit.
    mark: (usize, usize),
}

impl Snapshot {
    pub fn new(text: String, len_u16: usize, version: u64) -> Self {
        Snapshot {
            text,
            version,
            len_u16,
            mark: (0, 0),
        }
    }

    /// Applies an edit as a text storage reports it: `range` holds `inserted` in the new text and the length changed
    /// by `delta`. `None` when the edit does not fit this copy, e.g. because it splits a surrogate pair.
    pub fn patched(mut self, range: Range<usize>, delta: isize, inserted: &str, version: u64) -> Option<Snapshot> {
        let old_len = usize::try_from(range.len() as isize - delta).ok()?;
        let (mark_u16, mark_byte) = if self.mark.0 <= range.start { self.mark } else { (0, 0) };
        let start = byte_at_u16_exact(&self.text, mark_byte, range.start - mark_u16)?;
        let end = byte_at_u16_exact(&self.text, start, old_len)?;
        self.text.replace_range(start..end, inserted);
        self.version = version;
        self.len_u16 = (self.len_u16 + range.len()).checked_sub(old_len)?;
        // Text before the edit is unchanged.
        self.mark = (range.start, start);
        Some(self)
    }

    /// UTF-16 offset of a byte offset; counting starts at the last edit when the offset lies behind it.
    pub fn u16_at(&self, byte: usize) -> usize {
        let byte = self.text.floor_char_boundary(byte.min(self.text.len()));
        if byte >= self.mark.1 {
            self.mark.0 + utf16_len(&self.text[self.mark.1..byte])
        } else {
            utf16_len(&self.text[..byte])
        }
    }

    /// Byte offset of the char boundary at or after a UTF-16 offset.
    pub fn byte_at(&self, units: usize) -> usize {
        if units >= self.mark.0 {
            byte_at_u16(&self.text, self.mark.1, units - self.mark.0)
        } else {
            byte_at_u16(&self.text, 0, units)
        }
    }

    /// A byte range as UTF-16 range.
    pub fn u16_range(&self, bytes: &Range<usize>) -> Range<usize> {
        let start = self.u16_at(bytes.start);
        let from = self.text.floor_char_boundary(bytes.start.min(self.text.len()));
        let to = self.text.floor_char_boundary(bytes.end.min(self.text.len())).max(from);
        start..start + utf16_len(&self.text[from..to])
    }

    /// A UTF-16 range as byte range.
    pub fn byte_range(&self, units: &Range<usize>) -> Range<usize> {
        let start = self.byte_at(units.start);
        start..byte_at_u16(&self.text, start, units.end.saturating_sub(units.start))
    }
}

/// The blocks the preview view shows, in order: their layout keys, UTF-16 lengths and whether each is still shown
/// as plain text, waiting to be rendered.
#[derive(Default)]
pub struct Blocks {
    pub keys: Vec<u64>,
    pub lens: Vec<usize>,
    pub placeholders: Vec<bool>,
}

/// Which blocks of the preview change when a new layout replaces the shown one.
#[derive(Debug, PartialEq, Eq)]
pub struct BlockDiff {
    /// Number of unchanged blocks at the start and at the end.
    pub prefix: usize,
    pub suffix: usize,
    /// UTF-16 range of the preview text that the changed blocks replace.
    pub replace: Range<usize>,
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
    pub fn diff(&self, new_keys: &[u64], edited: Option<(Range<usize>, isize)>, shown_len: usize) -> BlockDiff {
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
                replace: 0..shown_len,
            };
        }
        let mut delta = 0isize;
        if let Some((dirty, d)) = edited {
            delta = d;
            let mut pos = 0;
            for (i, len) in self.lens.iter().enumerate() {
                let end = pos + len;
                if dirty.start <= end && dirty.end >= pos {
                    prefix = prefix.min(i);
                    suffix = suffix.min(old_count - 1 - i);
                }
                pos = end;
            }
        }
        let start: usize = self.lens[..prefix].iter().sum();
        let end: usize = self.lens[..old_count - suffix].iter().sum();
        let end = (end as isize + delta).max(start as isize) as usize;
        let start = start.min(shown_len);
        BlockDiff {
            prefix,
            suffix,
            replace: start..end.clamp(start, shown_len),
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
    use splat::fuzz::{Rng, rounds};

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
                let delta = new.len() as isize - (b - a) as isize;
                snap = snap
                    .patched(a..a + new.len(), delta, &inserted, version)
                    .expect("edit fits");
                assert_eq!(snap.text, String::from_utf16(&reference).unwrap());
                assert_eq!(snap.len_u16, reference.len());
                for &u in &boundaries(&reference) {
                    let byte = snap.byte_at(u);
                    assert_eq!(snap.u16_at(byte), u, "{:?} at {u}", snap.text);
                }
            }
        }
    }

    #[test]
    fn edits_splitting_a_surrogate_pair_are_refused() {
        let snap = Snapshot::new("a😀b".into(), 4, 0);
        assert!(snap.clone().patched(2..3, 0, "x", 1).is_none());
        assert!(snap.clone().patched(2..2, -1, "", 1).is_none());
        assert_eq!(snap.patched(1..2, -1, "x", 1).unwrap().text, "axb");
    }

    #[test]
    fn ranges_convert_both_ways() {
        let mut snap = Snapshot::new("ä😀 b".into(), 5, 0);
        snap = snap.patched(1..2, 1, "x", 1).unwrap();
        assert_eq!(snap.text, "äx😀 b");
        assert_eq!(snap.u16_range(&(2..3)), 1..2);
        assert_eq!(snap.u16_range(&(3..7)), 2..4);
        assert_eq!(snap.byte_range(&(2..4)), 3..7);
        assert_eq!(snap.u16_range(&(0..100)), 0..6);
        // A byte offset inside a character counts up to the character's start.
        assert_eq!(snap.u16_at(1), 0);
    }

    fn content(key: u64) -> String {
        let pieces = ["x", "ä", "😀", "\n"];
        (0..=(key % 5) as usize)
            .map(|i| pieces[(key as usize + i) % 4])
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
                edited = Some((a..b, typed.len() as isize - (b - a) as isize));
                text.splice(a..b, typed);
            }
            let diff = state.diff(&new, edited.clone(), text.len());
            text.splice(diff.replace.clone(), shown(&new[diff.prefix..new.len() - diff.suffix]));
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
        state.replace(&diff, new, lens, &(1..2));
        assert_eq!(state.placeholders, [false, false, true, false, true]);
        assert!(state.pending());
        state.forget();
        assert_eq!(
            state.diff(&[1], None, 7),
            BlockDiff {
                prefix: 0,
                suffix: 0,
                replace: 0..7
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
