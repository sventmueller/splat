//! Repositories the real `git` made, in every form it leaves them, against what it says they hold.

use std::fs;
use std::path::Path;

use super::support::{Real, Temp, git, git_is_here};
use crate::object::parse_hex;
use crate::pack::{Idx, Pack, Stored};
use crate::source::OnDisk;
use crate::{ReadError, Repository, diff, find};

fn commit_git_says(folder: &Path) -> String {
    String::from_utf8(git(folder, &["rev-parse", "HEAD^{commit}"]))
        .unwrap()
        .trim()
        .to_string()
}

fn head_commit_of(repository: &Repository) -> String {
    repository.head_commit().unwrap().unwrap().to_string()
}

/// That `file`, below `folder`, holds what `git` in that folder says, and that the commit is the one it says.
fn same_as_git_in(folder: &Path, file: &str) {
    let path = folder.join(file);
    let repository = find(&path).unwrap();
    let expected = git(folder, &["cat-file", "blob", &format!("HEAD:{file}")]);
    assert_eq!(
        repository.head_version(&path).unwrap().as_deref(),
        Some(&expected[..]),
        "{file}"
    );
    assert_eq!(head_commit_of(&repository), commit_git_says(folder));
}

fn same_as_git(repo: &Real, file: &str) {
    same_as_git_in(repo.root(), file);
}

fn in_the_last_commit(path: &Path) -> Result<Option<Vec<u8>>, ReadError> {
    find(path).unwrap().head_version(path)
}

/// A file long enough for deltas, changed a little in each of `rounds` commits.
fn long_history(repo: &Real, rounds: usize) {
    let mut lines: Vec<String> = (0..1500)
        .map(|line| format!("line {line} of a file that goes on for a while\n"))
        .collect();
    for round in 0..rounds {
        lines[round * 37 % 1500] = format!("changed in round {round}\n");
        repo.write("long.md", lines.concat().as_bytes());
        repo.commit(&format!("round {round}"));
    }
}

/// What the packs of a repository store, entry by entry.
fn stored_in_packs(root: &Path) -> Vec<Stored> {
    let mut stored = Vec::new();
    for entry in fs::read_dir(root.join(".git/objects/pack")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "idx") {
            let idx = Idx::parse(Box::new(OnDisk::open(&path).unwrap().unwrap())).unwrap();
            let file = OnDisk::open(&path.with_extension("pack")).unwrap().unwrap();
            let pack = Pack::parse(Box::new(file), &idx).unwrap();
            for index in 0..idx.objects() {
                let offset = idx.find(&idx.name_at(index).unwrap()).unwrap().unwrap();
                stored.push(pack.entry(offset).unwrap().stored);
            }
        }
    }
    stored
}

/// Each commit of the history in turn, by moving `HEAD` onto it, so that every depth of the delta chains gets read.
fn every_commit_same_as_git(repo: &Real, file: &str) {
    let commits = String::from_utf8(repo.git(&["rev-list", "HEAD"])).unwrap();
    for commit in commits.lines() {
        repo.git(&["update-ref", "--no-deref", "HEAD", commit]);
        same_as_git(repo, file);
    }
}

#[test]
fn loose_objects_read_as_git_says() {
    let Some(repo) = Real::new() else { return };
    repo.write("a.md", b"first\n");
    repo.commit("one");
    repo.write("a.md", b"second\n");
    repo.commit("two");
    assert!(stored_in_packs(repo.root()).is_empty());
    same_as_git(&repo, "a.md");
}

#[test]
fn a_pack_with_offset_deltas_reads_as_git_says() {
    let Some(repo) = Real::new() else { return };
    long_history(&repo, 40);
    repo.git(&["gc", "-q"]);
    let stored = stored_in_packs(repo.root());
    assert!(
        stored.iter().any(|stored| matches!(stored, Stored::DeltaOnOffset(_))),
        "{stored:?}"
    );
    assert!(!repo.root().join(".git/refs/heads/main").exists(), "gc packs the refs");
    every_commit_same_as_git(&repo, "long.md");
}

