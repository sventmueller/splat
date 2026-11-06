//! Where a text differs from an earlier version of it, by lines.
//!
//! A line ends behind its `\n`, so `a` and `a\n` are different lines, as they are to Git, while a `\r` in front of the `\n` or at
//! the end of the text does not count, so that CRLF against LF is no change. Unchanged lines are always truly unchanged: the
//! search looks for the fewest changed lines (Myers' O(ND) algorithm in its linear-space form) until `WORK_BOUND` is spent, and
//! what it has not sorted out by then counts as changed.

use std::collections::HashMap;
use std::ops::Range;

/// How many steps the search may take in one comparison: diagonals visited, and lines walked along them. At five nanoseconds a
/// step at worst, measured, 10 MB in 200 000 lines stays below about 150 ms together with reading the lines. A search this long
/// sorts out about 6000 changed lines, not counting lines only one text has, which cost it nothing.
const WORK_BOUND: usize = 20_000_000;

/// What happened to a run of lines.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    /// Lines the old text does not have here.
    Added,
    /// Lines that stand where the old text has other lines.
    Modified,
    /// Lines of the old text that are gone from here.
    Removed,
}

/// One place where the texts differ, between lines that are equal in both.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Change {
    /// Whether lines were added, changed or removed.
    pub kind: ChangeKind,
    /// Lines of the new text; empty for `Removed`: the place between lines.
    pub lines: Range<usize>,
    /// The same as bytes of the new text.
    pub bytes: Range<usize>,
}

/// A change with the lines of the old text it replaces, which a caller has no use for and a check of the result does.
#[doc(hidden)]
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hunk {
    /// The lines of the old text this replaces; empty for `Added`.
    pub old_lines: Range<usize>,
    /// What a caller gets to see of it.
    pub change: Change,
}

/// Where `new` differs from `old`, in the order of the text.
pub fn changes(old: &str, new: &str) -> Vec<Change> {
    hunks(old, new).into_iter().map(|hunk| hunk.change).collect()
}

/// Where `new` differs from `old`, with the lines of `old` that each change replaces.
#[doc(hidden)]
pub fn hunks(old: &str, new: &str) -> Vec<Hunk> {
    hunks_within(old, new, WORK_BOUND)
}

/// The same with a search of at most `bound` steps.
#[doc(hidden)]
pub fn hunks_within(old: &str, new: &str, bound: usize) -> Vec<Hunk> {
    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();
    let prefix = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(a, b)| same_line(a, b))
        .count();
    let suffix = old_lines[prefix..]
        .iter()
        .rev()
        .zip(new_lines[prefix..].iter().rev())
        .take_while(|(a, b)| same_line(a, b))
        .count();
    let old_middle = &old_lines[prefix..old_lines.len() - suffix];
    let new_middle = &new_lines[prefix..new_lines.len() - suffix];
    let (old_same, new_same) = unchanged(old_middle, new_middle, bound);

    let mut hunks = Vec::new();
    let mut byte: usize = new_lines[..prefix].iter().map(|line| line.len()).sum();
    let (mut old_at, mut new_at) = (0, 0);
    while old_at < old_same.len() || new_at < new_same.len() {
        if old_same.get(old_at) == Some(&true) && new_same.get(new_at) == Some(&true) {
            byte += new_middle[new_at].len();
            old_at += 1;
            new_at += 1;
            continue;
        }
        let (old_from, new_from, byte_from) = (old_at, new_at, byte);
        while old_same.get(old_at) == Some(&false) {
            old_at += 1;
        }
        while new_same.get(new_at) == Some(&false) {
            byte += new_middle[new_at].len();
            new_at += 1;
        }
        let kind = match (old_at > old_from, new_at > new_from) {
            (true, true) => ChangeKind::Modified,
            (true, false) => ChangeKind::Removed,
            (false, _) => ChangeKind::Added,
        };
        hunks.push(Hunk {
            old_lines: prefix + old_from..prefix + old_at,
            change: Change {
                kind,
                lines: prefix + new_from..prefix + new_at,
                bytes: byte_from..byte,
            },
        });
    }
    hunks
}

