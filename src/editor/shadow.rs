//! Copies of unsaved text for what the panic hook never hears of: a stack overflow, a segfault, being killed for
//! memory. Each run of the app keeps its copies in a folder of its own and removes them on a normal end, so a folder
//! left behind by a process that is gone holds what would otherwise be lost.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::OnceLock;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::{Retained, Weak};
use objc2_foundation::NSTimer;

use super::{EditorController, OPEN_EDITORS, recovery_dir};
use crate::util::{Lane, file_name_of, process_exists};

/// How often unsaved text is copied, and so how much a crash can cost at most.
const EVERY_SECONDS: f64 = 5.0;

/// Not the disk thread: a network volume that stopped answering must not stop the copies.
static WRITER: Lane = Lane::new("splat-copies");

/// The file an editor's text was copied to, and the version of that text; `None` until a copy reached the file.
struct KeptCopy {
    file: PathBuf,
    version: Option<u64>,
}

thread_local! {
    static COPIES: RefCell<HashMap<usize, KeptCopy>> = RefCell::new(HashMap::new());
}

/// The folders of all runs. A self-test without an output folder must not touch the user's copies.
fn copies_root() -> Option<PathBuf> {
    if crate::test_hooks::SELFTEST {
        crate::test_hooks::output_dir()?;
    }
    Some(recovery_dir()?.parent()?.join("Unsaved"))
}

fn own_folder() -> Option<PathBuf> {
    Some(copies_root()?.join(std::process::id().to_string()))
}

/// A file in the folder of a run that the run keeps locked for as long as it lives; the system lets go of it however the
/// process ends, and a number of another process that came round again holds no lock.
const RUN_LOCK: &str = ".running";

/// Takes the lock of this run on its folder, once.
fn keep_run_lock(folder: &Path) {
    static LOCK: OnceLock<Option<std::fs::File>> = OnceLock::new();
    LOCK.get_or_init(|| {
        let file = std::fs::File::create(folder.join(RUN_LOCK)).ok()?;
        file.try_lock().ok()?;
        Some(file)
    });
}

/// Whether the run of a folder still lives: by its lock, or for a folder without one by its process number.
fn run_is_alive(folder: &Path, pid: u32) -> bool {
    match std::fs::File::open(folder.join(RUN_LOCK)) {
        Ok(file) => matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
        Err(_) => process_exists(pid),
    }
}

/// A path in `folder` nothing stands at: process numbers come round again, and an older copy must not be overwritten.
fn free_name(folder: &Path, name: &str) -> PathBuf {
    let plain = folder.join(name);
    if !plain.exists() {
        return plain;
    }
    (2..)
        .map(|n| folder.join(format!("{n}-{name}")))
        .find(|p| !p.exists())
        .unwrap_or(plain)
}

/// Recovered texts stay this long: the next start pointed to them, and they hold what may be private.
const KEPT_FOR: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 60 * 60);

/// Removes what lies in `recovery` for longer than `KEPT_FOR`.
fn clear_old_recovered(recovery: &Path, now: std::time::SystemTime) {
    for file in std::fs::read_dir(recovery).into_iter().flatten().flatten() {
        let age = file
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok());
        if age.is_some_and(|a| a > KEPT_FOR) {
            let _ = std::fs::remove_file(file.path());
        }
    }
}

/// `KEPT_FOR` and the offer to look at recovered texts both go by this date, and a move leaves that of the last copy, which
/// may lie further back than the time a text is kept.
fn start_keeping(recovered: &Path) {
    let dated = std::fs::File::options()
        .write(true)
        .open(recovered)
        .and_then(|file| file.set_modified(std::time::SystemTime::now()));
    if let Err(error) = dated {
        crate::diag::report("dating a recovered text failed", error);
    }
}