#[test]
fn a_pack_with_deltas_on_names_reads_as_git_says() {
    let Some(repo) = Real::new() else { return };
    long_history(&repo, 25);
    repo.git(&[
        "-c",
        "repack.useDeltaBaseOffset=false",
        "repack",
        "-adfq",
        "--depth=20",
        "--window=20",
    ]);
    let stored = stored_in_packs(repo.root());
    assert!(
        stored.iter().any(|stored| matches!(stored, Stored::DeltaOnName(_))),
        "{stored:?}"
    );
    every_commit_same_as_git(&repo, "long.md");
}

#[test]
fn further_commits_on_top_of_a_pack_read_as_git_says() {
    let Some(repo) = Real::new() else { return };
    long_history(&repo, 10);
    repo.git(&["gc", "-q"]);
    repo.write("long.md", b"rewritten after the pack\n");
    repo.write("new.md", b"only loose\n");
    repo.commit("after");
    assert!(!stored_in_packs(repo.root()).is_empty());
    same_as_git(&repo, "long.md");
    same_as_git(&repo, "new.md");
}

#[test]
fn several_packs_read_as_git_says() {
    let Some(repo) = Real::new() else { return };
    for round in 0..4 {
        repo.write(&format!("file{round}.md"), format!("round {round}\n").as_bytes());
        repo.commit(&format!("round {round}"));
        repo.git(&["repack", "-q"]);
    }
    for round in 0..4 {
        same_as_git(&repo, &format!("file{round}.md"));
    }
}

#[test]
fn a_multi_pack_index_changes_nothing() {
    let Some(repo) = Real::new() else { return };
    for round in 0..3 {
        repo.write(&format!("file{round}.md"), format!("round {round}\n").as_bytes());
        repo.commit(&format!("round {round}"));
        repo.git(&["repack", "-q"]);
    }
    repo.git(&["multi-pack-index", "write"]);
    assert!(repo.root().join(".git/objects/pack/multi-pack-index").exists());
    same_as_git(&repo, "file0.md");
    same_as_git(&repo, "file2.md");
}

/// The only pack of a repository, with its index made anew by `git index-pack` with those arguments.
fn index_made_with(repo: &Real, args: &str) -> std::path::PathBuf {
    let idx = fs::read_dir(repo.root().join(".git/objects/pack"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|extension| extension == "idx"))
        .unwrap();
    let pack = idx.with_extension("pack");
    fs::remove_file(&idx).unwrap();
    repo.git(&["index-pack", args, "-o", idx.to_str().unwrap(), pack.to_str().unwrap()]);
    idx
}

#[test]
fn an_index_with_eight_byte_offsets_reads_as_git_says() {
    let Some(repo) = Real::new() else { return };
    long_history(&repo, 5);
    repo.git(&["gc", "-q"]);
    let idx = index_made_with(&repo, "--index-version=2,12");
    let objects = u64::from(
        Idx::parse(Box::new(OnDisk::open(&idx).unwrap().unwrap()))
            .unwrap()
            .objects(),
    );
    let all_but_the_first_are_wide = 1032 + objects * 28 + (objects - 1) * 8 + 40;
    assert_eq!(fs::metadata(&idx).unwrap().len(), all_but_the_first_are_wide);
    every_commit_same_as_git(&repo, "long.md");
}

#[test]
fn an_index_of_the_old_version_cannot_be_read() {
    let Some(repo) = Real::new() else { return };
    long_history(&repo, 2);
    repo.git(&["gc", "-q"]);
    index_made_with(&repo, "--index-version=1");
    let path = repo.root().join("long.md");
    assert_eq!(in_the_last_commit(&path), Err(ReadError::Unsupported));
}

#[test]
fn packed_refs_read_as_git_says() {
    let Some(repo) = Real::new() else { return };
    repo.write("a.md", b"x\n");
    repo.commit("one");
    repo.git(&["tag", "-a", "v1", "-m", "a tag"]);
    repo.git(&["branch", "side"]);
    repo.git(&["pack-refs", "--all"]);
    assert!(!repo.root().join(".git/refs/heads/main").exists());
    same_as_git(&repo, "a.md");
}

