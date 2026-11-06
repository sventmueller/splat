//! Repositories built by hand to hurt, through the public calls: each gives its error, and in time.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use super::support::{
    HandMade, Packer, blob_entry_head, commit_text, copy, delta, insert, sparse_file, tree_bytes, write_stream, zlib,
    zlib_bomb,
};
use crate::object::{MAX_METADATA, MAX_OBJECT, Oid};
use crate::{ReadError, find};

const COMMIT: Oid = Oid([0xc0; 20]);
const TREE: Oid = Oid([0xb1; 20]);
const BLOB: Oid = Oid([0xb2; 20]);
const FILE: &str = "a.md";

/// A hand-made repository whose `HEAD` stands on `[0xc0; 20]`, with a tree that names `a.md` as `[0xb2; 20]`, leaving out the
/// commit, tree and blob for a test to provide however it likes.
fn with_head() -> (HandMade, PathBuf) {
    let repo = HandMade::new();
    repo.write("refs/heads/main", COMMIT.to_string().as_bytes());
    let file = repo.work_tree().join(FILE);
    fs::write(&file, "on the disk\n").unwrap();
    (repo, file)
}

fn with_commit_and_tree() -> (HandMade, PathBuf) {
    let (repo, file) = with_head();
    repo.loose(&COMMIT, "commit", commit_text(&TREE).as_bytes());
    repo.loose(&TREE, "tree", &tree_bytes(&[("100644", FILE, BLOB)]));
    (repo, file)
}

fn version(file: &Path) -> Result<Option<Vec<u8>>, ReadError> {
    find(file).unwrap().head_version(file)
}

fn commit_of(file: &Path) -> Result<Option<Oid>, ReadError> {
    find(file).unwrap().head_commit()
}

/// A repository with all it needs in one pack, to take apart.
fn packed() -> (HandMade, PathBuf, Vec<u8>, Vec<u8>) {
    let (repo, file) = with_head();
    let mut packer = Packer::new();
    packer.whole(COMMIT, 1, commit_text(&TREE).as_bytes());
    packer.whole(TREE, 2, &tree_bytes(&[("100644", FILE, BLOB)]));
    packer.whole(BLOB, 3, b"in the pack\n");
    let (pack, idx) = packer.finish();
    repo.raw_pack_files("one", &idx, &pack);
    (repo, file, pack, idx)
}

#[test]
fn a_repository_built_by_hand_reads_at_all() {
    let (repo, file, ..) = packed();
    assert_eq!(version(&file), Ok(Some(b"in the pack\n".to_vec())));
    assert_eq!(commit_of(&file), Ok(Some(COMMIT)));
    assert_eq!(find(&file).unwrap().work_tree, repo.work_tree().canonicalize().unwrap());
}

#[cfg(unix)]
#[test]
fn an_object_beyond_four_gibibytes_reads_from_a_pack_that_stays_on_the_disk() {
    use std::os::unix::fs::FileExt;
    let (repo, file) = with_commit_and_tree();
    let far = 5u64 << 30;
    let mut packer = Packer::new();
    packer.whole(Oid([0xa0; 20]), 3, b"near");
    packer.far_away(BLOB, far);
    let (near, idx) = packer.finish_with_wide_offsets(&[BLOB]);
    let (entries, trailer) = near.split_at(near.len() - 20);
    let mut entry = blob_entry_head(4);
    entry.extend_from_slice(&zlib(b"far\n"));
    repo.write_in_objects("pack/far.idx", &idx);
    let path = repo.objects().join("pack/far.pack");
    let sparse = fs::File::create(&path).unwrap();
    sparse.write_all_at(entries, 0).unwrap();
    sparse.write_all_at(&entry, far).unwrap();
    sparse.write_all_at(trailer, far + entry.len() as u64).unwrap();
    assert_eq!(version(&file), Ok(Some(b"far\n".to_vec())));
}

#[test]
fn a_truncated_index_is_damaged() {
    let (repo, file, pack, idx) = packed();
    for keep in [0, 7, 8, 1031, 1032, idx.len() / 2, idx.len() - 1] {
        repo.raw_pack_files("one", &idx[..keep], &pack);
        assert_eq!(version(&file), Err(ReadError::Damaged), "{keep}");
    }
}

#[test]
fn a_truncated_pack_is_damaged() {
    let (repo, file, pack, idx) = packed();
    for keep in [0, 11, 12, 31, 40, pack.len() - 1] {
        repo.raw_pack_files("one", &idx, &pack[..keep]);
        assert_eq!(version(&file), Err(ReadError::Damaged), "{keep}");
    }
}

