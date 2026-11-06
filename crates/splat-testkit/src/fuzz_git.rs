//! What the fuzz target `git` asks of its input: a comparison of two texts holds whatever they are, and the parsers of a
//! repository's files give an answer on any bytes, within their limits.

use splat_git::diff::{Change, ChangeKind, Hunk, changes, hunks, hunks_within};
use splat_git::raw::{self, Entries, Idx, Kind, Object, RefFile};

/// Far less than a real read may spend, so that one input stays quick; it reaches the same paths.
const BYTES_TO_INFLATE: u64 = 1 << 24;
const MOST_ENTRIES: usize = 16;
const MOST_OBJECTS: u32 = 16;
/// Longer texts are left to the time of the search, not to the table that holds it to the fewest changed lines.
const MOST_LINES_FOR_THE_TABLE: usize = 256;

/// The two parts of an input: two bytes say how long the first one is.
fn parts(data: &[u8]) -> (&[u8], &[u8]) {
    let [high, low, rest @ ..] = data else {
        return (data, &[]);
    };
    rest.split_at((usize::from(*high) << 8 | usize::from(*low)).min(rest.len()))
}

/// Everything at once on one input: as two texts, as a base and a delta, as an index and a pack, and each part as any file.
pub fn check_git(data: &[u8]) {
    let (first, second) = parts(data);
    compare(&String::from_utf8_lossy(first), &String::from_utf8_lossy(second));
    if let Ok(result) = raw::apply_delta(first, second, BYTES_TO_INFLATE) {
        assert!(result.len() as u64 <= BYTES_TO_INFLATE);
    }
    pack(first, second);
    for part in [first, second, data] {
        any_file(part);
    }
}

/// What a comparison promises, on its own and with other line endings and a search cut short.
fn compare(old: &str, new: &str) {
    let found = hunks(old, new);
    hunks_hold(old, new, &found, "a full search");
    let plain: Vec<Change> = found.iter().map(|hunk| hunk.change.clone()).collect();
    assert_eq!(changes(old, new), plain);
    for bound in [0, 1, 16, 256] {
        hunks_hold(old, new, &hunks_within(old, new, bound), "a search cut short");
    }
    let old_lines = old.split_inclusive('\n').count();
    let new_lines = new.split_inclusive('\n').count();
    if old_lines.max(new_lines) <= MOST_LINES_FOR_THE_TABLE {
        let changed: usize = found
            .iter()
            .map(|hunk| hunk.old_lines.len() + hunk.change.lines.len())
            .sum();
        let unchanged = old_lines + new_lines - changed;
        assert_eq!(unchanged, 2 * longest_common_lines(old, new), "{old:?} -> {new:?}");
    }
    if !new.contains('\r') {
        let crlf = new.replace('\n', "\r\n");
        let lines = |changes: &[Hunk]| -> Vec<_> { changes.iter().map(|hunk| hunk.change.lines.clone()).collect() };
        assert_eq!(
            lines(&hunks(old, &crlf)),
            lines(&found),
            "with other line endings: {old:?} -> {new:?}"
        );
    }
}

fn line_key(line: &str) -> (&str, bool) {
    let text = line.strip_suffix('\n').unwrap_or(line);
    (text.strip_suffix('\r').unwrap_or(text), line.ends_with('\n'))
}