/// Moves what ended runs left in `root` to `recovery`, and returns what it moved.
fn collect_ended_runs(root: &Path, recovery: &Path, alive: impl Fn(&Path, u32) -> bool) -> Vec<PathBuf> {
    let mut collected = Vec::new();
    for run in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let name = run.file_name();
        let Some(pid) = name.to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if alive(&run.path(), pid) {
            continue;
        }
        let mut left_behind = false;
        for file in std::fs::read_dir(run.path()).into_iter().flatten().flatten() {
            if file.file_name() == RUN_LOCK {
                continue;
            }
            let name = format!("{pid}-{}", file.file_name().to_string_lossy());
            let target = free_name(recovery, &name);
            if std::fs::create_dir_all(recovery).is_ok() && std::fs::rename(file.path(), &target).is_ok() {
                start_keeping(&target);
                collected.push(target);
            } else {
                left_behind = true;
            }
        }
        // What could not move stays, for the next start to try again: it may be the only copy of the text.
        if !left_behind {
            let _ = std::fs::remove_dir_all(run.path());
        }
    }
    collected
}

/// Moves what ended runs left behind to the recovery folder, where `offer_recovered` points to it.
pub(super) fn collect_left_behind() {
    if let (Some(root), Some(recovery)) = (copies_root(), recovery_dir()) {
        collect_ended_runs(&root, &recovery, run_is_alive);
        clear_old_recovered(&recovery, std::time::SystemTime::now());
    }
}

/// Takes over what ended runs left behind, and from now on copies unsaved text.
pub fn keep_copies(_mtm: MainThreadMarker) {
    collect_left_behind();
    let block = RcBlock::new(move |_: NonNull<NSTimer>| crate::util::guarded((), copy_unsaved));
    // SAFETY: The block only touches main-thread objects, and the timer runs on the main run loop it is scheduled on.
    let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(EVERY_SECONDS, true, &block) };
    timer.setTolerance(EVERY_SECONDS / 2.0);
}

/// Copies the text of every document with changes since its last copy, and removes the copies nobody needs anymore.
pub(super) fn copy_unsaved() {
    let Some(folder) = own_folder() else { return };
    let editors: Vec<Retained<EditorController>> =
        OPEN_EDITORS.with(|e| e.borrow().iter().filter_map(Weak::load).collect());
    let mut unsaved = Vec::new();
    for editor in &editors {
        let edited = editor.document().is_some_and(|d| d.isDocumentEdited());
        if edited && let Some(version) = editor.version_of_shown_text() {
            unsaved.push((editor, version));
        }
    }
    let ids: Vec<usize> = unsaved.iter().map(|(e, _)| Retained::as_ptr(e) as usize).collect();
    let outdated: Vec<PathBuf> = COPIES.with(|c| {
        let mut copies = c.borrow_mut();
        let gone: Vec<usize> = copies.keys().filter(|id| !ids.contains(id)).copied().collect();
        gone.iter()
            .filter_map(|id| copies.remove(id))
            .map(|copy| copy.file)
            .collect()
    });
    let mut writes = Vec::new();
    for ((editor, version), id) in unsaved.into_iter().zip(ids) {
        let file = COPIES.with(|c| {
            let mut copies = c.borrow_mut();
            match copies.get_mut(&id) {
                Some(copy) if copy.version == Some(version) => None,
                Some(copy) => {
                    copy.version = Some(version);
                    Some(copy.file.clone())
                }
                None => {
                    let name = file_name_of(
                        &editor
                            .document()
                            .map_or_else(|| "Dokument".to_string(), |d| d.displayName().to_string()),
                    );
                    let file = folder.join(format!("{}-{name}.md", copies.len() + id % 100_000));
                    copies.insert(
                        id,
                        KeptCopy {
                            file: file.clone(),
                            version: Some(version),
                        },
                    );
                    Some(file)
                }
            }
        });
        if let Some(file) = file {
            writes.push(CopyToWrite {
                editor: id,
                file,
                text: editor.source_text(),
            });
        }
    }
    if writes.is_empty() && outdated.is_empty() {
        return;
    }
    WRITER.run(move || {
        for file in outdated {
            let _ = std::fs::remove_file(file);
        }
        let not_written = write_copies(&folder, writes);
        if !not_written.is_empty() {
            crate::util::main_soon(move || copy_again(&not_written));
        }
    });
}