#[test]
fn a_detached_head_reads_as_git_says() {
    let Some(repo) = Real::new() else { return };
    repo.write("a.md", b"old\n");
    repo.commit("one");
    let old = commit_git_says(repo.root());
    repo.write("a.md", b"new\n");
    repo.commit("two");
    repo.git(&["checkout", "-q", "--detach", &old]);
    let head = fs::read_to_string(repo.root().join(".git/HEAD")).unwrap();
    assert_eq!(head.trim(), old);
    same_as_git(&repo, "a.md");
    assert_eq!(
        in_the_last_commit(&repo.root().join("a.md")).unwrap().unwrap(),
        b"old\n"
    );
}

#[test]
fn a_head_on_an_annotated_tag_stands_on_its_commit() {
    let Some(repo) = Real::new() else { return };
    repo.write("a.md", b"x\n");
    repo.commit("one");
    repo.git(&["tag", "-a", "v1", "-m", "a tag"]);
    repo.git(&["tag", "-a", "v2", "-m", "a tag of a tag", "v1"]);
    repo.git(&["symbolic-ref", "HEAD", "refs/tags/v2"]);
    same_as_git(&repo, "a.md");
    repo.git(&["pack-refs", "--all"]);
    same_as_git(&repo, "a.md");
    assert!(
        fs::read_to_string(repo.root().join(".git/packed-refs"))
            .unwrap()
            .contains("\n^")
    );
    repo.git(&["gc", "-q"]);
    same_as_git(&repo, "a.md");
}

#[test]
fn a_branch_without_a_commit_has_no_version_and_no_commit() {
    let Some(repo) = Real::new() else { return };
    let path = repo.write("a.md", b"not committed\n");
    let repository = find(&path).unwrap();
    assert_eq!(repository.head_commit(), Ok(None));
    assert_eq!(repository.head_version(&path), Ok(None));
}

#[test]
fn a_chain_of_symbolic_refs_ends_where_git_ends_it() {
    let Some(repo) = Real::new() else { return };
    let path = repo.write("a.md", b"x\n");
    repo.commit("one");
    for (from, to) in [("a", "b"), ("b", "c"), ("c", "main")] {
        repo.git(&[
            "symbolic-ref",
            &format!("refs/heads/{from}"),
            &format!("refs/heads/{to}"),
        ]);
    }
    repo.git(&["symbolic-ref", "HEAD", "refs/heads/a"]);
    same_as_git(&repo, "a.md");
    repo.git(&["symbolic-ref", "refs/heads/zero", "refs/heads/a"]);
    repo.git(&["symbolic-ref", "HEAD", "refs/heads/zero"]);
    assert!(
        !super::support::git_output(repo.root(), &["rev-parse", "--verify", "HEAD"])
            .status
            .success()
    );
    assert_eq!(in_the_last_commit(&path), Err(ReadError::Damaged));
}

#[test]
fn a_linked_worktree_has_its_own_head() {
    let Some(repo) = Real::new() else { return };
    repo.write("a.md", b"main\n");
    repo.commit("one");
    let linked = repo.root().parent().unwrap().join("linked");
    repo.git(&["worktree", "add", "-q", "-b", "side", linked.to_str().unwrap()]);
    fs::write(linked.join("a.md"), "side\n").unwrap();
    git(&linked, &["commit", "-q", "-am", "on the side"]);
    same_as_git_in(&linked, "a.md");
    same_as_git(&repo, "a.md");
    let repository = find(&linked.join("a.md")).unwrap();
    assert_eq!(repository.work_tree, linked.canonicalize().unwrap());
    assert_eq!(
        repository.head_version(&linked.join("a.md")).unwrap().unwrap(),
        b"side\n"
    );
    repo.git(&["gc", "-q"]);
    same_as_git_in(&linked, "a.md");
}

#[test]
fn a_ref_of_a_linked_worktree_lies_in_its_own_directory() {
    let Some(repo) = Real::new() else { return };
    repo.write("a.md", b"x\n");
    repo.commit("one");
    let linked = repo.root().parent().unwrap().join("linked");
    repo.git(&["worktree", "add", "-q", "--detach", linked.to_str().unwrap()]);
    git(&linked, &["update-ref", "refs/worktree/mine", "HEAD"]);
    git(&linked, &["symbolic-ref", "HEAD", "refs/worktree/mine"]);
    same_as_git_in(&linked, "a.md");
}