/// What is compared of a line: its text without the end of the line, and whether it has an end.
fn line_key(line: &str) -> (&str, bool) {
    let (text, ended) = match line.strip_suffix('\n') {
        Some(text) => (text, true),
        None => (line, false),
    };
    (text.strip_suffix('\r').unwrap_or(text), ended)
}

fn same_line(a: &str, b: &str) -> bool {
    line_key(a) == line_key(b)
}

/// Which lines of each side stay. A line only one side has can never match, and leaving those out of the search is what keeps
/// two texts that have nothing in common cheap.
fn unchanged<'a>(old: &[&'a str], new: &[&'a str], bound: usize) -> (Vec<bool>, Vec<bool>) {
    let mut numbers: HashMap<(&'a str, bool), usize> = HashMap::with_capacity(old.len().max(new.len()));
    let mut number = |lines: &[&'a str]| -> Vec<usize> {
        let number_of = |&line: &&'a str| {
            let next = numbers.len();
            *numbers.entry(line_key(line)).or_insert(next)
        };
        lines.iter().map(number_of).collect()
    };
    let old_numbers = number(old);
    let new_numbers = number(new);
    let mut sides = vec![0u8; numbers.len()];
    old_numbers.iter().for_each(|&line| sides[line] |= 1);
    new_numbers.iter().for_each(|&line| sides[line] |= 2);
    let (old_places, a): (Vec<usize>, Vec<usize>) = shared(&old_numbers, &sides);
    let (new_places, b): (Vec<usize>, Vec<usize>) = shared(&new_numbers, &sides);

    let mut search = Search::new(&a, &b, bound);
    search.run();
    let stay = |places: &[usize], found: &[bool], len: usize| {
        let mut same = vec![false; len];
        for (&place, _) in places.iter().zip(found).filter(|(_, found)| **found) {
            same[place] = true;
        }
        same
    };
    (
        stay(&old_places, &search.a_same, old.len()),
        stay(&new_places, &search.b_same, new.len()),
    )
}

fn shared(numbers: &[usize], sides: &[u8]) -> (Vec<usize>, Vec<usize>) {
    numbers
        .iter()
        .enumerate()
        .filter(|&(_, &line)| sides[line] == 3)
        .map(|(place, &line)| (place, line))
        .unzip()
}

/// The search over two sequences of line numbers.
struct Search<'a> {
    a: &'a [usize],
    b: &'a [usize],
    a_same: Vec<bool>,
    b_same: Vec<bool>,
    forward: Front,
    backward: Front,
    work: usize,
    bound: usize,
}