struct CopyToWrite {
    editor: usize,
    file: PathBuf,
    text: String,
}

/// Writes the copies and returns the editors whose copy did not reach its file.
fn write_copies(folder: &Path, writes: Vec<CopyToWrite>) -> Vec<usize> {
    if let Err(error) = std::fs::create_dir_all(folder) {
        crate::diag::report("the folder for copies of unsaved text cannot be made", error);
        return writes.iter().map(|write| write.editor).collect();
    }
    keep_run_lock(folder);
    let mut not_written = Vec::new();
    for write in writes {
        // Through a second file, so a crash while writing leaves the last copy whole.
        let partial = write.file.with_extension("partial");
        if let Err(error) = std::fs::write(&partial, write.text).and_then(|()| std::fs::rename(&partial, &write.file)) {
            crate::diag::report("copying unsaved text failed", error);
            not_written.push(write.editor);
        }
    }
    not_written
}

/// The next round of copies writes these again, though their text has not changed since.
fn copy_again(editors: &[usize]) {
    COPIES.with(|copies| {
        let mut copies = copies.borrow_mut();
        for editor in editors {
            if let Some(copy) = copies.get_mut(editor) {
                copy.version = None;
            }
        }
    });
}

/// The folder for the files the share menu hands out, with the text as it stands, saved or not. Each run has its own, for
/// `discard_share_files` to tell whose it is.
pub(super) fn share_root() -> PathBuf {
    std::env::temp_dir().join("Splat-Teilen")
}

/// The folder of this run for one shared file.
pub(super) fn new_share_folder() -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let dir = share_root()
        .join(std::process::id().to_string())
        .join(stamp.to_string());
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// How long a share service gets to read a file after its window closed; a transfer can outlast the window.
const SHARED_FOR: std::time::Duration = std::time::Duration::from_secs(600);

/// A window with a shared file has closed.
pub(super) fn discard_share_file_later(file: &Path) {
    let Some(folder) = file.parent().map(Path::to_path_buf) else {
        return;
    };
    crate::util::main_after(SHARED_FOR, move || {
        crate::util::DISK.run(move || remove_share_folder(&folder));
    });
}

fn remove_share_folder(folder: &Path) {
    if let Err(error) = std::fs::remove_dir_all(folder)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        crate::diag::report("removing a shared file failed", error);
    }
}

/// Removes the shared files of this run, as it ends, or those of runs that are gone, as it starts.
pub fn discard_share_files(ending: bool) {
    for folder in share_runs_to_remove(&share_root(), std::process::id(), ending, process_exists) {
        remove_share_folder(&folder);
    }
}

fn share_runs_to_remove(root: &Path, own: u32, ending: bool, lives: impl Fn(u32) -> bool) -> Vec<PathBuf> {
    let Ok(runs) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    runs.flatten()
        .filter(|run| {
            let pid = run.file_name().to_str().and_then(|name| name.parse::<u32>().ok());
            match pid {
                Some(pid) if pid == own => ending,
                Some(pid) => !lives(pid),
                None => true,
            }
        })
        .map(|run| run.path())
        .collect()
}

