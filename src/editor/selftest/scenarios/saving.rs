//! Saving, unsaved text and its copies, and a file that changes on disk.

use std::cell::Cell;
use std::path::PathBuf;

use objc2::MainThreadOnly;
use objc2_app_kit::{NSAlertFirstButtonReturn, NSAlertSecondButtonReturn};
use objc2_foundation::{NSFileManager, NSRange};

use super::super::{Ctx, Step, append, applied, at_least, check, pause, settle, source, type_in_source, when};
use super::{before, end_sheet, remember, sheet, sizes, undo};
use crate::util::ns;

fn copies_of(c: &Ctx) -> Vec<PathBuf> {
    let folder = c.out.join("Unsaved").join(std::process::id().to_string());
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect()
}

pub(super) fn unsaved_copy_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            remember(c);
            type_in_source(&c.e, "Q");
            crate::editor::shadow::copy_unsaved();
        }),
        when(
            "the copy of the unsaved text to be written",
            |c| {
                copies_of(c)
                    .iter()
                    .any(|f| std::fs::read_to_string(f).is_ok_and(|t| t == source(&c.e)))
            },
            |c| {
                check(
                    "unsaved text has a copy that outlives a crash",
                    copies_of(c)
                        .iter()
                        .any(|f| std::fs::read_to_string(f).is_ok_and(|t| t == source(&c.e))),
                    "",
                );
                // What a run that crashed leaves behind.
                let ended = c.out.join("Unsaved/999999");
                let _ = std::fs::create_dir_all(&ended);
                let _ = std::fs::write(ended.join("Notiz.md"), "Text vor dem Absturz");
                crate::editor::shadow::collect_left_behind();
                check(
                    "the next start takes over the copies of a crashed run",
                    std::fs::read_to_string(c.out.join("Recovery/999999-Notiz.md"))
                        .is_ok_and(|t| t == "Text vor dem Absturz")
                        && !ended.exists(),
                    "",
                );
                check("a running app keeps its own copies", !copies_of(c).is_empty(), "");
                undo(c);
                if let Some(d) = c.e.document() {
                    d.updateChangeCount(objc2_app_kit::NSDocumentChangeType::ChangeCleared);
                }
                crate::editor::shadow::copy_unsaved();
            },
        ),
        when(
            "the copy of the saved text to go",
            |c| copies_of(c).is_empty(),
            |c| {
                check(
                    "a saved document needs no copy",
                    copies_of(c).is_empty(),
                    &format!("{:?}", copies_of(c)),
                );
                check(
                    "undoing leaves the text of before",
                    source(&c.e) == before(c),
                    &sizes(c),
                );
                if let Some(d) = c.e.document() {
                    d.updateChangeCount(objc2_app_kit::NSDocumentChangeType::ChangeDone);
                }
            },
        ),
    ]
}

/// The file changes while a formula's editor is open: its offsets mean nothing then, and nothing of it may reach the text.
pub(super) fn reload_under_an_open_editor_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            remember(c);
            append(&c.e, "\n\nFormel $a+b$ steht hier.\n");
        }),
        settle(|c| {
            let at = source(&c.e).find("a+b").unwrap_or(0);
            super::super::set_source_caret(&c.e, at);
            c.e.atom_editor().edit_at_selection();
            check(
                "the formula's editor opens for the reload check",
                c.e.atom_editor().is_open(),
                "no editor",
            );
        }),
        // Its popover is still fading in when it counts as open.
        at_least(
            0.3,
            "the formula's editor to open",
            |c| c.e.atom_editor().is_open(),
            |c| {
                let replaced = "# Von außen geändert\n\nGanz anderer Text.\n";
                c.e.replace_all_text(replaced);
                check(
                    "a reload closes the formula's editor",
                    !c.e.atom_editor().is_open(),
                    "still open",
                );
            },
        ),
        when(
            "the formula's editor to close and the reloaded text to settle",
            |c| !c.e.atom_editor().is_open() && super::super::settled(&c.e),
            |c| {
                check(
                    "nothing of a closed editor reaches the reloaded text",
                    source(&c.e) == "# Von außen geändert\n\nGanz anderer Text.\n",
                    &source(&c.e),
                );
                c.e.updater().replace_all_text(&before(c));
            },
        ),
        settle(|_| {}),
    ]
}