/// Hunks in the order of the text that cover whole lines and have nothing but equal lines between them, of the kind their two
/// sides say, with the bytes of their lines: all that a caller can rely on, and what makes everything reported as unchanged
/// really unchanged.
pub fn hunks_hold(old: &str, new: &str, found: &[Hunk], what: &str) {
    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();
    let what = || format!("{what}: {old:?} -> {new:?}: {found:?}");
    let same = |old_from: usize, new_from: usize, count: usize| {
        (0..count).for_each(|at| {
            assert_eq!(
                line_key(old_lines[old_from + at]),
                line_key(new_lines[new_from + at]),
                "{}",
                what()
            )
        });
    };
    let (mut old_at, mut new_at, mut byte) = (0, 0, 0);
    for (number, hunk) in found.iter().enumerate() {
        let change = &hunk.change;
        assert!(
            old_at <= hunk.old_lines.start && hunk.old_lines.start <= hunk.old_lines.end,
            "{}",
            what()
        );
        assert!(
            new_at <= change.lines.start && change.lines.start <= change.lines.end,
            "{}",
            what()
        );
        assert!(
            hunk.old_lines.end <= old_lines.len() && change.lines.end <= new_lines.len(),
            "{}",
            what()
        );
        let between = hunk.old_lines.start - old_at;
        assert_eq!(between, change.lines.start - new_at, "{}", what());
        assert!(number == 0 || between > 0, "{}", what());
        same(old_at, new_at, between);
        byte += new_lines[new_at..change.lines.start]
            .iter()
            .map(|line| line.len())
            .sum::<usize>();
        let size: usize = new_lines[change.lines.clone()].iter().map(|line| line.len()).sum();
        assert_eq!(change.bytes, byte..byte + size, "{}", what());
        assert!(
            new.is_char_boundary(change.bytes.start) && new.is_char_boundary(change.bytes.end),
            "{}",
            what()
        );
        let kind = match (hunk.old_lines.is_empty(), change.lines.is_empty()) {
            (false, false) => ChangeKind::Modified,
            (false, true) => ChangeKind::Removed,
            (true, false) => ChangeKind::Added,
            (true, true) => panic!("a change of no lines, {}", what()),
        };
        assert_eq!(change.kind, kind, "{}", what());
        byte += size;
        old_at = hunk.old_lines.end;
        new_at = change.lines.end;
    }
    assert_eq!(old_lines.len() - old_at, new_lines.len() - new_at, "{}", what());
    same(old_at, new_at, old_lines.len() - old_at);
}

/// The length of a longest common subsequence of lines, by the textbook table.
fn longest_common_lines(old: &str, new: &str) -> usize {
    let a: Vec<(&str, bool)> = old.split_inclusive('\n').map(line_key).collect();
    let b: Vec<(&str, bool)> = new.split_inclusive('\n').map(line_key).collect();
    let mut row = vec![0; b.len() + 1];
    for line in &a {
        let mut diagonal = 0;
        for (at, other) in b.iter().enumerate() {
            let next = if line == other {
                diagonal + 1
            } else {
                row[at].max(row[at + 1])
            };
            diagonal = row[at + 1];
            row[at + 1] = next;
        }
    }
    row[b.len()]
}

fn within_its_limit(object: &Object) {
    assert!(
        object.data.len() as u64 <= object.kind.limit(),
        "{:?} of {}",
        object.kind,
        object.data.len()
    );
    assert!(matches!(
        object.kind,
        Kind::Commit | Kind::Tree | Kind::Blob | Kind::Tag
    ));
}

/// Whatever objects a pack gives: `read_packed` has already held them to their index, and the search through the pack may not panic.
fn pack(idx: &[u8], pack: &[u8]) {
    if let Ok(objects) = raw::read_packed(idx.to_vec(), pack.to_vec(), MOST_OBJECTS, BYTES_TO_INFLATE) {
        objects.iter().flatten().for_each(within_its_limit);
    }
}

/// The same bytes as each kind of file a repository holds.
fn any_file(bytes: &[u8]) {
    let _ = raw::check_format(bytes);
    let _ = raw::packed_ref(bytes, b"refs/heads/main");
    let _ = raw::commit_tree(bytes);
    let _ = raw::tag_target(bytes);
    if let Ok(RefFile::Symbolic(name)) = raw::parse_ref_file(bytes) {
        assert!(
            raw::valid_ref_name(name) && name.starts_with("refs/") && !name.contains(".."),
            "{name}"
        );
    }
    if let Ok(object) = raw::read_loose(&bytes.to_vec()) {
        within_its_limit(&object);
    }
    tree(bytes);
    if let Ok(idx) = Idx::parse(Box::new(bytes.to_vec())) {
        for index in 0..idx.objects().min(MOST_OBJECTS) {
            if let Ok(name) = idx.name_at(index) {
                let _ = idx.find(&name);
            }
        }
    }
}

fn tree(bytes: &[u8]) {
    let mut whole = true;
    let mut seen = Vec::new();
    for entry in Entries::new(bytes) {
        match entry {
            Ok(entry) => {
                assert!(!entry.name.is_empty() && !entry.name.contains(&b'/'), "{entry:?}");
                if seen.len() < MOST_ENTRIES {
                    seen.push(entry);
                }
            }
            Err(_) => whole = false,
        }
    }
    for entry in seen {
        match raw::lookup(bytes, entry.name) {
            Ok(found) => {
                assert!(whole, "a tree with damage in it was believed");
                assert_eq!(found.map(|found| found.name), Some(entry.name));
            }
            Err(_) => assert!(!whole, "a whole tree was called damaged"),
        }
    }
}