#[test]
fn a_submodule_is_a_repository_of_its_own() {
    let Some(inner) = Real::new() else { return };
    inner.write("inner.md", b"in the submodule\n");
    inner.commit("inner");
    let Some(outer) = Real::new() else { return };
    outer.write("outer.md", b"in the repository around it\n");
    outer.git(&["submodule", "add", "-q", inner.root().to_str().unwrap(), "sub"]);
    outer.commit("with a submodule");
    let sub = outer.root().join("sub");
    assert!(sub.join(".git").is_file());
    same_as_git_in(&sub, "inner.md");
    same_as_git(&outer, "outer.md");
    let around = find(&outer.root().join("outer.md")).unwrap();
    assert_eq!(
        around.head_version(&sub),
        Ok(None),
        "the entry of a submodule is no file"
    );
    assert_eq!(around.head_version(&sub.join("inner.md")), Ok(None), "and no folder");
    assert_eq!(
        find(&sub.join("inner.md")).unwrap().work_tree,
        sub.canonicalize().unwrap()
    );
}

#[test]
fn objects_of_a_shared_clone_come_from_its_alternates() {
    let Some(repo) = Real::new() else { return };
    long_history(&repo, 6);
    repo.git(&["gc", "-q"]);
    let clone = repo.root().parent().unwrap().join("clone");
    repo.git(&[
        "clone",
        "-q",
        "--shared",
        repo.root().to_str().unwrap(),
        clone.to_str().unwrap(),
    ]);
    assert!(clone.join(".git/objects/info/alternates").is_file());
    assert!(
        !fs::read_dir(clone.join(".git/objects/pack"))
            .unwrap()
            .any(|entry| entry.is_ok())
    );
    same_as_git_in(&clone, "long.md");
    fs::write(clone.join("own.md"), "only here\n").unwrap();
    git(&clone, &["add", "own.md"]);
    git(&clone, &["commit", "-q", "-m", "own"]);
    same_as_git_in(&clone, "long.md");
    same_as_git_in(&clone, "own.md");
}

#[test]
fn alternates_go_as_deep_as_git_goes() {
    if !git_is_here() {
        return;
    }
    let temp = Temp::new();
    let mut previous: Option<std::path::PathBuf> = None;
    for step in 0..9 {
        let folder = temp.path().join(format!("store{step}"));
        fs::create_dir_all(&folder).unwrap();
        git(&folder, &["init", "-q"]);
        if let Some(previous) = &previous {
            let objects = previous.join(".git/objects").canonicalize().unwrap();
            fs::write(
                folder.join(".git/objects/info/alternates"),
                format!("{}\n", objects.display()),
            )
            .unwrap();
        } else {
            fs::write(folder.join("a.md"), "deep\n").unwrap();
            git(&folder, &["add", "a.md"]);
            git(&folder, &["commit", "-q", "-m", "deep"]);
        }
        previous = Some(folder);
    }
    let commit = commit_git_says(&temp.path().join("store0"));
    let commit = parse_hex(commit.as_bytes()).unwrap();
    for step in 0..9 {
        let folder = temp.path().join(format!("store{step}"));
        let store = crate::store::Store::open(&folder.join(".git/objects")).unwrap();
        let git_finds_it = super::support::git_output(&folder, &["cat-file", "-e", &commit.to_string()])
            .status
            .success();
        assert_eq!(store.read_any(&commit).is_ok(), git_finds_it, "{step} alternates down");
    }
}

#[test]
fn a_file_in_a_subfolder_reads_as_git_says() {
    let Some(repo) = Real::new() else { return };
    repo.write("docs/deep/down/a.md", b"down here\n");
    repo.write("docs/other.md", b"beside it\n");
    repo.commit("one");
    same_as_git(&repo, "docs/deep/down/a.md");
    same_as_git(&repo, "docs/other.md");
}