/// A step that runs once a look at the files that began after it started waiting has been taken in: what that look left alone
/// it will leave alone.
fn after_a_look(f: impl Fn(&Ctx) + 'static) -> Step {
    use std::sync::atomic::Ordering;
    let seen = Cell::new(None);
    when(
        "a look at the files",
        move |c| {
            crate::document::reload_changed_documents(c.e.mtm());
            let now = crate::test_hooks::LOOKS_TAKEN.load(Ordering::SeqCst);
            let first = seen.get().unwrap_or(now);
            seen.set(Some(first));
            now >= first + 2
        },
        f,
    )
}

/// What is saved is exactly what the source holds, and unsaved text outlives a change of its file by another program.
pub(super) fn saving_and_unsaved_text_steps() -> Vec<Step> {
    let path = |c: &Ctx| c.e.document().and_then(|d| d.fileURL()?.path()).map(|p| p.to_string());
    vec![
        applied(|c| {
            if let Some(d) = c.e.document() {
                unsafe { d.saveDocument(None) };
            }
        }),
        when(
            "the document to be saved",
            |c| c.e.document().is_some_and(|d| !d.isDocumentEdited()),
            move |c| {
                let saved = path(c).and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
                check(
                    "a saved file holds exactly the text of the source",
                    saved == c.e.exact_text().into_bytes(),
                    &format!("{} bytes, {} in the source", saved.len(), c.e.exact_text().len()),
                );
                remember(c);
                type_in_source(&c.e, "Ungespeichert");
                if let Some(p) = path(c) {
                    let _ = std::fs::write(p, "Fremder Text\n");
                }
            },
        ),
        after_a_look(|c| {
            check(
                "unsaved text outlives a change of its file by another program",
                source(&c.e).contains("Ungespeichert") && !source(&c.e).contains("Fremder Text"),
                &sizes(c),
            );
        }),
        applied(move |c| {
            if let Some(p) = path(c) {
                let _ = std::fs::write(p, before(c));
            }
        }),
        after_a_look(|c| {
            undo(c);
            check(
                "the unsaved text can be taken back",
                source(&c.e) == before(c),
                &sizes(c),
            );
        }),
        // Until the look at the file has taken over its date, AppKit counts it as changed by another program: a save asks
        // in a sheet, which ends the save if it is answered here, and an autosave fails without asking.
        when(
            "the look at the file to take its date",
            move |c| {
                end_sheet(c, NSAlertFirstButtonReturn);
                let known = c.e.document().and_then(|d| d.fileModificationDate());
                let on_disk = path(c).and_then(|p| {
                    NSFileManager::defaultManager()
                        .attributesOfItemAtPath_error(&ns(&p))
                        .ok()
                        .and_then(|attributes| attributes.fileModificationDate())
                });
                sheet(c).is_none()
                    && known
                        .zip(on_disk)
                        .is_some_and(|(known, on_disk)| known.isEqualToDate(&on_disk))
            },
            |_| {},
        ),
    ]
}

fn buttons_of(view: &objc2_app_kit::NSView, out: &mut Vec<String>) {
    for child in view.subviews().iter() {
        if let Some(button) = child.downcast_ref::<objc2_app_kit::NSButton>() {
            out.push(button.title().to_string());
        }
        buttons_of(&child, out);
    }
}

/// The titles of the buttons on the sheet of the window, from right to left.
fn sheet_buttons(c: &Ctx) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(view) = sheet(c).and_then(|s| s.contentView()) {
        buttons_of(&view, &mut out);
    }
    out
}