#[test]
fn an_offset_past_the_end_of_the_pack_is_damaged() {
    let (repo, file, pack, idx) = packed();
    let table = 1032 + 3 * 24;
    for offset in [
        0u32,
        11,
        u32::try_from(pack.len()).unwrap() - 20,
        u32::try_from(pack.len()).unwrap(),
        0x7fff_ffff,
    ] {
        let mut lying = idx.clone();
        lying[table..table + 4].copy_from_slice(&offset.to_be_bytes());
        repo.raw_pack_files("one", &lying, &pack);
        assert_eq!(version(&file), Err(ReadError::Damaged), "{offset}");
    }
}

#[test]
fn an_eight_byte_offset_past_the_end_of_the_pack_is_damaged() {
    let (repo, file) = with_head();
    let mut packer = Packer::new();
    packer.whole(TREE, 2, b"");
    packer.whole(COMMIT, 1, commit_text(&TREE).as_bytes());
    let (pack, idx) = packer.finish_with_wide_offsets(&[COMMIT]);
    let mut lying = idx;
    let table = 1032 + 2 * 24 + 2 * 4;
    lying[table..table + 8].copy_from_slice(&u64::MAX.to_be_bytes());
    repo.raw_pack_files("one", &lying, &pack);
    assert_eq!(version(&file), Err(ReadError::Damaged));
}

#[test]
fn counts_in_an_index_that_fall_are_damaged() {
    let (repo, file, pack, mut idx) = packed();
    idx[8 + 4 * 10..8 + 4 * 11].copy_from_slice(&5u32.to_be_bytes());
    repo.raw_pack_files("one", &idx, &pack);
    assert_eq!(version(&file), Err(ReadError::Damaged));
}

#[test]
fn a_delta_that_reads_past_its_base_is_damaged() {
    let (repo, file) = with_commit_and_tree();
    let mut packer = Packer::new();
    let base = packer.whole(Oid([0xa0; 20]), 3, b"short");
    packer.ofs_delta(BLOB, base, &delta(5, 100, &[copy(0, 100)]));
    repo.pack("one", packer);
    assert_eq!(version(&file), Err(ReadError::Damaged));
}

#[test]
fn a_chain_deeper_than_the_cap_is_too_large() {
    let (repo, file) = with_commit_and_tree();
    let mut packer = Packer::new();
    let mut base = packer.whole(Oid([0xa0; 20]), 3, b"x");
    for step in 0u8..200 {
        let mut name = Oid([0xa1; 20]);
        name.0[1] = step;
        let name = if step == 199 { BLOB } else { name };
        base = packer.ofs_delta(name, base, &delta(1, 1, &[copy(0, 1)]));
    }
    repo.pack("one", packer);
    assert_eq!(version(&file), Err(ReadError::TooLarge));
}

#[test]
fn deltas_on_each_other_end_in_time() {
    let (repo, file) = with_commit_and_tree();
    let mut packer = Packer::new();
    packer.ref_delta(BLOB, Oid([0xa0; 20]), &delta(1, 1, &[insert(b"a")]));
    packer.ref_delta(Oid([0xa0; 20]), BLOB, &delta(1, 1, &[insert(b"b")]));
    repo.pack("one", packer);
    assert_eq!(version(&file), Err(ReadError::TooLarge));
}

#[test]
fn a_loose_zlib_bomb_declared_small_stops_at_what_it_declares() {
    let (repo, file) = with_commit_and_tree();
    write_stream(&repo.objects(), &BLOB, &zlib_bomb(b"blob 1024\0", 600 << 20));
    let started = Instant::now();
    assert_eq!(version(&file), Err(ReadError::Damaged));
    assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
}

#[test]
fn a_loose_zlib_bomb_declared_large_is_too_large_before_anything_inflates() {
    let (repo, file) = with_commit_and_tree();
    write_stream(
        &repo.objects(),
        &BLOB,
        &zlib_bomb(format!("blob {}\0", 600u64 << 20).as_bytes(), 600 << 20),
    );
    let started = Instant::now();
    assert_eq!(version(&file), Err(ReadError::TooLarge));
    assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
}

#[test]
fn a_loose_zlib_bomb_declared_at_the_limit_stops_at_the_limit() {
    let (repo, file) = with_commit_and_tree();
    write_stream(
        &repo.objects(),
        &BLOB,
        &zlib_bomb(format!("blob {MAX_OBJECT}\0").as_bytes(), MAX_OBJECT + (64 << 20)),
    );
    assert_eq!(version(&file), Err(ReadError::Damaged));
}