#[test]
fn a_name_with_spaces_and_umlauts_reads_as_git_says() {
    let Some(repo) = Real::new() else { return };
    repo.write("Mein Ordner/Übersicht für März.md", b"Inhalt\n");
    repo.commit("one");
    same_as_git(&repo, "Mein Ordner/Übersicht für März.md");
}

#[test]
fn an_empty_file_is_an_empty_version() {
    let Some(repo) = Real::new() else { return };
    let path = repo.write("empty.md", b"");
    repo.commit("one");
    assert_eq!(in_the_last_commit(&path), Ok(Some(Vec::new())));
}

#[test]
fn a_large_file_reads_as_git_says_loose_and_packed() {
    let Some(repo) = Real::new() else { return };
    let mut random = super::support::Random::new(77);
    let bytes: Vec<u8> = (0..4_000_000)
        .map(|_| u8::try_from(random.below(256)).unwrap())
        .collect();
    repo.write("large.bin", &bytes);
    repo.commit("one");
    assert_eq!(
        in_the_last_commit(&repo.root().join("large.bin")),
        Ok(Some(bytes.clone()))
    );
    same_as_git(&repo, "large.bin");
    repo.git(&["gc", "-q"]);
    assert_eq!(in_the_last_commit(&repo.root().join("large.bin")), Ok(Some(bytes)));
}

const COMPOSED: &str = "Bücher.md";
const DECOMPOSED: &str = "Bu\u{308}cher.md";

#[test]
fn a_decomposed_name_on_the_disk_finds_a_composed_name_in_the_commit() {
    let Some(repo) = Real::new() else { return };
    repo.git(&["config", "core.precomposeunicode", "false"]);
    let stored = stored_blob(&repo, b"composed in the commit\n");
    repo.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{stored},{COMPOSED}"),
    ]);
    repo.git(&["commit", "-q", "-m", "one"]);
    let listed = repo.git(&["ls-tree", "-z", "--name-only", "HEAD"]);
    assert!(listed.starts_with(COMPOSED.as_bytes()), "{listed:?}");
    let path = repo.write(DECOMPOSED, b"changed on the disk\n");
    assert_eq!(in_the_last_commit(&path).unwrap().unwrap(), b"composed in the commit\n");
}

#[test]
fn a_composed_name_on_the_disk_finds_a_decomposed_name_in_the_commit() {
    let Some(repo) = Real::new() else { return };
    repo.git(&["config", "core.precomposeunicode", "false"]);
    let stored = stored_blob(&repo, b"decomposed in the commit\n");
    repo.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{stored},{DECOMPOSED}"),
    ]);
    repo.git(&["commit", "-q", "-m", "one"]);
    let listed = repo.git(&["ls-tree", "-z", "--name-only", "HEAD"]);
    assert!(listed.starts_with(DECOMPOSED.as_bytes()), "{listed:?}");
    let path = repo.write(COMPOSED, b"changed on the disk\n");
    assert_eq!(
        in_the_last_commit(&path).unwrap().unwrap(),
        b"decomposed in the commit\n"
    );
}

fn stored_blob(repo: &Real, data: &[u8]) -> String {
    let file = repo.root().parent().unwrap().join("blob");
    fs::write(&file, data).unwrap();
    String::from_utf8(repo.git(&["hash-object", "-w", file.to_str().unwrap()]))
        .unwrap()
        .trim()
        .to_string()
}

#[test]
fn a_file_renamed_to_another_case_still_finds_its_version() {
    let Some(repo) = Real::new() else { return };
    repo.write("Docs/README.md", b"as committed\n");
    repo.commit("one");
    let lower = repo.root().join("docs");
    fs::rename(repo.root().join("Docs"), &lower).unwrap();
    let renamed = lower.join("readme.md");
    fs::rename(lower.join("README.md"), &renamed).unwrap();
    assert_eq!(in_the_last_commit(&renamed).unwrap().unwrap(), b"as committed\n");
}

#[test]
fn an_untracked_file_has_no_version() {
    let Some(repo) = Real::new() else { return };
    repo.write("a.md", b"x\n");
    repo.commit("one");
    let path = repo.write("untracked.md", b"y\n");
    assert_eq!(in_the_last_commit(&path), Ok(None));
    let deep = repo.write("new/folder/untracked.md", b"y\n");
    assert_eq!(in_the_last_commit(&deep), Ok(None));
}

