//! The entries of a tree object: `<octal mode> <name>\0` and twenty bytes of object name, one after the other.

use unicode_normalization::UnicodeNormalization;

use crate::ReadError;
use crate::object::Oid;

/// No file system has names this long, however a name is normalized.
const LONGEST_NAME_TO_NORMALIZE: usize = 4096;

/// One entry of a tree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry<'a> {
    /// The file type and permissions.
    pub(crate) mode: u32,
    /// The name, one path component.
    pub name: &'a [u8],
    /// The object it stands for.
    pub(crate) oid: Oid,
}

impl Entry<'_> {
    /// Whether it is a folder.
    pub(crate) fn is_tree(&self) -> bool {
        self.mode & 0o170_000 == 0o040_000
    }

    /// Whether it is a submodule, whose commit lies in another repository.
    pub(crate) fn is_gitlink(&self) -> bool {
        self.mode & 0o170_000 == 0o160_000
    }
}

/// The entries of a tree, in the order they are stored; the first that makes no sense ends them with `Damaged`.
pub struct Entries<'a> {
    rest: &'a [u8],
}

impl<'a> Entries<'a> {
    /// The entries of the tree in `tree`.
    pub fn new(tree: &'a [u8]) -> Entries<'a> {
        Entries { rest: tree }
    }

    fn parse(&mut self) -> Option<Entry<'a>> {
        let space = self.rest.iter().position(|&byte| byte == b' ')?;
        let (digits, after_space) = self.rest.split_at(space);
        if digits.is_empty() || digits.len() > 7 {
            return None;
        }
        let mut mode = 0;
        for &digit in digits {
            if !(b'0'..=b'7').contains(&digit) {
                return None;
            }
            mode = mode << 3 | u32::from(digit - b'0');
        }
        let after_space = &after_space[1..];
        let end = after_space.iter().position(|&byte| byte == 0)?;
        let (name, after_name) = after_space.split_at(end);
        if name.is_empty() || name.contains(&b'/') {
            return None;
        }
        let (oid, rest) = after_name[1..].split_first_chunk::<20>()?;
        self.rest = rest;
        Some(Entry {
            mode,
            name,
            oid: Oid(*oid),
        })
    }
}

impl<'a> Iterator for Entries<'a> {
    type Item = Result<Entry<'a>, ReadError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let entry = self.parse();
        if entry.is_none() {
            self.rest = &[];
        }
        Some(entry.ok_or(ReadError::Damaged))
    }
}

/// The name as a case-insensitive file system with Unicode folding treats it, as far as ASCII letters and what decomposes
/// into them go; other scripts keep their case.
fn folded(name: &str) -> impl Iterator<Item = char> + '_ {
    name.nfd().map(|letter| letter.to_ascii_lowercase())
}

fn same_name(stored: &[u8], asked: &[u8]) -> bool {
    if stored.eq_ignore_ascii_case(asked) {
        return true;
    }
    if (stored.is_ascii() && asked.is_ascii()) || stored.len() > LONGEST_NAME_TO_NORMALIZE {
        return false;
    }
    match (std::str::from_utf8(stored), std::str::from_utf8(asked)) {
        (Ok(stored), Ok(asked)) => folded(stored).eq(folded(asked)),
        _ => false,
    }
}

