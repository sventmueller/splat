//! Checks of what lies around the text: the word count, the marks for what changed since the last commit, the notes in
//! the sidebar, the metadata form, pasting formatted text, the export options and importing a Word document.

use std::path::{Path, PathBuf};
use std::process::Command;

use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSButton, NSDocumentController, NSModalResponseCancel, NSPasteboard, NSPasteboardTypeHTML, NSPasteboardTypeRTF,
    NSPasteboardTypeString, NSView, NSWindow,
};
use objc2_foundation::{NSData, NSPoint, NSRange};
use splat_git::diff::ChangeKind;

use super::super::actions::Action;
use super::super::export_options::ExportOptions;
use super::super::notes::NoteAnswer;
use super::super::panes::Side;
use super::super::sidebar::{NoteEvent, SidebarTab};
use super::{Ctx, Step, applied, check, editable_fields, perform, select_in_source, settle, skip, source, when};
use crate::document::MdDocument;
use crate::util::{constant, ns};

fn doc_dir(c: &Ctx) -> PathBuf {
    c.e.doc_dir().unwrap_or_else(|| c.out.clone())
}

fn sheet(c: &Ctx) -> Option<Retained<NSWindow>> {
    c.e.window().and_then(|w| w.attachedSheet())
}

fn set_text(c: &Ctx, text: &str) {
    c.e.replace_all_text(text);
    c.e.updater().update_now();
}

fn buttons(view: &NSView, out: &mut Vec<Retained<NSButton>>) {
    for sub in view.subviews().iter() {
        if let Ok(button) = sub.clone().downcast::<NSButton>() {
            out.push(button);
        }
        buttons(&sub, out);
    }
}

/// Clicks the button titled `title` on the sheet.
fn click(c: &Ctx, title: &str) -> bool {
    let Some(content) = sheet(c).and_then(|s| s.contentView()) else {
        return false;
    };
    let mut found = Vec::new();
    buttons(&content, &mut found);
    let button = found.into_iter().find(|b| b.title().to_string() == title);
    if let Some(button) = &button {
        // SAFETY: A click without a sender, as a button takes it.
        unsafe { button.performClick(None) };
    }
    button.is_some()
}

/// A picture of the window or, while one is open, of its sheet, to look at beside a run.
fn capture(c: &Ctx, name: &str) {
    let window = sheet(c).or_else(|| c.e.window());
    if let Some(window) = window {
        // On a locked screen no display cycle comes along to do this.
        window.layoutIfNeeded();
        super::window_image(&window, &c.out.join(format!("look-{name}.png")));
    }
}

fn fields_of_sheet(c: &Ctx) -> Vec<Retained<objc2_app_kit::NSTextField>> {
    let mut fields = Vec::new();
    if let Some(content) = sheet(c).and_then(|s| s.contentView()) {
        editable_fields(&content, &mut fields);
    }
    fields
}