#[test]
fn a_file_deleted_in_the_last_commit_has_no_version() {
    let Some(repo) = Real::new() else { return };
    repo.write("a.md", b"x\n");
    repo.write("b.md", b"stays\n");
    repo.commit("one");
    repo.git(&["rm", "-q", "a.md"]);
    repo.commit("two");
    let path = repo.write("a.md", b"back on the disk\n");
    assert_eq!(in_the_last_commit(&path), Ok(None));
}

#[test]
fn a_file_deleted_from_the_disk_still_has_its_version() {
    let Some(repo) = Real::new() else { return };
    let path = repo.write("a.md", b"x\n");
    repo.commit("one");
    fs::remove_file(&path).unwrap();
    assert_eq!(in_the_last_commit(&path).unwrap().unwrap(), b"x\n");
}

#[test]
fn a_folder_has_no_version() {
    let Some(repo) = Real::new() else { return };
    repo.write("docs/a.md", b"x\n");
    repo.commit("one");
    let repository = find(&repo.root().join("docs")).unwrap();
    assert_eq!(repository.head_version(&repo.root().join("docs")), Ok(None));
    assert_eq!(repository.head_version(repo.root()), Ok(None));
}

#[test]
fn a_file_inside_the_git_directory_has_no_version() {
    let Some(repo) = Real::new() else { return };
    repo.write("a.md", b"x\n");
    repo.commit("one");
    let config = repo.root().join(".git/config");
    assert_eq!(find(&config).unwrap().work_tree, repo.root().canonicalize().unwrap());
    assert_eq!(in_the_last_commit(&config), Ok(None));
}

#[test]
fn a_linked_worktree_with_relative_paths_reads_as_git_says() {
    let Some(repo) = Real::new() else { return };
    repo.write("a.md", b"x\n");
    repo.commit("one");
    let linked = repo.root().parent().unwrap().join("linked");
    let linked_text = linked.to_str().unwrap();
    repo.git(&["worktree", "add", "-q", "--relative-paths", "-b", "side", linked_text]);
    let config = fs::read_to_string(repo.root().join(".git/config")).unwrap();
    assert!(config.to_ascii_lowercase().contains("relativeworktrees"), "{config}");
    assert!(
        fs::read_to_string(linked.join(".git"))
            .unwrap()
            .starts_with("gitdir: ../")
    );
    same_as_git_in(&linked, "a.md");
}

#[test]
fn a_git_directory_kept_elsewhere_reads_as_git_says() {
    if !git_is_here() {
        return;
    }
    let temp = Temp::new();
    let work_tree = temp.path().join("work");
    let elsewhere = temp.path().join("elsewhere.git");
    fs::create_dir_all(&work_tree).unwrap();
    git(
        &work_tree,
        &["init", "-q", "--separate-git-dir", elsewhere.to_str().unwrap()],
    );
    assert!(work_tree.join(".git").is_file());
    fs::write(work_tree.join("a.md"), "x\n").unwrap();
    git(&work_tree, &["add", "a.md"]);
    git(&work_tree, &["commit", "-q", "-m", "one"]);
    same_as_git_in(&work_tree, "a.md");
}

#[test]
fn a_path_through_a_file_of_the_commit_has_no_version() {
    let Some(repo) = Real::new() else { return };
    let was_a_file = repo.write("a", b"x\n");
    repo.commit("one");
    fs::remove_file(&was_a_file).unwrap();
    let below = repo.write("a/b.md", b"y\n");
    assert_eq!(in_the_last_commit(&below), Ok(None));
}

