//! Which repository a file lies in, by what lies on the disk alone: `$GIT_DIR`, `core.worktree` and the like are ignored, as
//! they would let the surroundings say where to read.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::Repository;
use crate::source::read_small;

/// Far more than the longest path of any file system.
const MAX_POINTER_FILE: u64 = 64 << 10;

/// The canonical form of a path, which may lead to something that is gone: what still exists of it is canonical then.
pub(crate) fn canonical(path: &Path) -> io::Result<PathBuf> {
    match path.canonicalize() {
        Err(error) if error.kind() == io::ErrorKind::NotFound => match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) => Ok(canonical(parent)?.join(name)),
            _ => Err(error),
        },
        other => other,
    }
}

/// As Git does: a `.git` folder that is none is passed over, and a `.git` file that leads nowhere ends the search.
pub(crate) fn find(file: &Path) -> Option<Repository> {
    let start = canonical(file).ok()?;
    for folder in start.ancestors() {
        let dot_git = folder.join(".git");
        let Ok(metadata) = fs::metadata(&dot_git) else {
            continue;
        };
        if metadata.is_file() {
            return follow_pointer(folder, &dot_git);
        }
        if metadata.is_dir()
            && let Some(repository) = repository_at(folder, &dot_git)
        {
            return Some(repository);
        }
    }
    None
}

/// Worktrees and submodules have a file `.git` with the line `gitdir: <path>`, relative to the folder of that file.
fn follow_pointer(work_tree: &Path, pointer: &Path) -> Option<Repository> {
    let text = String::from_utf8(read_small(pointer, MAX_POINTER_FILE).ok()??).ok()?;
    let target = text
        .strip_prefix("gitdir:")?
        .trim_start_matches(' ')
        .trim_end_matches(['\n', '\r']);
    repository_at(work_tree, &work_tree.join(target))
}