#[test]
fn a_packed_zlib_bomb_stops_at_what_it_declares() {
    let (repo, file) = with_commit_and_tree();
    let mut packer = Packer::new();
    let mut bytes = vec![0xb0 | 0xf, 0x7f];
    bytes.extend_from_slice(&zlib_bomb(b"", 600 << 20));
    packer.raw(BLOB, bytes);
    repo.pack("one", packer);
    let started = Instant::now();
    assert_eq!(version(&file), Err(ReadError::Damaged));
    assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
}

#[test]
fn huge_declared_sizes_cost_nothing() {
    for (kind, oid) in [("commit", COMMIT), ("tree", TREE), ("blob", BLOB)] {
        let (repo, file) = with_commit_and_tree();
        let declared = u64::MAX;
        write_stream(&repo.objects(), &oid, &zlib(format!("{kind} {declared}\0").as_bytes()));
        assert_eq!(version(&file), Err(ReadError::TooLarge), "{kind}");
    }
}

#[test]
fn huge_declared_sizes_in_a_pack_cost_nothing() {
    let parts = [
        (1u8, COMMIT, commit_text(&TREE).into_bytes()),
        (2, TREE, tree_bytes(&[("100644", FILE, BLOB)])),
        (3, BLOB, b"x".to_vec()),
    ];
    for huge in [COMMIT, TREE, BLOB] {
        let (repo, file) = with_head();
        let mut packer = Packer::new();
        for (kind, name, data) in &parts {
            if *name == huge {
                packer.declared(*name, *kind, MAX_OBJECT + 1, b"");
            } else {
                packer.whole(*name, *kind, data);
            }
        }
        repo.pack("one", packer);
        assert_eq!(version(&file), Err(ReadError::TooLarge), "{huge}");
    }
}

#[test]
fn a_tree_above_its_bound_is_too_large() {
    let (repo, file) = with_head();
    repo.loose(&COMMIT, "commit", commit_text(&TREE).as_bytes());
    write_stream(
        &repo.objects(),
        &TREE,
        &zlib(format!("tree {}\0", MAX_METADATA + 1).as_bytes()),
    );
    assert_eq!(version(&file), Err(ReadError::TooLarge));
}

#[test]
fn a_tree_entry_with_a_slash_is_damaged() {
    let (repo, file) = with_head();
    repo.loose(&COMMIT, "commit", commit_text(&TREE).as_bytes());
    repo.loose(&TREE, "tree", &tree_bytes(&[("100644", "x/../a.md", BLOB)]));
    assert_eq!(version(&file), Err(ReadError::Damaged));
}

#[test]
fn head_pointing_outside_the_git_directory_is_damaged() {
    let (repo, file) = with_commit_and_tree();
    repo.write("HEAD", b"ref: ../../outside\n");
    repo.write_beside_git_dir("outside", COMMIT.to_string().as_bytes());
    assert_eq!(commit_of(&file), Err(ReadError::Damaged));
    assert_eq!(version(&file), Err(ReadError::Damaged));
}

#[test]
fn a_ref_loop_is_damaged() {
    let (repo, file) = with_commit_and_tree();
    repo.write("refs/heads/main", b"ref: refs/heads/main\n");
    assert_eq!(commit_of(&file), Err(ReadError::Damaged));
    assert_eq!(version(&file), Err(ReadError::Damaged));
}

#[test]
fn head_on_something_that_is_no_commit_is_damaged() {
    for (kind, data) in [("blob", &b"text"[..]), ("tree", b"")] {
        let (repo, file) = with_head();
        repo.loose(&COMMIT, kind, data);
        assert_eq!(commit_of(&file), Err(ReadError::Damaged), "{kind}");
    }
}

#[test]
fn head_on_a_missing_commit_is_damaged() {
    let (_repo, file) = with_head();
    assert_eq!(commit_of(&file), Err(ReadError::Damaged));
    assert_eq!(version(&file), Err(ReadError::Damaged));
}

#[test]
fn a_tag_on_itself_ends() {
    let (repo, file) = with_head();
    repo.loose(&COMMIT, "tag", format!("object {COMMIT}\ntype tag\n").as_bytes());
    assert_eq!(commit_of(&file), Err(ReadError::Damaged));
}

#[test]
fn a_tag_without_a_target_is_damaged() {
    let (repo, file) = with_head();
    repo.loose(&COMMIT, "tag", b"nothing useful\n");
    assert_eq!(commit_of(&file), Err(ReadError::Damaged));
}