/// The entry for one path component: its exact name first, then, in a second pass, a name a file system on macOS would take
/// for the same, since it may hand out another case or another Unicode form than the commit has. On a case-sensitive volume
/// that second pass can find a different file of a similar name. The whole tree is read either way, so that a damaged tree
/// is damaged wherever the entry lies.
pub fn lookup<'a>(tree: &'a [u8], name: &[u8]) -> Result<Option<Entry<'a>>, ReadError> {
    let mut exact = None;
    for entry in Entries::new(tree) {
        let entry = entry?;
        if exact.is_none() && entry.name == name {
            exact = Some(entry);
        }
    }
    if exact.is_some() {
        return Ok(exact);
    }
    Ok(Entries::new(tree).flatten().find(|entry| same_name(entry.name, name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::support::tree_bytes;

    #[test]
    fn entries_read_in_order_with_mode_and_object() {
        let tree = tree_bytes(&[
            ("100644", "a.md", Oid([1; 20])),
            ("40000", "docs", Oid([2; 20])),
            ("160000", "sub", Oid([3; 20])),
        ]);
        let entries: Vec<Entry> = Entries::new(&tree).map(Result::unwrap).collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            (entries[0].mode, entries[0].name, entries[0].oid),
            (0o100_644, &b"a.md"[..], Oid([1; 20]))
        );
        assert!(entries[1].is_tree() && !entries[1].is_gitlink());
        assert!(entries[2].is_gitlink() && !entries[2].is_tree());
        assert!(!entries[0].is_tree() && !entries[0].is_gitlink());
    }

    #[test]
    fn an_empty_tree_has_no_entries() {
        assert_eq!(Entries::new(b"").count(), 0);
        assert_eq!(lookup(b"", b"a"), Ok(None));
    }

    #[test]
    fn a_name_with_a_slash_is_damaged() {
        let tree = tree_bytes(&[("100644", "../../secret", Oid([1; 20]))]);
        assert_eq!(lookup(&tree, b"secret"), Err(ReadError::Damaged));
    }

    #[test]
    fn damage_counts_wherever_it_lies() {
        let mut tree = tree_bytes(&[("100644", "a.md", Oid([1; 20]))]);
        tree.extend_from_slice(b"100644 b/c\0");
        tree.extend_from_slice(&[0; 20]);
        assert_eq!(lookup(&tree, b"a.md"), Err(ReadError::Damaged));
    }

    #[test]
    fn entries_that_make_no_sense_are_damaged_and_end_the_entries() {
        let oid = [b'x'; 20];
        let oid = std::str::from_utf8(&oid).unwrap();
        for tree in [
            "100644".to_string(),
            format!(" a\0{oid}"),
            format!("10064x a\0{oid}"),
            format!("12345678 a\0{oid}"),
            format!("100644 \0{oid}"),
            format!("100644 a{oid}"),
            format!("100644 a\0{}", &oid[..19]),
        ] {
            let mut entries = Entries::new(tree.as_bytes());
            assert_eq!(entries.next(), Some(Err(ReadError::Damaged)), "{tree}");
            assert_eq!(entries.next(), None, "{tree}");
        }
    }

    #[test]
    fn the_exact_name_wins_over_a_similar_one_before_it() {
        let tree = tree_bytes(&[
            ("100644", "README.md", Oid([1; 20])),
            ("100644", "readme.md", Oid([2; 20])),
        ]);
        assert_eq!(lookup(&tree, b"readme.md").unwrap().unwrap().oid, Oid([2; 20]));
        assert_eq!(lookup(&tree, b"README.md").unwrap().unwrap().oid, Oid([1; 20]));
    }

    #[test]
    fn another_case_of_ascii_letters_is_the_same_name_in_the_second_pass() {
        let tree = tree_bytes(&[("100644", "Docs.MD", Oid([1; 20]))]);
        assert_eq!(lookup(&tree, b"docs.md").unwrap().unwrap().oid, Oid([1; 20]));
        assert_eq!(lookup(&tree, b"other.md"), Ok(None));
    }

    #[test]
    fn composed_and_decomposed_letters_are_the_same_name_in_the_second_pass() {
        let tree = tree_bytes(&[("100644", "Bücher.md", Oid([1; 20]))]);
        assert_eq!(
            lookup(&tree, "Bu\u{308}cher.md".as_bytes()).unwrap().unwrap().oid,
            Oid([1; 20])
        );
        let tree = tree_bytes(&[("100644", "Bu\u{308}cher.md", Oid([2; 20]))]);
        assert_eq!(
            lookup(&tree, "Bücher.md".as_bytes()).unwrap().unwrap().oid,
            Oid([2; 20])
        );
    }

    #[test]
    fn case_and_form_together_are_the_same_name_too() {
        let tree = tree_bytes(&[("100644", "ÄRGER.MD", Oid([1; 20]))]);
        assert_eq!(
            lookup(&tree, "a\u{308}rger.md".as_bytes()).unwrap().unwrap().oid,
            Oid([1; 20])
        );
    }

    #[test]
    fn names_that_are_no_text_compare_only_by_their_bytes() {
        let mut tree = b"100644 caf\xe9\0".to_vec();
        tree.extend_from_slice(&[1; 20]);
        assert_eq!(lookup(&tree, b"caf\xe9").unwrap().unwrap().oid, Oid([1; 20]));
        assert_eq!(lookup(&tree, "café".as_bytes()), Ok(None));
    }

    #[test]
    fn a_stored_name_longer_than_any_file_system_allows_is_not_normalized() {
        let long = format!("ä{}", "x".repeat(LONGEST_NAME_TO_NORMALIZE));
        let tree = tree_bytes(&[("100644", long.as_str(), Oid([1; 20]))]);
        assert_eq!(
            lookup(
                &tree,
                format!("a\u{308}{}", "x".repeat(LONGEST_NAME_TO_NORMALIZE)).as_bytes()
            ),
            Ok(None)
        );
    }
}