/// A git directory has `HEAD` of its own; objects and refs lie in the directory its file `commondir` names, if it has one, which
/// is how a linked worktree shares them with the main one.
fn repository_at(work_tree: &Path, git_dir: &Path) -> Option<Repository> {
    let git_dir = git_dir.canonicalize().ok()?;
    let common_dir = match read_small(&git_dir.join("commondir"), MAX_POINTER_FILE).ok()? {
        Some(text) => {
            let text = String::from_utf8(text).ok()?;
            git_dir.join(text.trim_end_matches(['\n', '\r'])).canonicalize().ok()?
        }
        None => git_dir.clone(),
    };
    let complete =
        git_dir.join("HEAD").is_file() && common_dir.join("objects").is_dir() && common_dir.join("refs").is_dir();
    complete.then(|| Repository {
        work_tree: work_tree.to_path_buf(),
        git_dir,
        common_dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::support::{HandMade, Temp};

    #[test]
    fn a_path_that_is_gone_is_canonical_as_far_as_it_exists() {
        let temp = Temp::new();
        let real = temp.path().canonicalize().unwrap();
        assert_eq!(
            canonical(&temp.path().join("gone/deeper")).unwrap(),
            real.join("gone/deeper")
        );
        assert_eq!(canonical(temp.path()).unwrap(), real);
    }

    #[test]
    fn a_path_of_nothing_is_no_path() {
        assert!(canonical(Path::new("")).is_err());
        assert!(canonical(Path::new("gone-without-any-parent")).is_err());
        assert!(canonical(Path::new("a\0b")).is_err());
    }

    #[test]
    fn a_repository_is_found_from_below() {
        let repo = HandMade::new();
        let deep = repo.work_tree().join("a/b/c.md");
        let found = find(&deep).unwrap();
        assert_eq!(found.work_tree, repo.work_tree().canonicalize().unwrap());
        assert_eq!(found.common_dir, found.git_dir);
    }

    #[test]
    fn no_repository_is_found_where_there_is_none() {
        let temp = Temp::new();
        assert_eq!(find(&temp.path().join("a.md")).map(|found| found.work_tree), None);
        assert_eq!(find(Path::new("")), None);
    }

    #[test]
    fn a_folder_named_git_that_is_no_repository_is_passed_over() {
        let outer = HandMade::new();
        let inner = outer.work_tree().join("inner");
        fs::create_dir_all(inner.join(".git/objects")).unwrap();
        fs::write(inner.join(".git/HEAD"), "x").unwrap();
        let found = find(&inner.join("a.md")).unwrap();
        assert_eq!(found.work_tree, outer.work_tree().canonicalize().unwrap());
    }

    #[test]
    fn a_folder_with_git_refs_but_no_objects_is_no_repository() {
        let temp = Temp::new();
        fs::create_dir_all(temp.path().join(".git/refs")).unwrap();
        fs::write(temp.path().join(".git/HEAD"), "x").unwrap();
        assert_eq!(find(temp.path()).map(|found| found.work_tree), None);
    }

    #[test]
    fn a_pointer_file_leads_to_the_git_directory_it_names() {
        let main = HandMade::new();
        let temp = Temp::new();
        let line = format!("gitdir: {}\r\n", main.git_dir().display());
        fs::write(temp.path().join(".git"), line).unwrap();
        let found = find(&temp.path().join("a.md")).unwrap();
        assert_eq!(found.work_tree, temp.path().canonicalize().unwrap());
        assert_eq!(found.git_dir, main.git_dir().canonicalize().unwrap());
    }

    #[test]
    fn a_pointer_file_that_leads_nowhere_ends_the_search() {
        let outer = HandMade::new();
        let inner = outer.work_tree().join("inner");
        fs::create_dir_all(&inner).unwrap();
        for pointer in ["gitdir: ../gone\n", "nonsense\n", "gitdir: \n"] {
            fs::write(inner.join(".git"), pointer).unwrap();
            assert_eq!(
                find(&inner.join("a.md")).map(|found| found.work_tree),
                None,
                "{pointer}"
            );
        }
        fs::write(inner.join(".git"), [0xff, 0xfe]).unwrap();
        assert_eq!(find(&inner.join("a.md")).map(|found| found.work_tree), None);
    }

    #[test]
    fn the_common_directory_may_be_relative_and_must_hold_objects_and_refs() {
        let main = HandMade::new();
        let linked = main.git_dir().join("worktrees/one");
        fs::create_dir_all(&linked).unwrap();
        fs::write(linked.join("HEAD"), "ref: refs/heads/one\n").unwrap();
        fs::write(linked.join("commondir"), "../..\n").unwrap();
        let temp = Temp::new();
        fs::write(temp.path().join(".git"), format!("gitdir: {}\n", linked.display())).unwrap();
        let found = find(&temp.path().join("a.md")).unwrap();
        assert_eq!(found.common_dir, main.git_dir().canonicalize().unwrap());
        assert_eq!(found.git_dir, linked.canonicalize().unwrap());
        fs::write(linked.join("commondir"), "../gone\n").unwrap();
        assert_eq!(find(&temp.path().join("a.md")).map(|found| found.work_tree), None);
        fs::write(linked.join("commondir"), [0xff]).unwrap();
        assert_eq!(find(&temp.path().join("a.md")).map(|found| found.work_tree), None);
    }

    #[test]
    fn a_git_directory_with_a_commondir_that_cannot_be_read_is_no_repository() {
        let repo = HandMade::new();
        fs::write(repo.git_dir().join("commondir"), vec![b'x'; 1 << 17]).unwrap();
        assert_eq!(find(repo.work_tree()).map(|found| found.work_tree), None);
    }

    #[test]
    fn a_special_file_named_git_does_not_end_the_search() {
        let outer = HandMade::new();
        let inner = outer.work_tree().join("inner");
        fs::create_dir_all(&inner).unwrap();
        let Ok(status) = std::process::Command::new("mkfifo").arg(inner.join(".git")).status() else {
            eprintln!("mkfifo is absent: skipped");
            return;
        };
        assert!(status.success());
        assert_eq!(
            find(&inner.join("a.md")).unwrap().work_tree,
            outer.work_tree().canonicalize().unwrap()
        );
    }
}