#[test]
fn a_commit_without_a_tree_is_damaged() {
    let (repo, file) = with_head();
    repo.loose(&COMMIT, "commit", b"author nobody\n");
    assert_eq!(version(&file), Err(ReadError::Damaged));
    assert_eq!(commit_of(&file), Ok(Some(COMMIT)), "the commit itself still has a name");
}

#[test]
fn a_tree_that_names_something_else_is_damaged() {
    let (repo, file) = with_head();
    repo.loose(&COMMIT, "commit", commit_text(&BLOB).as_bytes());
    repo.loose(&BLOB, "blob", b"no tree");
    assert_eq!(version(&file), Err(ReadError::Damaged));
}

#[test]
fn a_file_entry_that_names_a_tree_is_damaged() {
    let (repo, file) = with_commit_and_tree();
    repo.loose(&BLOB, "tree", b"");
    assert_eq!(version(&file), Err(ReadError::Damaged));
}

#[test]
fn a_tree_where_a_folder_is_expected_is_followed_and_anything_else_is_not() {
    let (repo, _) = with_head();
    repo.loose(&COMMIT, "commit", commit_text(&TREE).as_bytes());
    repo.loose(
        &TREE,
        "tree",
        &tree_bytes(&[
            ("40000", "docs", Oid([0xb3; 20])),
            ("100644", "plain", BLOB),
            ("160000", "sub", BLOB),
        ]),
    );
    repo.loose(
        &Oid([0xb3; 20]),
        "tree",
        &tree_bytes(&[
            ("100644", "a.md", BLOB),
            ("40000", "folder", Oid([0xb4; 20])),
            ("160000", "link", BLOB),
        ]),
    );
    repo.loose(&BLOB, "blob", b"found\n");
    let at = |path: &str| {
        let file = repo.work_tree().join(path);
        version(&file)
    };
    assert_eq!(at("docs/a.md"), Ok(Some(b"found\n".to_vec())));
    assert_eq!(at("docs/folder"), Ok(None), "a folder");
    assert_eq!(at("docs/link"), Ok(None), "a submodule");
    assert_eq!(at("plain/a.md"), Ok(None), "below a file");
    assert_eq!(at("sub/a.md"), Ok(None), "below a submodule");
    assert_eq!(at("gone/a.md"), Ok(None), "below nothing");
}

#[test]
fn a_path_deeper_than_the_cap_is_too_large_and_one_at_the_cap_is_not() {
    let (repo, _) = with_commit_and_tree();
    let mut deep = repo.work_tree().to_path_buf();
    for _ in 0..256 {
        deep.push("d");
    }
    fs::create_dir_all(&deep).unwrap();
    assert_eq!(version(&deep), Ok(None), "at the cap");
    assert_eq!(version(&deep.join("d")), Err(ReadError::TooLarge), "one below it");
}

#[test]
fn a_tree_nested_as_deep_as_a_path_can_go_reads() {
    let (repo, _) = with_head();
    repo.loose(&COMMIT, "commit", commit_text(&TREE).as_bytes());
    repo.loose(
        &TREE,
        "tree",
        &tree_bytes(&[("40000", "d", TREE), ("100644", FILE, BLOB)]),
    );
    repo.loose(&BLOB, "blob", b"far down\n");
    let mut deep = repo.work_tree().to_path_buf();
    for _ in 0..255 {
        deep.push("d");
    }
    assert_eq!(version(&deep.join(FILE)), Ok(Some(b"far down\n".to_vec())));
}

