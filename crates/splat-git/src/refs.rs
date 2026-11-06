//! What `HEAD` stands on: symbolic refs, loose refs and `packed-refs`.

use std::path::Path;

use crate::ReadError;
use crate::object::{Oid, parse_hex};
use crate::source::read_small;

/// As many files as Git reads for one ref, the first being `HEAD`.
const MAX_SYMBOLIC_DEPTH: usize = 5;
const MAX_REF_FILE: u64 = 4096;
const MAX_REF_NAME: usize = 1024;
const MAX_PACKED_REFS: u64 = 256 << 20;

/// What a ref file says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefFile<'a> {
    /// The object it stands on.
    Direct(Oid),
    /// The ref it follows, a name that cannot leave the git directory.
    Symbolic(&'a str),
}

/// The contents of a ref file; Git allows spaces behind `ref:` and anything white behind the name and the object.
pub fn parse_ref_file(content: &[u8]) -> Result<RefFile<'_>, ReadError> {
    if let Some(rest) = content.strip_prefix(b"ref:") {
        let name = std::str::from_utf8(rest.trim_ascii()).map_err(|_| ReadError::Damaged)?;
        return if valid_ref_name(name) {
            Ok(RefFile::Symbolic(name))
        } else {
            Err(ReadError::Damaged)
        };
    }
    let (name, rest) = content.split_at_checked(40).ok_or(ReadError::Damaged)?;
    if rest.first().is_some_and(|byte| !byte.is_ascii_whitespace()) {
        return Err(ReadError::Damaged);
    }
    parse_hex(name).map(RefFile::Direct).ok_or(ReadError::Damaged)
}