pub(super) fn word_count_steps() -> Vec<Step> {
    let reads = |c: &Ctx, needle: &str| {
        c.e.word_counter()
            .shown_text()
            .is_some_and(|text| text.contains(needle))
    };
    vec![
        applied(|c| {
            set_text(
                c,
                "---\ntitle: nicht gezählt\n---\n\nEin **kleiner** Text mit [Link](https://x.de).\n",
            )
        }),
        when(
            "the count of the text to show",
            move |c| {
                reads(
                    c,
                    &crate::i18n::tf("Wörter: {} · Zeichen: {} · Lesezeit: {} Min.", &["5", "26", "1"]),
                )
            },
            |c| {
                capture(c, "word-count");
                check(
                    "the count leaves out the metadata and the link's address",
                    c.e.word_counter().shown_text().is_some_and(|text| !text.contains("11")),
                    &c.e.word_counter().shown_text().unwrap_or_default(),
                );
                check(
                    "the count stands in the rightmost pane",
                    c.e.word_counter().sits_in_the_rightmost_pane(),
                    "",
                );
                select_in_source(&c.e, "Ein **kleiner**");
            },
        ),
        when(
            "the count of the selection to show",
            move |c| {
                reads(
                    c,
                    &crate::i18n::tf("Auswahl – Wörter: {} von {} · Zeichen: {}", &["2", "5", "11"]),
                )
            },
            |c| {
                check(
                    "a selection is counted without its marks, beside the count of the whole text",
                    true,
                    "",
                );
                perform(&c.e, Action::ToggleWordCount.spec().selector, 0);
            },
        ),
        when(
            "the count to go away",
            |c| c.e.word_counter().shown_text().is_none(),
            |c| {
                check("the View menu hides the count", true, "");
                perform(&c.e, Action::ToggleWordCount.spec().selector, 0);
                c.e.panes().set_preview_collapsed(true);
            },
        ),
        when(
            "the preview to go away",
            |c| !c.e.panes().preview_visible(),
            |c| {
                c.e.word_counter().place();
                check(
                    "without the preview the count stands in the source",
                    c.e.word_counter().sits_in_the_rightmost_pane(),
                    "",
                );
                c.e.panes().set_preview_collapsed(false);
            },
        ),
        when(
            "the preview to come back",
            |c| c.e.panes().preview_visible(),
            |c| {
                let mut long: String = (1..=150).map(|n| format!("Absatz {n} mit etwas Text.\n\n")).collect();
                long.push_str(&"Die letzte Zeile reicht weit nach rechts und endet hier. ".repeat(6));
                set_text(c, &long);
            },
        ),
        when(
            "the long text to reach the preview",
            |c| {
                crate::util::storage(c.e.preview())
                    .string()
                    .to_string()
                    .trim_end()
                    .ends_with("endet hier.")
            },
            |c| {
                c.e.word_counter().place();
                let preview = c.e.preview();
                if let (Some(lm), Some(tc)) = (
                    crate::util::layout_manager(preview),
                    crate::util::text_container(preview),
                ) {
                    lm.ensureLayoutForTextContainer(&tc);
                }
                let height = preview.bounds().size.height;
                preview.scrollPoint(NSPoint::new(0.0, height));
                check(
                    "scrolled to its end, the text's last line stands above the count",
                    c.e.word_counter().end_stands_clear(preview),
                    &format!(
                        "{:?} of {height}, then {:?}",
                        preview.visibleRect(),
                        preview.bounds().size
                    ),
                );
                preview.scrollPoint(NSPoint::new(0.0, 0.0));
                let end = NSRange::new(crate::util::storage(preview).length(), 0);
                preview.setSelectedRange(end);
                preview.scrollRangeToVisible(end);
                check(
                    "the caret at the end brings the last line above the count",
                    c.e.word_counter().end_stands_clear(preview),
                    "",
                );
                preview.scrollPoint(NSPoint::new(0.0, 0.0));
            },
        ),
    ]
}

/// Commits `text` as the document's file without touching the file, which a document that is open would take for the work
/// of another program.
fn commit_text(dir: &Path, text: &str) -> bool {
    use std::io::Write;
    let Ok(mut hashing) = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
    else {
        return false;
    };
    if let Some(mut input) = hashing.stdin.take()
        && input.write_all(text.as_bytes()).is_err()
    {
        return false;
    }
    let Ok(output) = hashing.wait_with_output() else {
        return false;
    };
    let blob = String::from_utf8_lossy(&output.stdout).trim().to_string();
    git(
        dir,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{blob},QuickReference.md"),
        ],
    ) && git(dir, &["commit", "-qm", "x"])
}

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(["-c", "user.name=Splat", "-c", "user.email=splat@example.invalid"])
        .args(args)
        .current_dir(dir)
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Lines that differ from each other, as blank lines between them would leave a diff several ways to read a change.
const COMMITTED: &str = "# Titel\n- Erster\n- Zweiter\n- Dritter\n- Vierter\n";