/// Removes the copies of this run, when it ends normally or the panic hook has saved the texts.
pub fn discard_copies() {
    let Some(folder) = own_folder() else { return };
    // Behind what the writer still has to do, or that would bring the folder back and a normal end would look like a crash
    // at the next start. A writer that hangs on a dead volume may not hold up the end of the app.
    let (done, finished) = std::sync::mpsc::channel();
    let behind_the_writes = folder.clone();
    WRITER.run(move || {
        let _ = std::fs::remove_dir_all(behind_the_writes);
        let _ = done.send(());
    });
    if finished.recv_timeout(std::time::Duration::from_secs(1)).is_err() {
        let _ = std::fs::remove_dir_all(folder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_files_go_with_their_run() {
        let root = std::env::temp_dir().join(format!("splat-share-{}", std::process::id()));
        for run in ["11", "22", "33", "alt"] {
            std::fs::create_dir_all(root.join(run)).unwrap();
        }
        let removed = |ending| {
            let mut names: Vec<String> = share_runs_to_remove(&root, 11, ending, |pid| pid == 22)
                .iter()
                .filter_map(|p| p.file_name()?.to_str().map(str::to_string))
                .collect();
            names.sort();
            names
        };
        assert_eq!(
            removed(false),
            ["33", "alt"],
            "neither this run's nor another that lives"
        );
        assert_eq!(removed(true), ["11", "33", "alt"]);
        std::fs::remove_dir_all(&root).unwrap();
        assert!(share_runs_to_remove(&root, 11, true, |_| true).is_empty());
    }

    #[test]
    fn copies_of_ended_runs_move_to_the_recovery_folder() {
        let root = std::env::temp_dir().join(format!("splat-shadow-{}", std::process::id()));
        let (unsaved, recovery) = (root.join("Unsaved"), root.join("Recovery"));
        for (run, file) in [("111", "1-Notiz.md"), ("222", "2-Brief.md"), ("kein-prozess", "x.md")] {
            std::fs::create_dir_all(unsaved.join(run)).unwrap();
            std::fs::write(unsaved.join(run).join(file), "Text").unwrap();
        }
        let collected = collect_ended_runs(&unsaved, &recovery, |_, pid| pid == 222);
        assert_eq!(collected, [recovery.join("111-1-Notiz.md")]);
        assert_eq!(std::fs::read_to_string(&collected[0]).unwrap(), "Text");
        assert!(!unsaved.join("111").exists(), "the ended run's folder goes");
        assert!(
            unsaved.join("222/2-Brief.md").exists(),
            "a running app keeps its copies"
        );
        assert!(unsaved.join("kein-prozess/x.md").exists());
        assert!(collect_ended_runs(&root.join("Fehlt"), &recovery, |_, _| false).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn copies_that_cannot_move_stay_where_they_are() {
        let root = std::env::temp_dir().join(format!("splat-shadow-stuck-{}", std::process::id()));
        let unsaved = root.join("Unsaved");
        std::fs::create_dir_all(unsaved.join("111")).unwrap();
        std::fs::write(unsaved.join("111/1-Notiz.md"), "Text").unwrap();
        // A file where the recovery folder should be keeps it from being made.
        let recovery = root.join("Recovery");
        std::fs::write(&recovery, "").unwrap();
        assert!(collect_ended_runs(&unsaved, &recovery, |_, _| false).is_empty());
        assert_eq!(std::fs::read_to_string(unsaved.join("111/1-Notiz.md")).unwrap(), "Text");
        std::fs::remove_file(&recovery).unwrap();
        assert_eq!(
            collect_ended_runs(&unsaved, &recovery, |_, _| false),
            [recovery.join("111-1-Notiz.md")]
        );
        assert!(!unsaved.join("111").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_process_number_that_comes_round_again_overwrites_nothing() {
        let root = std::env::temp_dir().join(format!("splat-shadow-again-{}", std::process::id()));
        let (unsaved, recovery) = (root.join("Unsaved"), root.join("Recovery"));
        std::fs::create_dir_all(&recovery).unwrap();
        std::fs::write(recovery.join("111-1-Notiz.md"), "älter").unwrap();
        std::fs::create_dir_all(unsaved.join("111")).unwrap();
        std::fs::write(unsaved.join("111/1-Notiz.md"), "neuer").unwrap();
        let collected = collect_ended_runs(&unsaved, &recovery, |_, _| false);
        assert_eq!(collected, [recovery.join("2-111-1-Notiz.md")]);
        assert_eq!(
            std::fs::read_to_string(recovery.join("111-1-Notiz.md")).unwrap(),
            "älter"
        );
        assert_eq!(std::fs::read_to_string(&collected[0]).unwrap(), "neuer");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_text_is_kept_from_the_day_it_is_taken_over() {
        let root = std::env::temp_dir().join(format!("splat-shadow-late-{}", std::process::id()));
        let (unsaved, recovery) = (root.join("Unsaved"), root.join("Recovery"));
        std::fs::create_dir_all(unsaved.join("111")).unwrap();
        let copy = unsaved.join("111/1-Notiz.md");
        std::fs::write(&copy, "Text").unwrap();
        let long_ago = std::time::SystemTime::now() - KEPT_FOR - KEPT_FOR;
        std::fs::File::options()
            .write(true)
            .open(&copy)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        let collected = collect_ended_runs(&unsaved, &recovery, |_, _| false);
        clear_old_recovered(&recovery, std::time::SystemTime::now());
        assert_eq!(std::fs::read_to_string(&collected[0]).unwrap(), "Text");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_copy_that_did_not_reach_its_file_is_named_and_leaves_nothing_half_written() {
        let root = std::env::temp_dir().join(format!("splat-shadow-write-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let blocked = root.join("Datei");
        std::fs::write(&blocked, "").unwrap();
        let write = |folder: &Path, editor, name: &str| CopyToWrite {
            editor,
            file: folder.join(name),
            text: "Text".to_string(),
        };
        let beneath = blocked.join("Ordner");
        assert_eq!(write_copies(&beneath, vec![write(&beneath, 1, "1.md")]), [1]);
        let folder = root.join("Ordner");
        let writes = vec![write(&folder, 2, "2.md"), write(&folder.join("fehlt"), 3, "3.md")];
        assert_eq!(write_copies(&folder, writes), [3]);
        assert_eq!(std::fs::read_to_string(folder.join("2.md")).unwrap(), "Text");
        assert!(!folder.join("2.partial").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_copy_that_failed_is_written_again() {
        let file = PathBuf::from("/kein/Pfad.md");
        COPIES.with(|c| {
            c.borrow_mut().insert(1, KeptCopy { file, version: Some(7) });
        });
        copy_again(&[1, 2]);
        let version = COPIES.with(|c| c.borrow_mut().remove(&1).and_then(|copy| copy.version));
        assert_eq!(version, None);
    }

    #[test]
    fn only_old_recovered_texts_go() {
        let root = std::env::temp_dir().join(format!("splat-shadow-old-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("Notiz.md");
        std::fs::write(&file, "Text").unwrap();
        clear_old_recovered(&root, std::time::SystemTime::now());
        assert!(file.exists());
        clear_old_recovered(&root, std::time::SystemTime::now() + KEPT_FOR + KEPT_FOR);
        assert!(!file.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_number_that_came_round_again_does_not_keep_a_run_alive() {
        let root = std::env::temp_dir().join(format!("splat-shadow-lock-{}", std::process::id()));
        let (unsaved, recovery) = (root.join("Unsaved"), root.join("Recovery"));
        let run = unsaved.join(std::process::id().to_string());
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(run.join("1-Notiz.md"), "Text").unwrap();
        let lock = std::fs::File::create(run.join(RUN_LOCK)).unwrap();
        lock.try_lock().unwrap();
        assert!(run_is_alive(&run, std::process::id()), "it holds its lock");
        assert!(collect_ended_runs(&unsaved, &recovery, run_is_alive).is_empty());
        drop(lock);
        assert!(
            !run_is_alive(&run, std::process::id()),
            "this process lives, but the run of that folder has ended"
        );
        assert_eq!(
            collect_ended_runs(&unsaved, &recovery, run_is_alive).len(),
            1,
            "and its copy is taken over"
        );
        assert!(!run.exists(), "the lock goes with the folder");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn without_a_lock_the_process_number_decides() {
        let root = std::env::temp_dir().join(format!("splat-shadow-nolock-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(run_is_alive(&root, std::process::id()));
        assert!(!run_is_alive(&root, u32::MAX));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn only_running_processes_count_as_alive() {
        assert!(process_exists(std::process::id()));
        assert!(!process_exists(u32::MAX));
    }
}
