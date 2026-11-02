//! Copies of unsaved text for what the panic hook never hears of: a stack overflow, a segfault, being killed for
//! memory. Each run of the app keeps its copies in a folder of its own and removes them on a normal end, so a folder
//! left behind by a process that is gone holds what would otherwise be lost.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2::{DefinedClass, MainThreadMarker};
use objc2_foundation::NSTimer;

use super::{EditorController, OPEN_EDITORS, recovery_dir};
use crate::util::{Lane, file_name_of, process_exists};

/// How often unsaved text is copied, and so how much a crash can cost at most.
const EVERY_SECONDS: f64 = 5.0;

/// Not the disk thread: a network volume that stopped answering must not stop the copies.
static WRITER: Lane = Lane::new("splat-copies");

/// The file an editor's text was copied to, and the version of that text.
struct KeptCopy {
    file: PathBuf,
    version: u64,
}

thread_local! {
    static COPIES: RefCell<HashMap<usize, KeptCopy>> = RefCell::new(HashMap::new());
}

/// The folders of all runs. A self-test without an output folder must not touch the user's copies.
fn copies_root() -> Option<PathBuf> {
    #[cfg(feature = "selftest")]
    std::env::var_os("SPLAT_SELFTEST_OUT")?;
    Some(recovery_dir()?.parent()?.join("Unsaved"))
}

fn own_folder() -> Option<PathBuf> {
    Some(copies_root()?.join(std::process::id().to_string()))
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

/// Moves what ended runs left in `root` to `recovery`, and returns what it moved.
fn adopt(root: &Path, recovery: &Path, alive: impl Fn(u32) -> bool) -> Vec<PathBuf> {
    let mut adopted = Vec::new();
    for run in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let name = run.file_name();
        let Some(pid) = name.to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if alive(pid) {
            continue;
        }
        let mut left_behind = false;
        for file in std::fs::read_dir(run.path()).into_iter().flatten().flatten() {
            let name = format!("{pid}-{}", file.file_name().to_string_lossy());
            let target = free_name(recovery, &name);
            if std::fs::create_dir_all(recovery).is_ok() && std::fs::rename(file.path(), &target).is_ok() {
                adopted.push(target);
            } else {
                left_behind = true;
            }
        }
        // What could not move stays, for the next start to try again: it may be the only copy of the text.
        if !left_behind {
            let _ = std::fs::remove_dir_all(run.path());
        }
    }
    adopted
}

/// Moves what ended runs left behind to the recovery folder, where `offer_recovered` points to it.
pub(super) fn adopt_left_behind() {
    if let (Some(root), Some(recovery)) = (copies_root(), recovery_dir()) {
        adopt(&root, &recovery, process_exists);
        clear_old_recovered(&recovery, std::time::SystemTime::now());
    }
}

/// Takes over what ended runs left behind, and from now on copies unsaved text.
pub fn keep_copies(_mtm: MainThreadMarker) {
    adopt_left_behind();
    let block = RcBlock::new(move |_: NonNull<NSTimer>| copy_unsaved());
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
        if edited && editor.ivars().source.get().is_some() {
            unsaved.push(editor);
        }
    }
    let ids: Vec<usize> = unsaved.iter().map(|e| Retained::as_ptr(e) as usize).collect();
    let outdated: Vec<PathBuf> = COPIES.with(|c| {
        let mut copies = c.borrow_mut();
        let gone: Vec<usize> = copies.keys().filter(|id| !ids.contains(id)).copied().collect();
        gone.iter()
            .filter_map(|id| copies.remove(id))
            .map(|copy| copy.file)
            .collect()
    });
    let mut writes = Vec::new();
    for (editor, id) in unsaved.into_iter().zip(ids) {
        let version = editor.ivars().copy.version();
        let file = COPIES.with(|c| {
            let mut copies = c.borrow_mut();
            match copies.get_mut(&id) {
                Some(copy) if copy.version == version => None,
                Some(copy) => {
                    copy.version = version;
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
                            version,
                        },
                    );
                    Some(file)
                }
            }
        });
        if let Some(file) = file {
            writes.push((file, editor.snapshot().text.clone()));
        }
    }
    if writes.is_empty() && outdated.is_empty() {
        return;
    }
    WRITER.run(move || {
        for file in outdated {
            let _ = std::fs::remove_file(file);
        }
        if std::fs::create_dir_all(&folder).is_ok() {
            for (file, text) in writes {
                // Through a second file, so a crash while writing leaves the last copy whole.
                let partial = file.with_extension("partial");
                if std::fs::write(&partial, text).is_ok() {
                    let _ = std::fs::rename(&partial, &file);
                }
            }
        }
    });
}

/// Removes the copies of this run, when it ends normally or the panic hook has saved the texts.
pub fn discard_copies() {
    let Some(folder) = own_folder() else { return };
    // Behind what the writer still has to do, or that would bring the folder back and a normal end would look like a crash
    // at the next start. A writer that hangs on a dead volume may not hold up the end of the app.
    let (done, finished) = std::sync::mpsc::channel();
    let in_order = folder.clone();
    WRITER.run(move || {
        let _ = std::fs::remove_dir_all(in_order);
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
    fn copies_of_ended_runs_move_to_the_recovery_folder() {
        let root = std::env::temp_dir().join(format!("splat-shadow-{}", std::process::id()));
        let (unsaved, recovery) = (root.join("Unsaved"), root.join("Recovery"));
        for (run, file) in [("111", "1-Notiz.md"), ("222", "2-Brief.md"), ("kein-prozess", "x.md")] {
            std::fs::create_dir_all(unsaved.join(run)).unwrap();
            std::fs::write(unsaved.join(run).join(file), "Text").unwrap();
        }
        let adopted = adopt(&unsaved, &recovery, |pid| pid == 222);
        assert_eq!(adopted, [recovery.join("111-1-Notiz.md")]);
        assert_eq!(std::fs::read_to_string(&adopted[0]).unwrap(), "Text");
        assert!(!unsaved.join("111").exists(), "the ended run's folder goes");
        assert!(
            unsaved.join("222/2-Brief.md").exists(),
            "a running app keeps its copies"
        );
        assert!(unsaved.join("kein-prozess/x.md").exists());
        assert!(adopt(&root.join("Fehlt"), &recovery, |_| false).is_empty());
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
        assert!(adopt(&unsaved, &recovery, |_| false).is_empty());
        assert_eq!(std::fs::read_to_string(unsaved.join("111/1-Notiz.md")).unwrap(), "Text");
        std::fs::remove_file(&recovery).unwrap();
        assert_eq!(adopt(&unsaved, &recovery, |_| false), [recovery.join("111-1-Notiz.md")]);
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
        let adopted = adopt(&unsaved, &recovery, |_| false);
        assert_eq!(adopted, [recovery.join("2-111-1-Notiz.md")]);
        assert_eq!(
            std::fs::read_to_string(recovery.join("111-1-Notiz.md")).unwrap(),
            "älter"
        );
        assert_eq!(std::fs::read_to_string(&adopted[0]).unwrap(), "neuer");
        std::fs::remove_dir_all(&root).unwrap();
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
    fn only_running_processes_count_as_alive() {
        assert!(process_exists(std::process::id()));
        assert!(!process_exists(u32::MAX));
    }
}