pub(super) fn change_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            let dir = doc_dir(c);
            set_text(c, COMMITTED);
            if !(git(&dir, &["init", "-q"]) && commit_text(&dir, COMMITTED)) {
                skip(
                    "the marks for what changed",
                    "there is no git here to make a repository with",
                );
            }
            c.e.look_at_repository();
        }),
        settle(|c| {
            check(
                "a text as it was committed has no marks",
                c.e.change_marks().marks(Side::Source).is_empty(),
                "",
            );
            let changed = COMMITTED
                .replace("- Dritter", "- Dritter, geändert")
                .replace("- Erster\n", "");
            set_text(c, &format!("{changed}- Fünfter\n"));
        }),
        when(
            "the marks for what changed",
            |c| c.e.change_marks().marks(Side::Source).len() >= 3,
            |c| {
                capture(c, "changes");
                for (side, name) in [(Side::Source, "source"), (Side::Preview, "preview")] {
                    let drawn = c.e.change_marks().drawn(side);
                    let in_order = drawn.windows(2).all(|d| d[0].1.origin.y < d[1].1.origin.y);
                    let on_its_pane = drawn
                        .iter()
                        .all(|(_, rect)| rect.origin.x >= 0.0 && rect.size.height >= 2.0);
                    check(
                        &format!("the {name} draws each mark, from the top down"),
                        drawn.len() == 3 && in_order && on_its_pane,
                        &format!("{drawn:?}"),
                    );
                }
                let drawn = c.e.change_marks().drawn(Side::Preview);
                let changed = drawn.iter().find(|(kind, _)| *kind == ChangeKind::Modified);
                let added = drawn.iter().find(|(kind, _)| *kind == ChangeKind::Added);
                check(
                    "a changed line's bar ends above the line behind it in the preview",
                    changed.zip(added).is_some_and(|((_, a), (_, b))| {
                        let one_line = b.size.height;
                        a.size.height < 1.5 * one_line
                    }),
                    &format!("{drawn:?}"),
                );
                let source = c.e.change_marks().marks(Side::Source);
                let preview = c.e.change_marks().marks(Side::Preview);
                let kinds: Vec<ChangeKind> = source.iter().map(|m| m.kind).collect();
                check(
                    "a change, a removal and an addition each get their mark",
                    kinds.contains(&ChangeKind::Modified)
                        && kinds.contains(&ChangeKind::Removed)
                        && kinds.contains(&ChangeKind::Added),
                    &format!("{kinds:?}"),
                );
                check(
                    "both panes mark the same changes",
                    source.len() == preview.len() && preview.windows(2).all(|p| p[0].start <= p[1].start),
                    &format!("{source:?} {preview:?}"),
                );
                perform(&c.e, Action::ToggleChanges.spec().selector, 0);
            },
        ),
        when(
            "the marks to go away",
            |c| c.e.change_marks().marks(Side::Source).is_empty(),
            |c| {
                check("the View menu switches the marks off", true, "");
                perform(&c.e, Action::ToggleChanges.spec().selector, 0);
            },
        ),
        when(
            "the marks to come back",
            |c| !c.e.change_marks().marks(Side::Source).is_empty(),
            |c| {
                check("the new text is committed", commit_text(&doc_dir(c), &source(&c.e)), "");
                c.e.look_at_repository();
            },
        ),
        when(
            "the marks to end with a new commit",
            |c| {
                c.e.look_at_repository();
                c.e.change_marks().marks(Side::Source).is_empty()
            },
            |_| check("a new commit is the new base", true, ""),
        ),
    ]
}

fn write_notes(dir: &Path) {
    let sub = dir.join("Unterordner");
    std::fs::create_dir_all(&sub).expect("a test folder");
    std::fs::write(
        dir.join("Alpha.md"),
        "Alpha mit #pruefung und Äpfeln. Siehe [[Beta]].\n",
    )
    .expect("a note");
    std::fs::write(sub.join("Beta.md"), "Beta: ÄPFEL und `#keintag`.\n").expect("a note");
    std::fs::write(sub.join("Gamma.md"), "Gamma zeigt auf [Beta](Beta.md#kopf).\n").expect("a note");
    std::fs::write(dir.join("kein-markdown.txt"), "Äpfel\n").expect("a file");
}

fn open_notes(c: &Ctx) -> Vec<Retained<MdDocument>> {
    NSDocumentController::sharedDocumentController(c.e.mtm())
        .documents()
        .iter()
        .filter_map(|d| d.downcast::<MdDocument>().ok())
        .collect()
}

fn note_is_open(c: &Ctx, name: &str) -> bool {
    open_notes(c)
        .iter()
        .any(|d| d.displayName().to_string().starts_with(name))
}

