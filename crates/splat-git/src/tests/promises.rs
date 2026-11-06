//! What the crate exists for: it reads a foreign repository without letting that repository do anything.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::support::Real;
use crate::find;

/// The sources that ship, without the tests at the end of each.
fn shipped_sources() -> Vec<(&'static str, &'static str)> {
    let sources = [
        ("lib.rs", include_str!("../lib.rs")),
        ("config.rs", include_str!("../config.rs")),
        ("delta.rs", include_str!("../delta.rs")),
        ("diff.rs", include_str!("../diff.rs")),
        ("discover.rs", include_str!("../discover.rs")),
        ("inflate.rs", include_str!("../inflate.rs")),
        ("object.rs", include_str!("../object.rs")),
        ("pack.rs", include_str!("../pack.rs")),
        ("refs.rs", include_str!("../refs.rs")),
        ("source.rs", include_str!("../source.rs")),
        ("store.rs", include_str!("../store.rs")),
        ("tree.rs", include_str!("../tree.rs")),
    ];
    sources
        .into_iter()
        .map(|(name, text)| (name, text.split("#[cfg(test)]\nmod tests {").next().unwrap()))
        .collect()
}

#[test]
fn every_module_is_looked_at() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut on_disk: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .map(|path| path.file_name().unwrap().to_str().unwrap().to_string())
        .collect();
    let mut looked_at: Vec<String> = shipped_sources().iter().map(|(name, _)| name.to_string()).collect();
    on_disk.sort();
    looked_at.sort();
    assert_eq!(on_disk, looked_at);
}

#[test]
fn nothing_that_ships_can_start_a_process_write_reach_the_network_or_read_the_environment() {
    let forbidden = [
        "std::process",
        "Command::",
        "std::env",
        "env::",
        "std::net",
        "TcpStream",
        "UdpSocket",
        "File::create",
        "OpenOptions",
        "fs::write",
        "fs::copy",
        "fs::rename",
        "fs::remove",
        "create_dir",
        "set_permissions",
        "set_len",
        "unsafe ",
    ];
    for (name, text) in shipped_sources() {
        for word in forbidden {
            assert!(!text.contains(word), "{name} mentions {word}");
        }
    }
}

#[test]
fn a_repository_can_go_to_the_thread_that_reads_the_disk() {
    fn on_another_thread<T: Send + Sync>() {}
    on_another_thread::<crate::Repository>();
    on_another_thread::<crate::ReadError>();
}

#[test]
fn the_tests_are_cut_off_at_the_right_place() {
    let sources = shipped_sources();
    let diff = sources.iter().find(|(name, _)| *name == "diff.rs").unwrap().1;
    assert!(diff.contains("fn hunks_within"));
    assert!(!diff.contains("fn changes_come_in_the_order_of_the_text"));
}

#[cfg(unix)]
fn executable(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn what_a_repository_asks_to_run_is_not_run() {
    let Some(repo) = Real::new() else { return };
    let path = repo.write("a.md", b"x\n");
    repo.write(".gitattributes", b"*.md filter=evil\n");
    repo.commit("one");
    let marker = repo.root().parent().unwrap().join("marker");
    let script = repo.root().parent().unwrap().join("evil.sh");
    executable(&script, &format!("#!/bin/sh\ntouch '{}'\n", marker.display()));
    let script = script.display().to_string();
    repo.git(&["config", "core.fsmonitor", &script]);
    repo.git(&["config", "core.sshCommand", &script]);
    repo.git(&["config", "core.pager", &script]);
    repo.git(&["config", "filter.evil.clean", &script]);
    repo.git(&["config", "filter.evil.smudge", &script]);
    repo.git(&["config", "filter.evil.process", &script]);
    for hook in [
        "post-checkout",
        "pre-commit",
        "reference-transaction",
        "post-index-change",
    ] {
        executable(
            &repo.root().join(".git/hooks").join(hook),
            &format!("#!/bin/sh\nexec '{script}'\n"),
        );
    }
    assert!(!marker.exists(), "git itself has not run it so far");

    let repository = find(&path).unwrap();
    assert_eq!(repository.head_version(&path).unwrap().unwrap(), b"x\n");
    assert!(repository.head_commit().unwrap().is_some());
    assert!(!marker.exists());
}

/// Every file below a folder with its contents, size and time of change.
fn snapshot(folder: &Path) -> BTreeMap<PathBuf, (Vec<u8>, SystemTime)> {
    let mut files = BTreeMap::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let changed = fs::metadata(&path).unwrap().modified().unwrap();
                files.insert(path.clone(), (fs::read(&path).unwrap(), changed));
            }
        }
    }
    files
}

#[test]
fn reading_leaves_every_file_of_the_repository_as_it_was() {
    let Some(repo) = Real::new() else { return };
    let path = repo.write("docs/a.md", b"x\n");
    repo.commit("one");
    repo.git(&["gc", "-q"]);
    repo.write("docs/a.md", b"y\n");
    repo.commit("two");
    let before = snapshot(repo.root());
    let folders_before = fs::read_dir(repo.root().join(".git")).unwrap().count();
    let repository = find(&path).unwrap();
    assert!(repository.head_version(&path).unwrap().is_some());
    assert!(repository.head_commit().unwrap().is_some());
    assert_eq!(snapshot(repo.root()), before);
    assert_eq!(fs::read_dir(repo.root().join(".git")).unwrap().count(), folders_before);
}