impl<'a> Search<'a> {
    fn new(a: &'a [usize], b: &'a [usize], bound: usize) -> Search<'a> {
        // Each round visits at least half as many diagonals as there were rounds, in both directions, so the bound cannot pay
        // for more rounds than this, and memory stays small however long the texts are.
        let rounds = ((a.len() + b.len()) / 2 + 1).min(bound.saturating_mul(2).isqrt() + 1);
        Search {
            a,
            b,
            a_same: vec![false; a.len()],
            b_same: vec![false; b.len()],
            forward: Front::new(rounds),
            backward: Front::new(rounds),
            work: 0,
            bound,
        }
    }

    /// Divides the problem at a point of a shortest edit path until one side is empty. Pending parts wait on a stack of their
    /// own rather than the call stack, so that no input can make it overflow; a split that made no progress would run into the
    /// bound.
    fn run(&mut self) {
        let mut pending = vec![(0..self.a.len(), 0..self.b.len())];
        while let Some((mut a, mut b)) = pending.pop() {
            while !a.is_empty() && !b.is_empty() && self.a[a.start] == self.b[b.start] {
                self.a_same[a.start] = true;
                self.b_same[b.start] = true;
                a.start += 1;
                b.start += 1;
                self.work += 1;
            }
            while !a.is_empty() && !b.is_empty() && self.a[a.end - 1] == self.b[b.end - 1] {
                a.end -= 1;
                b.end -= 1;
                self.a_same[a.end] = true;
                self.b_same[b.end] = true;
                self.work += 1;
            }
            if a.is_empty() || b.is_empty() {
                continue;
            }
            if let Some((x, y)) = self.split(a.clone(), b.clone()) {
                pending.push((a.start + x..a.end, b.start + y..b.end));
                pending.push((a.start..a.start + x, b.start..b.start + y));
            }
        }
    }

    /// The middle of a shortest edit path of two sequences that differ at both ends, from a search out of both corners, or `None`
    /// once the bound is spent. The backward search runs the same code over mirrored coordinates.
    fn split(&mut self, a: Range<usize>, b: Range<usize>) -> Option<(usize, usize)> {
        let (a, b) = (&self.a[a], &self.b[b]);
        let (n, m) = (a.len(), b.len());
        let odd = (n + m) % 2 == 1;
        self.forward.start(m);
        self.backward.start(m);
        for _ in 0..((n + m) / 2 + 1).min(self.forward.rounds) {
            let same = |x: usize, y: usize| a[x] == b[y];
            match self
                .forward
                .advance(&self.backward, (n, m), odd, same, &mut self.work, self.bound)
            {
                Step::Met(x, y) => return Some((x, y)),
                Step::Spent => return None,
                Step::Going => {}
            }
            let same = |x: usize, y: usize| a[n - 1 - x] == b[m - 1 - y];
            match self
                .backward
                .advance(&self.forward, (n, m), !odd, same, &mut self.work, self.bound)
            {
                Step::Met(x, y) => return Some((n - x, m - y)),
                Step::Spent => return None,
                Step::Going => {}
            }
        }
        None
    }
}

enum Step {
    Going,
    Met(usize, usize),
    Spent,
}

/// How far one direction has come. Diagonal `k` holds the points with `x + m - y == k`, so that no number is negative and both
/// directions start on diagonal `m`; a point of one direction on `k` lies on diagonal `n + m - k` of the other. `reach` holds one
/// more than the largest `x` of each diagonal, so that 0 can stand for a diagonal not reached.
struct Front {
    reach: Vec<usize>,
    rounds: usize,
    middle: usize,
    low: usize,
    high: usize,
}

impl Front {
    fn new(rounds: usize) -> Front {
        Front {
            reach: vec![0; 2 * rounds + 5],
            rounds,
            middle: 0,
            low: 0,
            high: 0,
        }
    }

    fn slot(&self, diagonal: usize) -> usize {
        diagonal + self.rounds + 2 - self.middle
    }

    fn start(&mut self, m: usize) {
        self.middle = m;
        self.low = m;
        self.high = m;
        let slot = self.slot(m);
        self.reach[slot] = 1;
    }