pub(super) fn note_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            write_notes(&doc_dir(c));
            // The folder of the test is a work tree of its own: the one it lies in leaves out `target`.
            std::fs::create_dir_all(doc_dir(c).join(".git")).expect("a folder");
            perform(&c.e, Action::ToggleFiles.spec().selector, 0);
        }),
        when(
            "the folder's notes",
            |c| c.e.notes().tree_paths().iter().any(|p| p.ends_with("Beta.md")),
            |c| {
                let paths = c.e.notes().tree_paths();
                check(
                    "the files show the notes of the folder and no other file",
                    paths.iter().any(|p| p.ends_with("Alpha.md"))
                        && paths.iter().any(|p| p.ends_with("Unterordner/Gamma.md"))
                        && !paths.iter().any(|p| p.ends_with("kein-markdown.txt")),
                    &format!("{paths:?}"),
                );
            },
        ),
        when(
            "the sidebar to stand",
            |c| c.e.panes().tab_visible(SidebarTab::Files),
            |_| {},
        ),
        settle(|c| {
            capture(c, "files");
            // The outline, older and known to look right, as a control for what a picture drawn by the app shows of a selection.
            c.e.note_event(NoteEvent::TabChosen(SidebarTab::Outline));
            c.e.panes().with_outline(|view| {
                let rows = objc2_foundation::NSIndexSet::indexSetWithIndex(1);
                view.selectRowIndexes_byExtendingSelection(&rows, false);
            });
            capture(c, "outline-selected");
            c.e.note_event(NoteEvent::TabChosen(SidebarTab::Files));
            let left =
                c.e.source()
                    .enclosingScrollView()
                    .map_or(0.0, |scroll| scroll.convertPoint_toView(NSPoint::new(0.0, 0.0), None).x);
            check(
                "the sidebar takes its room beside the source",
                left >= 160.0,
                &format!("{left}"),
            );
            check(
                "the word count stands in the corner of its smaller pane",
                c.e.word_counter().stands_in_its_corner(),
                "",
            );
            c.e.note_event(NoteEvent::TabChosen(SidebarTab::Search));
            c.e.note_event(NoteEvent::Search("äpfel".to_string()));
        }),
        when(
            "the search through the folder",
            |c| !c.e.notes().is_searching() && !c.e.notes().hit_paths().is_empty(),
            |c| {
                capture(c, "search");
                let paths = c.e.notes().hit_paths();
                check(
                    "a search finds every case and only notes",
                    paths.iter().any(|p| p.ends_with("Alpha.md"))
                        && paths.iter().any(|p| p.ends_with("Beta.md"))
                        && !paths.iter().any(|p| p.ends_with("kein-markdown.txt")),
                    &format!("{paths:?}"),
                );
                c.e.note_event(NoteEvent::Search("#pruefung".to_string()));
            },
        ),
        when(
            "the search for a tag",
            |c| !c.e.notes().is_searching() && c.e.notes().hit_paths().len() == 1,
            |c| {
                check(
                    "a tag is found where it is one",
                    c.e.notes().hit_paths().first().is_some_and(|p| p.ends_with("Alpha.md")),
                    "",
                );
                let beta = doc_dir(c).join("Unterordner/Beta.md");
                c.e.note_event(NoteEvent::LinksTo(beta));
            },
        ),
        when(
            "the notes that link to one",
            |c| !c.e.notes().is_searching() && c.e.notes().hit_paths().len() == 2,
            |c| {
                let paths = c.e.notes().hit_paths();
                check(
                    "the links to a note are found by wiki link and by address",
                    paths.iter().any(|p| p.ends_with("Alpha.md")) && paths.iter().any(|p| p.ends_with("Gamma.md")),
                    &format!("{paths:?}"),
                );
                c.e.note_event(NoteEvent::OpenHit(doc_dir(c).join("Alpha.md"), 0, 6));
            },
        ),
        when(
            "the note to open in a tab",
            |c| note_is_open(c, "Alpha"),
            |c| {
                let tabs = c.e.window().and_then(|w| w.tabbedWindows()).map_or(0, |t| t.count());
                check(
                    "a note opens as a tab of the window",
                    tabs >= 2,
                    &format!("{tabs} tabs"),
                );
                c.e.note_rename_chosen(
                    doc_dir(c).join("Unterordner/Beta.md"),
                    doc_dir(c).join("Unterordner/Neu.md"),
                );
            },
        ),
        when(
            SHEET_ASKS,
            |c| sheet(c).is_some(),
            |c| {
                let info = sheet(c).is_some();
                check("a rename that reaches other files asks first", info, "");
                check(
                    "the rename can be confirmed",
                    click(c, crate::i18n::t("Umbenennen")),
                    "",
                );
            },
        ),
        when(
            "the rename to be done",
            |c| doc_dir(c).join("Unterordner/Neu.md").exists() && sheet(c).is_none(),
            |c| {
                let gamma = std::fs::read_to_string(doc_dir(c).join("Unterordner/Gamma.md")).unwrap_or_default();
                check(
                    "a link in a closed file follows the rename, and nothing else in it changes",
                    gamma == "Gamma zeigt auf [Beta](Neu.md#kopf).\n",
                    &gamma,
                );
                let alpha = open_notes(c)
                    .iter()
                    .find(|d| d.displayName().to_string().starts_with("Alpha"))
                    .and_then(|d| d.editor())
                    .map(|e| source(&e))
                    .unwrap_or_default();
                check(
                    "a link in an open document follows the rename",
                    alpha.contains("[[Neu]]"),
                    &alpha,
                );
                c.e.note_event(NoteEvent::Rename(doc_dir(c).join("Alpha.md")));
            },
        ),
        when(
            "the sheet asking for a new name",
            |c| sheet(c).is_some(),
            |c| {
                let fields = fields_of_sheet(c);
                check(
                    "the sheet proposes the name without its extension",
                    fields.first().is_some_and(|f| f.stringValue().to_string() == "Alpha"),
                    "",
                );
                if let Some(field) = fields.first() {
                    field.setStringValue(&ns("Erste"));
                }
                check("the new name can be taken", click(c, crate::i18n::t("Umbenennen")), "");
            },
        ),
        when(
            "the open note to follow its new name",
            |c| doc_dir(c).join("Erste.md").exists() && note_is_open(c, "Erste") && sheet(c).is_none(),
            |c| {
                check(
                    "a note that is open moves with its document",
                    !doc_dir(c).join("Alpha.md").exists(),
                    "",
                );
                c.e.note_rename_chosen(doc_dir(c).join("Erste.md"), doc_dir(c).join("Unterordner/Gamma.md"));
            },
        ),
        when(
            "the alert about a name that is taken",
            |c| sheet(c).is_some(),
            |c| {
                check(
                    "a name that is taken is refused",
                    doc_dir(c).join("Erste.md").exists(),
                    "",
                );
                if let (Some(window), Some(alert)) = (c.e.window(), sheet(c)) {
                    window.endSheet_returnCode(&alert, objc2_app_kit::NSAlertFirstButtonReturn);
                }
            },
        ),
        when(
            "the alert to close",
            |c| sheet(c).is_none(),
            |c| {
                c.e.note_answer(NoteAnswer::Renamed {
                    failed: vec![doc_dir(c).join("Erste.md")],
                });
            },
        ),
        when(
            "the alert about what stayed",
            |c| sheet(c).is_some(),
            |c| {
                check("a rename that stopped says what stayed", true, "");
                if let (Some(window), Some(alert)) = (c.e.window(), sheet(c)) {
                    window.endSheet_returnCode(&alert, objc2_app_kit::NSAlertFirstButtonReturn);
                }
                c.e.note_event(NoteEvent::ChooseFolder);
            },
        ),
        when(
            "the panel for another folder",
            |c| sheet(c).is_some(),
            |c| {
                if let (Some(window), Some(panel)) = (c.e.window(), sheet(c)) {
                    window.endSheet_returnCode(&panel, NSModalResponseCancel);
                }
                c.e.use_notes_folder(doc_dir(c).join("Unterordner"));
            },
        ),
        when(
            "the other folder's notes",
            |c| {
                let paths = c.e.notes().tree_paths();
                !paths.is_empty() && paths.iter().all(|p| p.starts_with(doc_dir(c).join("Unterordner")))
            },
            |c| {
                c.e.note_event(NoteEvent::TabChosen(SidebarTab::Files));
                let (menu, clicked) =
                    c.e.panes()
                        .with_files(|files| (files.menu_of("Gamma.md"), files.click("Gamma.md")));
                check(
                    "a note's context menu has its three commands",
                    menu.len() == 3,
                    &format!("{menu:?}"),
                );
                check("a click on a note in the list opens it", clicked, "");
            },
        ),
        when(
            "the clicked note to open",
            |c| note_is_open(c, "Gamma"),
            |c| {
                for document in open_notes(c) {
                    let name = document.displayName().to_string();
                    if name.starts_with("Erste") || name.starts_with("Gamma") {
                        document.updateChangeCount(objc2_app_kit::NSDocumentChangeType::ChangeCleared);
                        document.close();
                    }
                }
                c.e.use_notes_folder(doc_dir(c));
            },
        ),
        when(
            "the whole folder again",
            |c| c.e.notes().tree_paths().iter().any(|p| p.ends_with("Erste.md")),
            |c| {
                let (rows, clicked) = c.e.panes().with_files(|files| {
                    let before = files.shown_rows();
                    let clicked = files.click("Unterordner");
                    let open = files.shown_rows();
                    files.click("Unterordner");
                    (before != open && files.shown_rows() == before, clicked)
                });
                check("a click on a folder opens and closes it", rows && clicked, "");
                c.e.note_event(NoteEvent::Search(String::new()));
                c.e.note_event(NoteEvent::TabChosen(SidebarTab::Outline));
            },
        ),
        settle(|c| {
            check(
                "the outline comes back as a tab",
                c.e.panes().sidebar_tab() == Some(SidebarTab::Outline),
                "",
            );
            perform(&c.e, Action::SearchFolder.spec().selector, 0);
            check(
                "the search command shows the search",
                c.e.panes().sidebar_tab() == Some(SidebarTab::Search),
                "",
            );
            perform(&c.e, Action::ToggleOutline.spec().selector, 0);
            check(
                "the outline command switches to the outline",
                c.e.panes().sidebar_tab() == Some(SidebarTab::Outline),
                "",
            );
            perform(&c.e, Action::ToggleOutline.spec().selector, 0);
        }),
    ]
}