#[test]
fn a_file_outside_the_work_tree_has_no_version() {
    let Some(repo) = Real::new() else { return };
    repo.write("a.md", b"x\n");
    repo.commit("one");
    let repository = find(&repo.root().join("a.md")).unwrap();
    let outside = repo.root().parent().unwrap().join("outside.md");
    fs::write(&outside, "y\n").unwrap();
    assert_eq!(repository.head_version(&outside), Ok(None));
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_stands_for_the_file_it_leads_to() {
    let Some(repo) = Real::new() else { return };
    let real = repo.write("docs/real.md", b"the real one\n");
    repo.commit("one");
    let link = repo.root().join("link.md");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert_eq!(in_the_last_commit(&link).unwrap().unwrap(), b"the real one\n");
    let folder_link = repo.root().join("folder");
    std::os::unix::fs::symlink(repo.root().join("docs"), &folder_link).unwrap();
    assert_eq!(
        in_the_last_commit(&folder_link.join("real.md")).unwrap().unwrap(),
        b"the real one\n"
    );
}

#[cfg(unix)]
#[test]
fn a_committed_link_is_what_git_stores_for_it() {
    let Some(repo) = Real::new() else { return };
    std::os::unix::fs::symlink("elsewhere.md", repo.root().join("link.md")).unwrap();
    repo.commit("one");
    fs::remove_file(repo.root().join("link.md")).unwrap();
    let path = repo.write("link.md", b"a file now\n");
    assert_eq!(in_the_last_commit(&path).unwrap().unwrap(), b"elsewhere.md");
    assert_eq!(git(repo.root(), &["cat-file", "blob", "HEAD:link.md"]), b"elsewhere.md");
}

#[test]
fn a_sha256_repository_is_found_and_cannot_be_read() {
    let Some(repo) = Real::with_init_args(&["--object-format=sha256"]) else {
        return;
    };
    let path = repo.write("a.md", b"x\n");
    repo.commit("one");
    let repository = find(&path).unwrap();
    assert_eq!(repository.head_commit(), Err(ReadError::Unsupported));
    assert_eq!(repository.head_version(&path), Err(ReadError::Unsupported));
}

#[test]
fn a_repository_with_a_reftable_cannot_be_read() {
    let Some(repo) = Real::with_init_args(&["--ref-format=reftable"]) else {
        return;
    };
    let path = repo.write("a.md", b"x\n");
    repo.commit("one");
    let repository = find(&path).unwrap();
    assert_eq!(repository.head_commit(), Err(ReadError::Unsupported));
    assert_eq!(repository.head_version(&path), Err(ReadError::Unsupported));
}

#[test]
fn line_endings_git_converted_are_no_change_against_the_disk() {
    let Some(repo) = Real::new() else { return };
    let on_disk = "a line\r\nanother line\r\n";
    let path = repo.write("crlf.md", on_disk.as_bytes());
    repo.git(&["-c", "core.autocrlf=true", "add", "crlf.md"]);
    repo.git(&["-c", "core.autocrlf=true", "commit", "-q", "-m", "one"]);
    let version = in_the_last_commit(&path).unwrap().unwrap();
    assert_eq!(
        version, b"a line\nanother line\n",
        "the bytes of the commit, without conversion"
    );
    assert_eq!(version, git(repo.root(), &["cat-file", "blob", "HEAD:crlf.md"]));
    assert_eq!(diff::changes(std::str::from_utf8(&version).unwrap(), on_disk), []);
}

#[test]
fn a_partial_clone_reads_what_it_has_and_a_missing_object_is_said_to_be() {
    let Some(repo) = Real::new() else { return };
    repo.git(&["config", "uploadpack.allowFilter", "true"]);
    repo.write("a.md", b"present\n");
    repo.write("b.md", b"left behind\n");
    repo.commit("one");
    let clone = repo.root().parent().unwrap().join("clone");
    let source = format!("file://{}", repo.root().display());
    git(
        repo.root(),
        &[
            "clone",
            "-q",
            "--no-checkout",
            "--filter=blob:none",
            &source,
            clone.to_str().unwrap(),
        ],
    );
    let config = fs::read_to_string(clone.join(".git/config")).unwrap();
    assert!(config.to_ascii_lowercase().contains("partialclone"), "{config}");
    git(&clone, &["checkout", "-q", "HEAD", "--", "a.md"]);
    same_as_git_in(&clone, "a.md");
    assert_eq!(in_the_last_commit(&clone.join("b.md")), Err(ReadError::Damaged));
}