    /// One more edit: every diagonal of the new parity takes the better of its two neighbours and follows equal lines as far as
    /// they go. The range of diagonals turns back at the corners of the box, and points past its edge are harmless: they have the
    /// largest `x` of their diagonal, meet nothing that is not met earlier inside the box, and are refused here if they do.
    fn advance(
        &mut self,
        other: &Front,
        (n, m): (usize, usize),
        check: bool,
        same: impl Fn(usize, usize) -> bool,
        work: &mut usize,
        bound: usize,
    ) -> Step {
        if self.low > 0 {
            self.low -= 1;
            let slot = self.slot(self.low);
            self.reach[slot - 1] = 0;
        } else {
            self.low += 1;
        }
        if self.high < n + m {
            self.high += 1;
            let slot = self.slot(self.high);
            self.reach[slot + 1] = 0;
        } else {
            self.high -= 1;
        }
        let mut diagonal = self.high;
        loop {
            let slot = self.slot(diagonal);
            let (from_left, from_above) = (self.reach[slot - 1], self.reach[slot + 1]);
            let start = if from_left >= from_above {
                from_left
            } else {
                from_above - 1
            };
            let mut x = start;
            let mut y = x + m - diagonal;
            while x < n && y < m && same(x, y) {
                x += 1;
                y += 1;
            }
            self.reach[slot] = x + 1;
            *work += x - start + 1;
            if check && x <= n && y <= m {
                let opposite = n + m - diagonal;
                if (other.low..=other.high).contains(&opposite) && x + other.reach[other.slot(opposite)] > n {
                    return Step::Met(x, y);
                }
            }
            if *work > bound {
                return Step::Spent;
            }
            if diagonal < self.low + 2 {
                return Step::Going;
            }
            diagonal -= 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::support::{Random, check_hunks, longest_common_lines};

    fn change(kind: ChangeKind, lines: Range<usize>, bytes: Range<usize>) -> Change {
        Change { kind, lines, bytes }
    }

    #[test]
    fn equal_texts_have_no_changes() {
        assert_eq!(changes("", ""), []);
        assert_eq!(changes("a\nb\n", "a\nb\n"), []);
        assert_eq!(changes("a\nb", "a\nb"), []);
    }

    #[test]
    fn a_line_that_only_the_new_text_has_is_added() {
        assert_eq!(changes("a\nc\n", "a\nb\nc\n"), [change(ChangeKind::Added, 1..2, 2..4)]);
        assert_eq!(changes("", "a\n"), [change(ChangeKind::Added, 0..1, 0..2)]);
        assert_eq!(changes("a\n", "a\nb\n"), [change(ChangeKind::Added, 1..2, 2..4)]);
        assert_eq!(changes("b\n", "a\nb\n"), [change(ChangeKind::Added, 0..1, 0..2)]);
    }

    #[test]
    fn a_line_that_only_the_old_text_has_is_removed_at_its_place() {
        assert_eq!(
            changes("a\nb\nc\n", "a\nc\n"),
            [change(ChangeKind::Removed, 1..1, 2..2)]
        );
        assert_eq!(changes("a\nb\n", "b\n"), [change(ChangeKind::Removed, 0..0, 0..0)]);
        assert_eq!(changes("a\nb\n", "a\n"), [change(ChangeKind::Removed, 1..1, 2..2)]);
        assert_eq!(changes("a\n", ""), [change(ChangeKind::Removed, 0..0, 0..0)]);
    }

    #[test]
    fn a_line_that_was_replaced_is_modified() {
        assert_eq!(
            changes("a\nb\nc\n", "a\nx\nc\n"),
            [change(ChangeKind::Modified, 1..2, 2..4)]
        );
        assert_eq!(
            changes("a\nb\nc\n", "a\nx\ny\nc\n"),
            [change(ChangeKind::Modified, 1..3, 2..6)]
        );
    }

    #[test]
    fn changes_come_in_the_order_of_the_text() {
        let found = changes("a\nb\nc\nd\ne\n", "x\na\nc\nd\ny\ne\n");
        assert_eq!(
            found,
            [
                change(ChangeKind::Added, 0..1, 0..2),
                change(ChangeKind::Removed, 2..2, 4..4),
                change(ChangeKind::Added, 4..5, 8..10),
            ]
        );
    }

    #[test]
    fn crlf_against_lf_is_no_change() {
        assert_eq!(changes("a\nb\n", "a\r\nb\r\n"), []);
        assert_eq!(changes("a\r\nb\r\n", "a\nb\n"), []);
        assert_eq!(changes("a\r\nb\nc\r\n", "a\nb\r\nc\n"), []);
        assert_eq!(changes("a\nb", "a\r\nb\r"), []);
    }

    #[test]
    fn a_change_between_crlf_lines_counts_their_bytes() {
        assert_eq!(
            changes("a\nb\nc\n", "a\r\nx\r\nc\r\n"),
            [change(ChangeKind::Modified, 1..2, 3..6)]
        );
    }

    #[test]
    fn only_one_carriage_return_is_ignored() {
        assert_eq!(changes("a\r\r\n", "a\n"), [change(ChangeKind::Modified, 0..1, 0..2)]);
        assert_eq!(changes("a\rb\n", "ab\n"), [change(ChangeKind::Modified, 0..1, 0..3)]);
    }

    #[test]
    fn a_text_without_a_last_newline_differs_from_one_with_it() {
        assert_eq!(changes("a\nb", "a\nb\n"), [change(ChangeKind::Modified, 1..2, 2..4)]);
        assert_eq!(changes("a\nb\n", "a\nb"), [change(ChangeKind::Modified, 1..2, 2..3)]);
        assert_eq!(changes("a\nb", "a\nc"), [change(ChangeKind::Modified, 1..2, 2..3)]);
        assert_eq!(changes("a", "a\nb"), [change(ChangeKind::Modified, 0..2, 0..3)]);
    }

    #[test]
    fn empty_lines_are_lines() {
        assert_eq!(changes("\n\n", "\n\n\n"), [change(ChangeKind::Added, 2..3, 2..3)]);
        assert_eq!(changes("\n", ""), [change(ChangeKind::Removed, 0..0, 0..0)]);
        assert_eq!(changes("", "\n"), [change(ChangeKind::Added, 0..1, 0..1)]);
    }

    #[test]
    fn offsets_lie_on_letters() {
        let new = "äö\nüß\nñ";
        let found = changes("äö\nñ", new);
        assert_eq!(found, [change(ChangeKind::Added, 1..2, 5..10)]);
        assert_eq!(&new[found[0].bytes.clone()], "üß\n");
    }

    #[test]
    fn lines_that_moved_stay_as_far_as_they_can() {
        let hunks = hunks("a\nb\nc\nd\n", "c\nd\na\nb\n");
        check_hunks("a\nb\nc\nd\n", "c\nd\na\nb\n", &hunks);
        assert_eq!(hunks.len(), 2);
    }

    #[test]
    fn past_the_bound_the_middle_counts_as_one_modified_change() {
        let old = "first\nb\nc\nd\ne\nf\ng\nlast\n";
        let new = "first\ng\nf\ne\nd\nc\nb\nlast\n";
        let hunks = hunks_within(old, new, 0);
        check_hunks(old, new, &hunks);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].change, change(ChangeKind::Modified, 1..7, 6..18));
        assert_eq!(hunks[0].old_lines, 1..7);
    }