const SHEET_ASKS: &str = "the sheet asking about the rename";

pub(super) fn metadata_steps() -> Vec<Step> {
    const FRONT: &str = "---\n# ein Kommentar\ntitle: Alt\ntags: [a, b]\nnested:\n  x: 1\n---\n\nText\n";
    vec![
        applied(|c| {
            set_text(c, FRONT);
            perform(&c.e, Action::EditMetadata.spec().selector, 0);
        }),
        when(
            "the metadata form",
            |c| sheet(c).is_some(),
            |c| {
                capture(c, "metadata");
                let fields = fields_of_sheet(c);
                let shown: Vec<String> = fields.iter().map(|f| f.stringValue().to_string()).collect();
                check(
                    "the form shows the fields it can change",
                    shown.contains(&"title".to_string())
                        && shown.contains(&"Alt".to_string())
                        && shown.contains(&"a, b".to_string()),
                    &format!("{shown:?}"),
                );
                if let Some(title) = fields.iter().find(|f| f.stringValue().to_string() == "Alt") {
                    title.setStringValue(&ns("Neu"));
                }
                check("a field can be added", click(c, crate::i18n::t("Feld hinzufügen")), "");
                let fields = fields_of_sheet(c);
                let empty: Vec<_> = fields.iter().filter(|f| f.stringValue().length() == 0).collect();
                if let [key, value] = empty.as_slice() {
                    key.setStringValue(&ns("date"));
                    value.setStringValue(&ns("2026-11-05"));
                }
                check("the form commits", click(c, crate::i18n::t("Übernehmen")), "");
            },
        ),
        when(
            "the form to close",
            |c| sheet(c).is_none(),
            |c| {
                check(
                    "only the changed field changes, and the new one stands behind the others",
                    source(&c.e)
                        == "---\n# ein Kommentar\ntitle: Neu\ntags: [a, b]\nnested:\n  x: 1\ndate: 2026-11-05\n---\n\nText\n",
                    &source(&c.e),
                );
                if let Some(undo) = c.e.document().and_then(|d| d.undoManager()) {
                    undo.undo();
                }
                check(
                    "the form's change is one undo step",
                    source(&c.e) == FRONT,
                    &source(&c.e),
                );
                perform(&c.e, Action::EditMetadata.spec().selector, 0);
            },
        ),
        when(
            "the metadata form again",
            |c| sheet(c).is_some(),
            |c| {
                if let (Some(window), Some(panel)) = (c.e.window(), sheet(c)) {
                    window.endSheet_returnCode(&panel, NSModalResponseCancel);
                }
            },
        ),
        when(
            "the form to close without a change",
            |c| sheet(c).is_none(),
            |c| {
                check("cancelling the form leaves the text", source(&c.e) == FRONT, "");
                let link = super::preview_link(&c.e, "splat-metadata");
                check("the metadata line is a link", link.is_some(), "");
                // As AppKit hands it over: on a window in the background, a real click would only bring it to the front.
                if let Some(at) = link {
                    c.e.links().link_clicked(c.e.preview(), &ns("splat-metadata"), at);
                }
            },
        ),
        when(
            "the form from a click on the metadata line",
            |c| sheet(c).is_some(),
            |c| {
                check("a click on the metadata line opens the form", true, "");
                if let (Some(window), Some(panel)) = (c.e.window(), sheet(c)) {
                    window.endSheet_returnCode(&panel, NSModalResponseCancel);
                }
            },
        ),
        when(
            "the form to close again",
            |c| sheet(c).is_none(),
            |c| {
                set_text(c, "Ohne Metadaten\n");
                c.e.metadata_chosen(vec![(
                    "title".to_string(),
                    crate::markdown::front_matter::Value::Text("Erster".to_string()),
                )]);
                check(
                    "a document without metadata gets a block at its start",
                    source(&c.e).starts_with("---\ntitle: Erster\n---\n\n"),
                    &source(&c.e),
                );
            },
        ),
    ]
}