fn mkfifo(path: &Path) -> bool {
    Command::new("mkfifo")
        .arg(path)
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn a_named_pipe_is_refused_without_waiting_for_a_writer() {
    let (repo, file) = with_commit_and_tree();
    let hex = BLOB.to_string();
    let loose = repo.objects().join(&hex[..2]).join(&hex[2..]);
    fs::create_dir_all(loose.parent().unwrap()).unwrap();
    if !mkfifo(&loose) {
        eprintln!("mkfifo is absent: skipped");
        return;
    }
    assert_eq!(version(&file), Err(ReadError::Damaged));
    let repository = find(&file).unwrap();
    fs::remove_file(repo.git_dir().join("refs/heads/main")).unwrap();
    assert!(mkfifo(&repo.git_dir().join("refs/heads/main")));
    assert_eq!(repository.head_commit(), Err(ReadError::Damaged));
    fs::remove_file(repo.git_dir().join("HEAD")).unwrap();
    assert!(mkfifo(&repo.git_dir().join("HEAD")));
    assert_eq!(repository.head_commit(), Err(ReadError::Damaged));
    assert_eq!(
        find(&file),
        None,
        "a pipe is no HEAD, so this folder is no repository to begin with"
    );
}

#[cfg(unix)]
fn forbidden(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(unix)]
#[test]
fn what_the_system_refuses_is_a_failed_volume_and_no_damage() {
    let (repo, file) = with_commit_and_tree();
    let hex = BLOB.to_string();
    let folder = repo.objects().join(&hex[..2]);
    fs::create_dir_all(&folder).unwrap();
    forbidden(&folder, 0o000);
    let result = version(&file);
    forbidden(&folder, 0o755);
    assert_eq!(result, Err(ReadError::Io(std::io::ErrorKind::PermissionDenied)));

    let packs = repo.objects().join("pack");
    fs::create_dir_all(&packs).unwrap();
    forbidden(&packs, 0o000);
    let result = version(&file);
    forbidden(&packs, 0o755);
    assert_eq!(result, Err(ReadError::Io(std::io::ErrorKind::PermissionDenied)));

    let config = repo.git_dir().join("config");
    fs::write(&config, "").unwrap();
    forbidden(&config, 0o000);
    let result = version(&file);
    forbidden(&config, 0o644);
    assert_eq!(result, Err(ReadError::Io(std::io::ErrorKind::PermissionDenied)));

    let repository = find(&file).unwrap();
    let below = repo.work_tree().join("locked");
    fs::create_dir_all(&below).unwrap();
    forbidden(&below, 0o000);
    let result = repository.head_version(&below.join("a.md"));
    let found = find(&below.join("a.md"));
    forbidden(&below, 0o755);
    assert_eq!(result, Err(ReadError::Io(std::io::ErrorKind::PermissionDenied)));
    assert_eq!(
        found, None,
        "a path that cannot be followed lies in no repository anyone can name"
    );
}

#[test]
fn a_format_it_cannot_read_stops_every_call() {
    let (repo, file) = with_commit_and_tree();
    repo.write("config", b"[extensions]\n\tobjectformat = sha256\n");
    assert_eq!(commit_of(&file), Err(ReadError::Unsupported));
    assert_eq!(version(&file), Err(ReadError::Unsupported));
}

#[test]
fn a_config_above_its_bound_is_too_large() {
    let (repo, file) = with_commit_and_tree();
    sparse_file(&repo.git_dir().join("config"), crate::MAX_CONFIG + 1);
    assert_eq!(commit_of(&file), Err(ReadError::TooLarge));
}

#[test]
fn a_config_that_makes_no_sense_is_damaged() {
    let (repo, file) = with_commit_and_tree();
    repo.write("config", b"\xff\xfe");
    assert_eq!(commit_of(&file), Err(ReadError::Damaged));
}

#[test]
fn a_repository_without_a_config_is_read_as_the_default_format() {
    let (repo, file) = with_commit_and_tree();
    repo.loose(&BLOB, "blob", b"fine\n");
    assert!(!repo.git_dir().join("config").exists());
    assert_eq!(version(&file), Ok(Some(b"fine\n".to_vec())));
}

#[test]
fn a_folder_where_an_object_should_be_is_a_missing_object() {
    let (repo, file) = with_commit_and_tree();
    let hex = BLOB.to_string();
    fs::create_dir_all(repo.objects().join(&hex[..2]).join(&hex[2..])).unwrap();
    assert_eq!(version(&file), Err(ReadError::Damaged));
}

#[test]
fn head_that_disappeared_after_finding_the_repository_cannot_be_believed() {
    let (repo, file) = with_commit_and_tree();
    let repository = find(&file).unwrap();
    fs::remove_file(repo.git_dir().join("HEAD")).unwrap();
    assert_eq!(repository.head_commit(), Err(ReadError::Damaged));
    assert_eq!(repository.head_version(&file), Err(ReadError::Damaged));
}

#[test]
fn an_error_says_in_words_what_went_wrong() {
    let all = [
        ReadError::Unsupported,
        ReadError::Damaged,
        ReadError::TooLarge,
        ReadError::Io(std::io::ErrorKind::PermissionDenied),
    ];
    let words: std::collections::HashSet<String> = all.iter().map(ToString::to_string).collect();
    assert_eq!(words.len(), all.len());
    let error: Box<dyn std::error::Error> = Box::new(ReadError::Io(std::io::ErrorKind::PermissionDenied));
    assert!(error.to_string().contains("permission denied"));
}