    #[test]
    fn past_the_bound_a_search_keeps_what_it_has_sorted_out() {
        let lines = |range: Range<u32>| -> String { range.map(|line| format!("{line}\n")).collect() };
        let old = lines(0..200);
        let new: String = lines(100..200) + &lines(0..100);
        for bound in [10, 100, 1000, 10_000, 100_000] {
            check_hunks(&old, &new, &hunks_within(&old, &new, bound));
        }
    }

    /// All texts of up to `length` lines from a few different lines, each against each.
    fn each_pair(length: u32, letters: &[&str], test: impl Fn(&str, &str)) {
        let texts = |length: u32| -> Vec<String> {
            let base = u32::try_from(letters.len()).unwrap();
            (0..=length)
                .flat_map(|size| {
                    (0..base.pow(size)).map(move |mut digits| {
                        (0..size)
                            .map(|_| {
                                let letter = letters[usize::try_from(digits % base).unwrap()];
                                digits /= base;
                                letter
                            })
                            .collect()
                    })
                })
                .collect()
        };
        let all = texts(length);
        for old in &all {
            for new in &all {
                test(old, new);
            }
        }
    }

    fn changed_lines(hunks: &[Hunk]) -> usize {
        hunks
            .iter()
            .map(|hunk| hunk.old_lines.len() + hunk.change.lines.len())
            .sum()
    }