fn board_with(setup: impl FnOnce(&NSPasteboard)) -> Retained<NSPasteboard> {
    let board = NSPasteboard::pasteboardWithUniqueName();
    board.clearContents();
    setup(&board);
    board
}

pub(super) fn paste_markdown_steps() -> Vec<Step> {
    vec![
        applied(|c| set_text(c, "Davor\n\nDanach\n")),
        settle(|c| {
            select_in_source(&c.e, "Davor");
            let board = board_with(|board| {
                let html = "<p>Ein <b>fetter</b> und <a href=\"https://example.org/\">verlinkter</a> Text</p><ul><li>Punkt</li></ul>";
                board.setData_forType(
                    Some(&NSData::with_bytes(html.as_bytes())),
                    constant!(NSPasteboardTypeHTML),
                );
                board.setString_forType(&ns("flach"), constant!(NSPasteboardTypeString));
            });
            c.e.editing().paste_as_markdown(&board);
            let text = source(&c.e);
            check(
                "formatted text arrives as Markdown",
                text.contains("Ein **fetter** und [verlinkter](https://example.org/) Text") && text.contains("- Punkt"),
                &text,
            );
            check("what is selected is not lost", text.contains("Davor"), &text);
            if let Some(undo) = c.e.document().and_then(|d| d.undoManager()) {
                undo.undo();
            }
            check(
                "pasting Markdown is one undo step",
                source(&c.e) == "Davor\n\nDanach\n",
                &source(&c.e),
            );

            select_in_source(&c.e, "Danach");
            let board = board_with(|board| {
                let rtf = br"{\rtf1\ansi Ein {\i kursives} Wort}";
                board.setData_forType(Some(&NSData::with_bytes(rtf)), constant!(NSPasteboardTypeRTF));
            });
            c.e.editing().paste_as_markdown(&board);
            check(
                "RTF arrives as Markdown too, inside the line",
                source(&c.e).contains("Ein *kursives* Wort"),
                &source(&c.e),
            );

            set_text(c, "");
            let board = board_with(|board| {
                board.setString_forType(&ns("A\tB\n1\t2\n"), constant!(NSPasteboardTypeString));
            });
            c.e.editing().paste_as_markdown(&board);
            check(
                "cells from a spreadsheet become a table",
                source(&c.e).contains("| A | B |") || source(&c.e).contains("| A "),
                &source(&c.e),
            );

            set_text(c, "");
            let png =
                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M/AAwAD/wG+tKvs0gAAAABJRU5ErkJggg==";
            let board = board_with(|board| {
                let html = format!("<img src=\"data:image/png;base64,{png}\" alt=\"Bild\">");
                board.setData_forType(
                    Some(&NSData::with_bytes(html.as_bytes())),
                    constant!(NSPasteboardTypeHTML),
                );
            });
            c.e.editing().paste_as_markdown(&board);
            let text = source(&c.e);
            let written = std::fs::read_dir(doc_dir(c))
                .into_iter()
                .flatten()
                .flatten()
                .filter(|f| {
                    f.file_name().to_string_lossy().ends_with(".png")
                        && f.file_name().to_string_lossy().starts_with(crate::i18n::t("Bild"))
                })
                .count();
            check(
                "a picture inside pasted HTML becomes a file beside the document",
                text.contains("![Bild](") && written >= 1,
                &format!("{text} {written}"),
            );

            let before = source(&c.e);
            let board = board_with(|_| {});
            c.e.editing().paste_as_markdown(&board);
            check("an empty pasteboard changes nothing", source(&c.e) == before, "");
        }),
    ]
}

