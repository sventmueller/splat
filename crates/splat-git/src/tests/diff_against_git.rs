//! `diff` against `git diff`: diffs are not unique, so what is compared is how many lines each calls changed, and what is reported
//! has to hold by itself.

use super::support::{Random, Temp, check_hunks, git_is_here, git_output};
use crate::diff::{Hunk, hunks};

/// How many lines `git diff` calls changed, removed and added together.
fn lines_git_changes(temp: &Temp, old: &str, new: &str, extra: &[&str]) -> usize {
    temp.write("old", old.as_bytes());
    temp.write("new", new.as_bytes());
    let mut args = vec!["diff", "--no-index", "--no-color", "--unified=0"];
    args.extend_from_slice(extra);
    args.extend_from_slice(&["old", "new"]);
    let output = git_output(temp.path(), &args);
    let printed = String::from_utf8(output.stdout).unwrap();
    printed
        .lines()
        .filter_map(|line| line.strip_prefix("@@ -"))
        .map(lines_of_hunk_header)
        .sum()
}

/// From `<start>[,<count>] +<start>[,<count>] @@`, the two counts, each one where it is left out.
fn lines_of_hunk_header(header: &str) -> usize {
    let (old, rest) = header.split_once(" +").unwrap();
    let new = rest.split_once(' ').unwrap().0;
    let count = |side: &str| side.split_once(',').map_or(1, |(_, count)| count.parse().unwrap());
    count(old) + count(new)
}

fn lines_changed(found: &[Hunk]) -> usize {
    found
        .iter()
        .map(|hunk| hunk.old_lines.len() + hunk.change.lines.len())
        .sum()
}

#[test]
fn the_header_of_a_hunk_says_how_many_lines_it_holds() {
    assert_eq!(lines_of_hunk_header("3 +3 @@"), 2);
    assert_eq!(lines_of_hunk_header("3,2 +4 @@ context"), 3);
    assert_eq!(lines_of_hunk_header("3,0 +4,5 @@"), 5);
}

#[test]
fn no_more_lines_are_changed_than_git_changes_and_as_few_as_with_its_minimal_search() {
    if !git_is_here() {
        return;
    }
    let temp = Temp::new();
    let mut random = Random::new(0xd1ff);
    for round in 0..150 {
        let alphabet = [2, 5, 40, 1000][round % 4];
        let old = random.lines(200, alphabet);
        let new = random.edited(&old, alphabet);
        let found = hunks(&old, &new);
        check_hunks(&old, &new, &found);
        let ours = lines_changed(&found);
        let git = lines_git_changes(&temp, &old, &new, &[]);
        assert!(ours <= git, "{ours} against {git}: {old:?} -> {new:?}");
        let minimal = lines_git_changes(&temp, &old, &new, &["--minimal"]);
        assert_eq!(ours, minimal, "{old:?} -> {new:?}");
    }
}

/// A text and two rounds of edits on it.
fn much_edited(random: &mut Random, lines: usize, alphabet: usize) -> (String, String) {
    let old = random.lines(lines, alphabet);
    let once = random.edited(&old, alphabet);
    let new = random.edited(&once, alphabet);
    (old, new)
}

#[test]
fn a_text_with_many_changes_under_the_bound_is_no_worse_than_git() {
    if !git_is_here() {
        return;
    }
    let temp = Temp::new();
    let mut random = Random::new(0xbeef);
    for alphabet in [3, 30, 300, 30_000] {
        let (old, new) = much_edited(&mut random, 8_000, alphabet);
        let found = hunks(&old, &new);
        check_hunks(&old, &new, &found);
        let ours = lines_changed(&found);
        let git = lines_git_changes(&temp, &old, &new, &[]);
        assert!(ours <= git, "{ours} against {git} with {alphabet} different lines");
    }
}

/// Past the bound the search stops, where `git` goes on with a heuristic of its own, so there is no limit to how many more lines
/// this calls changed; what it does report still has to hold.
#[test]
fn a_text_with_changes_past_the_bound_still_holds() {
    let mut random = Random::new(0xbeef);
    for alphabet in [3, 30, 300, 30_000] {
        let (old, new) = much_edited(&mut random, 40_000, alphabet);
        check_hunks(&old, &new, &hunks(&old, &new));
    }
}

#[test]
fn texts_that_have_nothing_in_common_change_every_line_as_git_does() {
    if !git_is_here() {
        return;
    }
    let temp = Temp::new();
    let old: String = (0..500).map(|line| format!("old {line}\n")).collect();
    let new: String = (0..700).map(|line| format!("new {line}\n")).collect();
    let found = hunks(&old, &new);
    check_hunks(&old, &new, &found);
    assert_eq!(found.len(), 1);
    assert_eq!(lines_changed(&found), lines_git_changes(&temp, &old, &new, &[]));
}