pub(super) fn another_version_on_disk_steps() -> Vec<Step> {
    const OUTSIDE: &str = "Von einem anderen Programm\n";
    let path = |c: &Ctx| {
        c.e.document()
            .and_then(|d| d.fileURL()?.path())
            .map(|p| PathBuf::from(p.to_string()))
    };
    let on_file = move |c: &Ctx| {
        path(c)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default()
    };
    let ask = |c: &Ctx| {
        if let Some(d) = c.e.document() {
            unsafe { d.saveDocument(None) };
        }
    };
    let typed_and_changed_elsewhere = move |c: &Ctx| {
        type_in_source(&c.e, "Ungespeichert");
        remember(c);
        let path = path(c).expect("a file");
        std::fs::write(&path, OUTSIDE).expect("a test file");
        ask(c);
    };
    let saved = |c: &Ctx| sheet(c).is_none() && c.e.document().is_some_and(|d| !d.isDocumentEdited());
    vec![
        applied(ask),
        when("the document to be saved", saved, typed_and_changed_elsewhere),
        when(
            "AppKit to ask about the other version",
            |c| sheet(c).is_some(),
            move |c| {
                check(
                    "a save over the file of another program asks first and leaves file and text alone",
                    on_file(c) == OUTSIDE && source(&c.e) == before(c),
                    &on_file(c),
                );
                check(
                    "the question offers to save anyway, to take the last version or to save elsewhere",
                    sheet_buttons(c).len() == 3,
                    &format!("{:?}", sheet_buttons(c)),
                );
                end_sheet(c, NSAlertFirstButtonReturn);
            },
        ),
        when("the text to be saved over the other version", saved, move |c| {
            check(
                "saving anyway writes the text over the other version",
                on_file(c) == before(c) && source(&c.e) == before(c),
                &sizes(c),
            );
            check(
                "saving anyway keeps no copy of the text",
                c.e.document().is_some_and(|d| !d.keeps_a_copy_of_its_text()),
                "",
            );
            typed_and_changed_elsewhere(c);
        }),
        when(
            "AppKit to ask about the other version again",
            |c| sheet(c).is_some(),
            |c| end_sheet(c, NSAlertSecondButtonReturn),
        ),
        when("the last version to be taken", saved, move |c| {
            check(
                "taking the last version shows what the other program wrote",
                on_file(c) == OUTSIDE && source(&c.e) == OUTSIDE,
                &sizes(c),
            );
            check(
                "taking the last version keeps no copy of the text",
                c.e.document().is_some_and(|d| !d.keeps_a_copy_of_its_text()),
                "",
            );
        }),
    ]
}

/// Keys, not `insertText`: only these merge into the undo step the view keeps open across a save, which no change count
/// then sees.
pub(super) fn typing_through_a_save_steps() -> Vec<Step> {
    let keys = |c: &Ctx, typed: &[&str]| {
        if let Some(window) = c.e.window() {
            window.makeFirstResponder(Some(c.e.source()));
            for key in typed {
                super::super::press_key(&window, key);
            }
        }
    };
    let saved_or_marked = |c: &Ctx| {
        let edited = c.e.document().is_some_and(|d| d.isDocumentEdited());
        let file = c.e.document().and_then(|d| d.fileURL()?.path()).map(|p| p.to_string());
        edited
            || file
                .and_then(|p| std::fs::read_to_string(p).ok())
                .is_some_and(|f| f == c.e.exact_text())
    };
    vec![
        applied(move |c| {
            remember(c);
            let end = c.e.source().string().length();
            c.e.source().setSelectedRange(NSRange::new(end, 0));
            keys(c, &["A", "B"]);
            if let Some(d) = c.e.document() {
                unsafe { d.saveDocument(None) };
            }
        }),
        when(
            "the typed text to be saved",
            |c| c.e.document().is_some_and(|d| !d.isDocumentEdited()),
            move |c| {
                keys(c, &["C"]);
                check(
                    "a key right after a save marks the document as edited",
                    c.e.document().is_some_and(|d| d.isDocumentEdited()),
                    "",
                );
                keys(c, &["D"]);
            },
        ),
        after_a_look(move |c| {
            check(
                "keys right after a save outlive the look at the file",
                source(&c.e) == format!("{}ABCD", before(c)),
                &super::super::tail(&source(&c.e), 20),
            );
            check(
                "keys right after a save can be undone",
                c.e.document()
                    .and_then(|d| d.undoManager())
                    .is_some_and(|u| u.canUndo()),
                "",
            );
            check(
                "keys right after a save are either marked or written",
                saved_or_marked(c),
                "",
            );
            let undo_manager = c.e.document().and_then(|d| d.undoManager());
            while source(&c.e) != before(c) && undo_manager.as_ref().is_some_and(|u| u.canUndo()) {
                undo(c);
            }
            check(
                "undoing reaches back across the save",
                source(&c.e) == before(c),
                &sizes(c),
            );
            let back = before(c);
            if source(&c.e) != back {
                c.e.updater().replace_all_text(&back);
            }
        }),
        // AppKit autosaves every 15 to 25 seconds; one that falls in here finds the file changed and asks in a sheet.
        pause(1.5, "an autosave gives no signal when it has asked", |c| {
            end_sheet(c, NSAlertFirstButtonReturn);
        }),
        // The question may also come only now, and a save behind it would wait for its answer in a modal loop.
        when(
            "the question of an autosave to close",
            |c| {
                end_sheet(c, NSAlertFirstButtonReturn);
                sheet(c).is_none()
            },
            |_| {},
        ),
    ]
}