/// The text of a PDF by poppler's tool, which only the developer's Mac has.
fn pdf_text(pdf: &Path) -> Option<String> {
    let tool = ["/opt/homebrew/bin/pdftotext", "/usr/local/bin/pdftotext"]
        .into_iter()
        .find(|tool| Path::new(tool).exists())?;
    let output = Command::new(tool).arg(pdf).arg("-").output().ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(super) fn export_option_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            set_text(c, "---\ntitle: Der Titel\nauthor: Die Autorin\n---\n\nDer Text.\n");
            ExportOptions {
                page_numbers: true,
                header: "Kopf von {author}".to_string(),
                footer: String::new(),
                title_page: true,
                word_template: None,
            }
            .save();
            let pdf = doc_dir(c).join("Optionen.pdf");
            super::export_pdf(&c.e, &pdf);
        }),
        when(
            "the PDF with its options",
            |c| doc_dir(c).join("Optionen.pdf").exists(),
            |c| {
                let pdf = doc_dir(c).join("Optionen.pdf");
                let _ = std::fs::copy(&pdf, c.out.join("look-options.pdf"));
                match pdf_text(&pdf) {
                    None => skip("the pages of a PDF with options", "pdftotext is not installed"),
                    Some(text) => {
                        let pages: Vec<&str> = text.split('\u{c}').collect();
                        check(
                            "the title page stands alone, with the metadata and without a header",
                            pages[0].contains("Der Titel")
                                && pages[0].contains("Die Autorin")
                                && !pages[0].contains("Kopf von")
                                && !pages[0].contains("Der Text.")
                                && pages
                                    .get(1)
                                    .is_some_and(|page| page.matches("Die Autorin").count() == 1),
                            &text,
                        );
                        check(
                            "the next page has the header, the text and its number",
                            pages.get(1).is_some_and(|page| {
                                let number = page.replace(' ', "");
                                page.contains("Kopf von Die Autorin")
                                    && page.contains("Der Text.")
                                    && number.contains("2/2")
                            }),
                            &text,
                        );
                    }
                }
                perform(&c.e, Action::ExportOptions.spec().selector, 0);
            },
        ),
        when(
            "the sheet with the export options",
            |c| sheet(c).is_some(),
            |c| {
                capture(c, "export-options");
                let fields = fields_of_sheet(c);
                check(
                    "the sheet shows what is set",
                    fields
                        .iter()
                        .any(|f| f.stringValue().to_string() == "Kopf von {author}"),
                    "",
                );
                if let Some(header) = fields
                    .iter()
                    .find(|f| f.stringValue().to_string() == "Kopf von {author}")
                {
                    header.setStringValue(&ns("Geändert"));
                }
                check(
                    "a template can be taken away",
                    click(c, crate::i18n::t("Entfernen")),
                    "",
                );
                check(
                    "the options can be taken over",
                    click(c, crate::i18n::t("Übernehmen")),
                    "",
                );
            },
        ),
        when(
            "the sheet to close",
            |c| sheet(c).is_none(),
            |c| {
                check(
                    "what the sheet holds is kept",
                    ExportOptions::load().header == "Geändert",
                    &ExportOptions::load().header,
                );
                let template = doc_dir(c).join("Vorlage.docx");
                let plain = c.e.export_source().word("Vorlage").unwrap_or_default();
                let _ = std::fs::write(&template, plain);
                ExportOptions {
                    title_page: true,
                    word_template: Some(template.clone()),
                    ..ExportOptions::default()
                }
                .save();
                let with_template = c.e.export_source().word_file("Bericht");
                check(
                    "a Word export takes the look of a template",
                    with_template.as_ref().is_ok_and(|bytes| {
                        splat_import::docx::import(bytes, "x").is_ok_and(|i| i.markdown.contains("Der Text."))
                    }),
                    "",
                );
                let _ = std::fs::write(&template, "keine Vorlage");
                check(
                    "a broken template says so instead of exporting without it",
                    c.e.export_source().word_file("Bericht").is_err(),
                    "",
                );
                ExportOptions {
                    word_template: Some(doc_dir(c).join("gibt-es-nicht.docx")),
                    ..ExportOptions::default()
                }
                .save();
                let failure = c.e.export_source().word_file("Bericht");
                if let Err(reason) = &failure {
                    c.e.report_export_failure(reason);
                }
                check("a missing template says so", failure.is_err(), "");
                ExportOptions::default().save();
            },
        ),
        when(
            "the alert about the template",
            |c| sheet(c).is_some(),
            |c| {
                if let (Some(window), Some(alert)) = (c.e.window(), sheet(c)) {
                    window.endSheet_returnCode(&alert, objc2_app_kit::NSAlertFirstButtonReturn);
                }
            },
        ),
    ]
}