    fn line_count(text: &str) -> usize {
        text.split_inclusive('\n').count()
    }

    #[test]
    fn every_pair_of_short_texts_gets_the_fewest_changed_lines() {
        each_pair(5, &["a\n", "b\n", "c\n"], |old, new| {
            let found = hunks(old, new);
            check_hunks(old, new, &found);
            let least = line_count(old) + line_count(new) - 2 * longest_common_lines(old, new);
            assert_eq!(changed_lines(&found), least, "{old:?} -> {new:?}");
        });
    }

    #[test]
    fn every_pair_of_short_texts_holds_past_any_bound() {
        each_pair(4, &["a\n", "b\n", "c"], |old, new| {
            for bound in [0, 1, 2, 3, 5, 8] {
                check_hunks(old, new, &hunks_within(old, new, bound));
            }
        });
    }

    #[test]
    fn random_texts_get_the_fewest_changed_lines() {
        let mut random = Random::new(0x5eed);
        for round in 0..2000 {
            let alphabet = 2 + round % 40;
            let old = random.lines(120, alphabet);
            let new = random.edited(&old, alphabet);
            let found = hunks(&old, &new);
            check_hunks(&old, &new, &found);
            let least = line_count(&old) + line_count(&new) - 2 * longest_common_lines(&old, &new);
            assert_eq!(changed_lines(&found), least, "{old:?} -> {new:?}");
        }
    }

    #[test]
    fn six_thousand_changed_lines_are_within_the_bound() {
        let lines: Vec<String> = (0..3000).map(|line| format!("{line}\n")).collect();
        let (old, new) = (lines.concat(), lines.iter().rev().cloned().collect::<String>());
        let found = hunks(&old, &new);
        check_hunks(&old, &new, &found);
        assert_eq!(
            changed_lines(&found),
            2 * 3000 - 2,
            "everything but one line, and no more"
        );
    }

    #[test]
    fn lines_only_one_text_has_cost_the_search_nothing() {
        let old: String = (0..100_000).map(|line| format!("old {line}\nsame {line}\n")).collect();
        let new: String = (0..100_000).map(|line| format!("new {line}\nsame {line}\n")).collect();
        let found = hunks_within(&old, &new, 1);
        check_hunks(&old, &new, &found);
        assert_eq!(
            found.len(),
            100_000,
            "a change at each changed line, though the search may take a single step"
        );
    }

    #[test]
    fn a_long_narrow_search_is_not_cut_short_before_its_bound_is_spent() {
        let old = "a\nb\n";
        let new = format!("b\n{}a\n", "b\n".repeat(4000));
        let found = hunks_within(old, &new, 3_000_000);
        check_hunks(old, &new, &found);
        assert_eq!(changed_lines(&found), 2 + 4002 - 2, "one line of each stays");
    }

    #[test]
    fn a_long_narrow_search_holds_past_any_bound() {
        let old = "a\nb\n";
        let new = format!("b\n{}a\n", "b\n".repeat(100));
        for bound in [0, 1, 4, 16, 25, 36, 49, 64, 100, 400, 2000] {
            check_hunks(old, &new, &hunks_within(old, &new, bound));
        }
    }

    #[test]
    fn a_tiny_bound_leaves_room_for_the_rounds_it_pays_for() {
        let front = Search::new(&[1, 2, 3, 4, 5, 6, 7, 8], &[8, 7, 6, 5, 4, 3, 2, 1], 4).forward;
        assert_eq!(front.rounds, 3);
        assert_eq!(front.reach.len(), 2 * 3 + 5);
    }

    #[test]
    fn the_search_for_a_large_problem_reserves_only_what_the_bound_pays_for() {
        let many: Vec<usize> = (0..1_000_000).collect();
        let front = Search::new(&many, &many, WORK_BOUND).forward;
        assert!(front.reach.len() < 20_000, "{}", front.reach.len());
    }
}