/// Git's rules for the name of a ref, which among other things keep it inside `refs/`.
pub fn valid_ref_name(name: &str) -> bool {
    name.len() <= MAX_REF_NAME
        && name.starts_with("refs/")
        && !name.contains("..")
        && !name.contains("@{")
        && !name.ends_with('.')
        && !name.bytes().any(|byte| byte < 0x20 || b" ~^:?*[\\\x7f".contains(&byte))
        && name
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

/// The object a ref stands on according to `packed-refs`; the `^` line behind a tag names the commit that tag stands on.
pub fn packed_ref(file: &[u8], name: &[u8]) -> Result<Option<Oid>, ReadError> {
    let mut lines = file.split(|&byte| byte == b'\n').peekable();
    while let Some(line) = lines.next() {
        if line.is_empty() || line[0] == b'#' || line[0] == b'^' {
            continue;
        }
        let (hex, rest) = line.split_at_checked(40).ok_or(ReadError::Damaged)?;
        let oid = parse_hex(hex).ok_or(ReadError::Damaged)?;
        let stored = rest.strip_prefix(b" ").ok_or(ReadError::Damaged)?;
        if stored == name {
            return match lines.peek().and_then(|next| next.strip_prefix(b"^")) {
                Some(peeled) => parse_hex(peeled).map(Some).ok_or(ReadError::Damaged),
                None => Ok(Some(oid)),
            };
        }
    }
    Ok(None)
}

/// Refs that each linked work tree has of its own, in its own directory instead of the shared one.
fn belongs_to_work_tree(name: &str) -> bool {
    ["refs/bisect/", "refs/worktree/", "refs/rewritten/"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// The object `HEAD` stands on, or `None` on a branch without a commit.
pub(crate) fn head(git_dir: &Path, common_dir: &Path) -> Result<Option<Oid>, ReadError> {
    let mut content = read_small(&git_dir.join("HEAD"), MAX_REF_FILE)?.ok_or(ReadError::Damaged)?;
    for _ in 0..MAX_SYMBOLIC_DEPTH {
        let name = match parse_ref_file(&content)? {
            RefFile::Direct(oid) => return Ok(Some(oid)),
            RefFile::Symbolic(name) => name.to_string(),
        };
        let folder = if belongs_to_work_tree(&name) {
            git_dir
        } else {
            common_dir
        };
        match read_small(&folder.join(&name), MAX_REF_FILE)? {
            Some(next) => content = next,
            None => {
                return match read_small(&common_dir.join("packed-refs"), MAX_PACKED_REFS)? {
                    Some(packed) => packed_ref(&packed, name.as_bytes()),
                    None => Ok(None),
                };
            }
        }
    }
    Err(ReadError::Damaged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::support::{HandMade, sparse_file};

    const ONE: &str = "1111111111111111111111111111111111111111";
    const TWO: &str = "2222222222222222222222222222222222222222";

    fn oid(text: &str) -> Oid {
        parse_hex(text.as_bytes()).unwrap()
    }

    #[test]
    fn a_ref_file_names_an_object_or_another_ref() {
        assert_eq!(parse_ref_file(ONE.as_bytes()), Ok(RefFile::Direct(oid(ONE))));
        assert_eq!(
            parse_ref_file(format!("{ONE}\n").as_bytes()),
            Ok(RefFile::Direct(oid(ONE)))
        );
        assert_eq!(
            parse_ref_file(format!("{ONE} junk").as_bytes()),
            Ok(RefFile::Direct(oid(ONE)))
        );
        assert_eq!(
            parse_ref_file(b"ref: refs/heads/main\n"),
            Ok(RefFile::Symbolic("refs/heads/main"))
        );
        assert_eq!(
            parse_ref_file(b"ref:refs/heads/main"),
            Ok(RefFile::Symbolic("refs/heads/main"))
        );
        assert_eq!(
            parse_ref_file(b"ref: \t refs/heads/main \r\n"),
            Ok(RefFile::Symbolic("refs/heads/main"))
        );
    }

    #[test]
    fn a_ref_file_with_anything_else_is_damaged() {
        let sha256 = "1".repeat(64);
        for content in [
            "",
            "short",
            sha256.as_str(),
            "ref:",
            "ref: HEAD",
            "not an object name at all, but forty long",
        ] {
            assert_eq!(parse_ref_file(content.as_bytes()), Err(ReadError::Damaged), "{content}");
        }
        assert_eq!(parse_ref_file(b"ref: refs/heads/\xff"), Err(ReadError::Damaged));
    }

    #[test]
    fn no_ref_name_can_leave_the_git_directory() {
        for name in [
            "../../etc/passwd",
            "/etc/passwd",
            "refs/../../secret",
            "refs/heads/../../../secret",
            "refs/heads/..",
            "refs/heads/.",
            "HEAD",
            "packed-refs",
            "objects/ab/cdef",
        ] {
            assert!(!valid_ref_name(name), "{name}");
        }
    }

    #[test]
    fn the_names_git_forbids_are_refused() {
        for name in [
            "refs/",
            "refs//x",
            "refs/x/",
            "refs/x.",
            "refs/.x",
            "refs/x.lock",
            "refs/x.lock/y",
            "refs/x@{y",
            "refs/x y",
            "refs/x~y",
            "refs/x^y",
            "refs/x:y",
            "refs/x?y",
            "refs/x*y",
            "refs/x[y",
            "refs/x\\y",
            "refs/x\ty",
            "refs/x\u{7f}y",
        ] {
            assert!(!valid_ref_name(name), "{name}");
        }
        assert!(!valid_ref_name(&format!("refs/{}", "x".repeat(MAX_REF_NAME))));
    }

    #[test]
    fn the_names_git_allows_are_taken() {
        for name in [
            "refs/heads/main",
            "refs/heads/a/b.c",
            "refs/tags/v1.0",
            "refs/heads/Bücher",
            "refs/heads/@",
        ] {
            assert!(valid_ref_name(name), "{name}");
        }
    }

    #[test]
    fn packed_refs_are_found_by_their_whole_name() {
        let file =
            format!("# pack-refs with: peeled fully-peeled sorted \n{ONE} refs/heads/main\n{TWO} refs/heads/main2\n");
        assert_eq!(packed_ref(file.as_bytes(), b"refs/heads/main"), Ok(Some(oid(ONE))));
        assert_eq!(packed_ref(file.as_bytes(), b"refs/heads/main2"), Ok(Some(oid(TWO))));
        assert_eq!(packed_ref(file.as_bytes(), b"refs/heads/mai"), Ok(None));
    }

    #[test]
    fn a_peeled_line_says_what_a_tag_stands_on() {
        let file = format!("{ONE} refs/tags/a\n^{TWO}\n{TWO} refs/tags/b\n");
        assert_eq!(packed_ref(file.as_bytes(), b"refs/tags/a"), Ok(Some(oid(TWO))));
        assert_eq!(packed_ref(file.as_bytes(), b"refs/tags/b"), Ok(Some(oid(TWO))));
        let file = format!("{ONE} refs/tags/a\n^short\n");
        assert_eq!(packed_ref(file.as_bytes(), b"refs/tags/a"), Err(ReadError::Damaged));
    }

    #[test]
    fn lines_of_packed_refs_that_make_no_sense_are_damaged() {
        for line in [
            "short".to_string(),
            format!("{}x refs/x", "g".repeat(39)),
            format!("{ONE}xrefs/x"),
        ] {
            assert_eq!(
                packed_ref(line.as_bytes(), b"refs/x"),
                Err(ReadError::Damaged),
                "{line}"
            );
        }
    }

    #[test]
    fn head_follows_a_branch_to_its_commit() {
        let repo = HandMade::new();
        repo.write("HEAD", b"ref: refs/heads/main\n");
        repo.write("refs/heads/main", format!("{ONE}\n").as_bytes());
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Ok(Some(oid(ONE))));
    }

    #[test]
    fn head_may_stand_directly_on_a_commit() {
        let repo = HandMade::new();
        repo.write("HEAD", format!("{TWO}\n").as_bytes());
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Ok(Some(oid(TWO))));
    }

    #[test]
    fn a_branch_without_a_commit_stands_on_nothing() {
        let repo = HandMade::new();
        repo.write("HEAD", b"ref: refs/heads/none\n");
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Ok(None));
        repo.write("packed-refs", format!("{ONE} refs/heads/other\n").as_bytes());
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Ok(None));
    }

    #[test]
    fn a_loose_ref_wins_over_a_packed_one() {
        let repo = HandMade::new();
        repo.write("HEAD", b"ref: refs/heads/main\n");
        repo.write("packed-refs", format!("{ONE} refs/heads/main\n").as_bytes());
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Ok(Some(oid(ONE))));
        repo.write("refs/heads/main", TWO.as_bytes());
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Ok(Some(oid(TWO))));
    }

    #[test]
    fn a_folder_of_refs_with_the_name_of_a_packed_ref_does_not_hide_it() {
        let repo = HandMade::new();
        repo.write("HEAD", b"ref: refs/heads/a\n");
        repo.write("refs/heads/a/b", TWO.as_bytes());
        repo.write("packed-refs", format!("{ONE} refs/heads/a\n").as_bytes());
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Ok(Some(oid(ONE))));
    }

    #[test]
    fn a_ref_below_a_file_is_no_ref() {
        let repo = HandMade::new();
        repo.write("HEAD", b"ref: refs/heads/a/b\n");
        repo.write("refs/heads/a", TWO.as_bytes());
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Ok(None));
    }

    #[test]
    fn a_chain_of_symbolic_refs_reads_up_to_the_cap() {
        let repo = HandMade::new();
        repo.write("HEAD", b"ref: refs/heads/a\n");
        repo.write("refs/heads/a", b"ref: refs/heads/b\n");
        repo.write("refs/heads/b", b"ref: refs/heads/c\n");
        repo.write("refs/heads/c", b"ref: refs/heads/d\n");
        repo.write("refs/heads/d", ONE.as_bytes());
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Ok(Some(oid(ONE))));
        repo.write("refs/heads/d", b"ref: refs/heads/e\n");
        repo.write("refs/heads/e", ONE.as_bytes());
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Err(ReadError::Damaged));
    }

    #[test]
    fn a_ref_loop_is_damaged() {
        let repo = HandMade::new();
        repo.write("HEAD", b"ref: refs/heads/a\n");
        repo.write("refs/heads/a", b"ref: refs/heads/b\n");
        repo.write("refs/heads/b", b"ref: refs/heads/a\n");
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Err(ReadError::Damaged));
    }

    #[test]
    fn head_pointing_outside_is_damaged_and_reads_nothing_there() {
        let repo = HandMade::new();
        repo.write("HEAD", b"ref: ../../outside\n");
        repo.write_beside_git_dir("outside", ONE.as_bytes());
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Err(ReadError::Damaged));
        repo.write("HEAD", b"ref: refs/../../../outside\n");
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Err(ReadError::Damaged));
    }

    #[test]
    fn a_missing_or_oversized_head_cannot_be_believed() {
        let repo = HandMade::new();
        assert_eq!(
            head(&repo.git_dir().join("elsewhere"), repo.git_dir()),
            Err(ReadError::Damaged)
        );
        sparse_file(&repo.git_dir().join("HEAD"), MAX_REF_FILE + 1);
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Err(ReadError::TooLarge));
    }

    #[test]
    fn a_packed_refs_file_above_the_limit_is_too_large() {
        let repo = HandMade::new();
        repo.write("HEAD", b"ref: refs/heads/main\n");
        sparse_file(&repo.git_dir().join("packed-refs"), MAX_PACKED_REFS + 1);
        assert_eq!(head(repo.git_dir(), repo.git_dir()), Err(ReadError::TooLarge));
    }

    #[test]
    fn refs_of_a_work_tree_lie_in_its_own_directory_and_the_others_in_the_shared_one() {
        let shared = HandMade::new();
        let own = HandMade::new();
        shared.write("refs/worktree/x", ONE.as_bytes());
        own.write("refs/worktree/x", TWO.as_bytes());
        shared.write("refs/heads/x", ONE.as_bytes());
        own.write("refs/heads/x", TWO.as_bytes());
        own.write("HEAD", b"ref: refs/worktree/x\n");
        assert_eq!(
            head(own.git_dir(), shared.git_dir())
                .map(|oid| oid.unwrap().to_string())
                .as_deref(),
            Ok(TWO)
        );
        own.write("HEAD", b"ref: refs/heads/x\n");
        assert_eq!(
            head(own.git_dir(), shared.git_dir())
                .map(|oid| oid.unwrap().to_string())
                .as_deref(),
            Ok(ONE)
        );
    }
}