pub(super) fn word_import_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            set_text(
                c,
                "# Kopf\n\nEin **fetter** Absatz mit Fußnote[^1].\n\n- Eins\n- Zwei\n\n[^1]: Die Notiz.\n",
            );
            let word = doc_dir(c).join("Import.docx");
            let bytes = c.e.export_source().word("Import").unwrap_or_default();
            let _ = std::fs::write(&word, bytes);
            let target = doc_dir(c).join("Importiert.md");
            let result = crate::word_import::import_file(&word, &target);
            check(
                "a Word document becomes a Markdown file",
                result.is_ok(),
                &format!("{result:?}"),
            );
            let text = std::fs::read_to_string(&target).unwrap_or_default();
            check(
                "its headings, marks, lists and footnotes come along",
                text.contains("# Kopf")
                    && text.contains("**fetter**")
                    && text.contains("- Eins")
                    && text.contains("Die Notiz."),
                &text,
            );
            let junk = doc_dir(c).join("Kein.docx");
            let _ = std::fs::write(&junk, "kein Word");
            check(
                "what is no Word file leaves nothing behind",
                crate::word_import::import_file(&junk, &doc_dir(c).join("Nichts.md")).is_err()
                    && !doc_dir(c).join("Nichts.md").exists(),
                "",
            );
            let mtm = c.e.mtm();
            let choosing = crate::word_import::ask(mtm);
            // SAFETY: Cancel accepts a missing sender.
            unsafe { choosing.cancel(None) };
            let saving = crate::word_import::ask_where(mtm, word.clone());
            check(
                "the Markdown file is proposed under the name of the Word document",
                saving.nameFieldStringValue().to_string() == "Import.md",
                &saving.nameFieldStringValue().to_string(),
            );
            // SAFETY: Cancel accepts a missing sender.
            unsafe { saving.cancel(None) };
            crate::word_import::import_and_open(word, doc_dir(c).join("Geoeffnet.md"));
        }),
        when(
            "the imported document to open",
            |c| note_is_open(c, "Geoeffnet"),
            |c| {
                for document in open_notes(c) {
                    if document.displayName().to_string().starts_with("Geoeffnet") {
                        document.close();
                    }
                }
                crate::word_import::import_and_open(doc_dir(c).join("Kein.docx"), doc_dir(c).join("Nichts.md"));
            },
        ),
        when(
            "the alert about a failed import",
            |c| sheet(c).is_some(),
            |c| {
                check("a failed import says so", !doc_dir(c).join("Nichts.md").exists(), "");
                if let (Some(window), Some(alert)) = (c.e.window(), sheet(c)) {
                    window.endSheet_returnCode(&alert, objc2_app_kit::NSAlertFirstButtonReturn);
                }
            },
        ),
    ]
}
