//! What reaches the disk: a save writes the bytes it should in the file's own format, one that fails changes nothing, and
//! only another program's change of a file rereads a document.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSSaveOperationType, NSStandardKeyBindingResponding};
use objc2_foundation::NSURL;

use super::controller::{next_turn, until};
use super::documents::{opened, restored, untitled, url_of};
use super::{check, type_in_source};
use crate::document::{MdDocument, reload_changed_documents};
use crate::editor::{EditorController, decoder};
use crate::test_hooks::{FAULT_AT, LOOKS_TAKEN, SAVES_ASKED_FOR};
use crate::util::ns;

/// Saves to `url` and reports whether that worked: `saveDocument` would ask in a window after a failure.
fn saved_to(document: &MdDocument, url: &NSURL) -> Option<bool> {
    let answer = Rc::new(Cell::new(None));
    let noted = answer.clone();
    let handler = crate::util::error_handler(move |error| noted.set(Some(error.is_none())));
    let kind = document.fileType().expect("a document type");
    document.saveToURL_ofType_forSaveOperation_completionHandler(
        url,
        &kind,
        NSSaveOperationType::SaveOperation,
        &handler,
    );
    until(|| answer.get().is_some());
    answer.get()
}

fn saved(document: &MdDocument) -> bool {
    let url = document.fileURL().expect("a file");
    saved_to(document, &url) == Some(true)
}

/// Saves as `operation` does and reports whether that worked.
fn saved_as(document: &MdDocument, url: &NSURL, operation: NSSaveOperationType) -> Option<bool> {
    let answer = Rc::new(Cell::new(None));
    let noted = answer.clone();
    let handler = crate::util::error_handler(move |error| noted.set(Some(error.is_none())));
    let kind = document.fileType().unwrap_or_else(|| ns("net.daringfireball.markdown"));
    document.saveToURL_ofType_forSaveOperation_completionHandler(url, &kind, operation, &handler);
    until(|| answer.get().is_some());
    answer.get()
}

/// Asks for looks at the files until one that started after this call has been taken in, so that what it left alone is
/// known. A look asked for earlier may answer first, and one on a busy volume turns the next away.
pub(in crate::editor::selftest) fn looked_at_files(mtm: MainThreadMarker) -> bool {
    let before = LOOKS_TAKEN.load(Ordering::SeqCst);
    until(|| {
        reload_changed_documents(mtm);
        LOOKS_TAKEN.load(Ordering::SeqCst) >= before + 2
    })
}

fn file_holds(path: &Path, wanted: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| text == wanted)
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("a mode");
}

fn at_end(e: &EditorController) {
    let end = e.source().string().length();
    e.source().setSelectedRange(objc2_foundation::NSRange::new(end, 0));
}

fn utf16(mark: [u8; 2], unit: fn(u16) -> [u8; 2], text: &str) -> Vec<u8> {
    let mut out = mark.to_vec();
    out.extend(text.encode_utf16().flat_map(unit));
    out
}

/// Opened and saved as they are, then with a line typed at the end, every format comes back byte for byte.
fn a_save_writes_the_format_the_file_came_in(mtm: MainThreadMarker, dir: &Path) {
    let le = |text: &str| utf16([0xFF, 0xFE], u16::to_le_bytes, text);
    let be = |text: &str| utf16([0xFE, 0xFF], u16::to_be_bytes, text);
    let cases: [(&str, Vec<u8>, Vec<u8>); 7] = [
        ("line feeds", b"a\nb\n".to_vec(), b"a\nb\nx\ny".to_vec()),
        (
            "Windows line ends",
            b"a\r\nb\r\n".to_vec(),
            b"a\r\nb\r\nx\r\ny".to_vec(),
        ),
        ("old Mac line ends", b"a\rb\r".to_vec(), b"a\rb\rx\ny".to_vec()),
        ("mixed line ends", b"a\r\nb\n".to_vec(), b"a\r\nb\nx\ny".to_vec()),
        (
            "a byte order mark",
            b"\xEF\xBB\xBFa\r\n".to_vec(),
            b"\xEF\xBB\xBFa\r\nx\r\ny".to_vec(),
        ),
        ("UTF-16 LE", le("ä\r\n"), le("ä\r\nx\r\ny")),
        ("UTF-16 BE", be("ä\n"), be("ä\nx\ny")),
    ];
    for (name, file, typed) in cases {
        let path = dir.join(format!("{name}.md"));
        std::fs::write(&path, &file).expect("a test file");
        let (document, e) = opened(mtm, &path);
        let untouched = saved(&document) && std::fs::read(&path).is_ok_and(|bytes| bytes == file);
        check(&format!("a save without a change keeps {name}"), untouched, "");
        at_end(&e);
        type_in_source(&e, "x\ny");
        let written = saved(&document);
        let bytes = std::fs::read(&path).unwrap_or_default();
        check(
            &format!("a save with a new line keeps {name}"),
            written && bytes == typed,
            &format!("{bytes:?} instead of {typed:?}"),
        );
        document.close();
    }
}

fn a_save_that_fails_changes_nothing(mtm: MainThreadMarker, dir: &Path) {
    let path = dir.join("Fehlschlag.md");
    std::fs::write(&path, "Alt\n").expect("a test file");
    let (document, e) = opened(mtm, &path);
    at_end(&e);
    type_in_source(&e, "Neu");
    let nowhere = url_of(&dir.join("gibt es nicht").join("Fehlschlag.md"));
    check(
        "a save to a folder that does not exist reports its failure",
        saved_to(&document, &nowhere) == Some(false),
        "",
    );
    check(
        "a failed save leaves the file as it was",
        std::fs::read_to_string(&path).is_ok_and(|text| text == "Alt\n"),
        "",
    );
    check(
        "a failed save leaves the text marked as unsaved",
        document.isDocumentEdited() && e.source_text() == "Alt\nNeu",
        &e.source_text(),
    );
    check(
        "a failed save keeps no copy of the text",
        !document.keeps_a_copy_of_its_text(),
        "",
    );
    check("a save after a failed one works", saved(&document), "");
    check(
        "a save after a failed one writes the text",
        std::fs::read_to_string(&path).is_ok_and(|text| text == "Alt\nNeu"),
        "",
    );
    document.close();
}

fn typing_goes_on_through_a_save(mtm: MainThreadMarker, dir: &Path) {
    let path = dir.join("Weitertippen.md");
    std::fs::write(&path, "Text").expect("a test file");
    let (document, e) = opened(mtm, &path);
    at_end(&e);
    type_in_source(&e, "A");
    check("what is typed marks the document", document.isDocumentEdited(), "");
    check("a save writes it", saved(&document), "");
    check("a save clears the mark", !document.isDocumentEdited(), "");
    type_in_source(&e, "B");
    check(
        "what is typed after a save marks the document at once",
        document.isDocumentEdited(),
        "",
    );
    let looked = |wanted: &str| looked_at_files(mtm) && e.source_text() == wanted;
    check(
        "a look at the file after a save leaves what was typed since",
        looked("TextAB"),
        &e.source_text(),
    );
    let undo = e.undo_manager().expect("an undo manager");
    check("a look at the file leaves the undo steps", undo.canUndo(), "");
    undo.undo();
    check(
        "undoing marks the document just when the text is not the one on file",
        document.isDocumentEdited() == (e.source_text() != "TextA"),
        &e.source_text(),
    );
    document.close();
}

/// Command-S can come at any moment of an autosave, and text can come between the two.
fn a_save_asked_for_during_an_autosave_writes_all_there_is(mtm: MainThreadMarker, dir: &Path) {
    let path = dir.join("Zusammenstoss.md");
    std::fs::write(&path, "Text").expect("a test file");
    let (document, e) = opened(mtm, &path);
    at_end(&e);
    let save = crate::util::item(mtm, "", Some(sel!(saveDocument:)), "");
    let waits_in_ms: [u64; 12] = [0, 0, 0, 0, 1, 2, 5, 10, 20, 50, 100, 200];
    let (mut under_way, mut unavailable) = (0, 0);
    let mut wrong = Vec::new();
    for (round, wait) in waits_in_ms.into_iter().enumerate() {
        type_in_source(&e, "x");
        let autosaved = Rc::new(Cell::new(false));
        let noted = autosaved.clone();
        let handler = crate::util::error_handler(move |_| noted.set(true));
        document.autosaveWithImplicitCancellability_completionHandler(true, &handler);
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(wait) {
            next_turn();
        }
        if round % 2 == 1 {
            type_in_source(&e, "y");
        }
        under_way += usize::from(!autosaved.get());
        unavailable += usize::from(!document.validateUserInterfaceItem(ProtocolObject::from_ref(&*save)));
        let begun = SAVES_ASKED_FOR.load(Ordering::Relaxed);
        // SAFETY: NSDocument's action method, called on the main thread without a sender.
        unsafe { document.saveDocument(None) };
        let ended = until(|| {
            SAVES_ASKED_FOR.load(Ordering::Relaxed) > begun && autosaved.get() && !document.isDocumentEdited()
        });
        let file = std::fs::read(&path).unwrap_or_default();
        if !ended || file != e.exact_text().into_bytes() {
            wrong.push(format!(
                "after {wait} ms: ended {ended}, {} bytes on file, {} in the view",
                file.len(),
                e.exact_text().len()
            ));
        }
    }
    check(
        "a save asked for during an autosave writes all there is",
        wrong.is_empty(),
        &wrong.join("; "),
    );
    check(
        "Save stays available during an autosave",
        unavailable == 0,
        &format!("{unavailable} times not"),
    );
    check(
        "those saves were asked for while an autosave was under way",
        under_way >= 4,
        &format!("{under_way} of {}", waits_in_ms.len()),
    );
    document.close();
}

/// A file whose date lies behind what the document knows is still another version: `cp -p` or a restore give it one.
fn another_program_decides_about_a_file_whatever_its_date(mtm: MainThreadMarker, dir: &Path) {
    let path = dir.join("Datum.md");
    std::fs::write(&path, "Neu\n").expect("a test file");
    let (document, e) = opened(mtm, &path);
    let shows = |wanted: &str| {
        let (e, wanted) = (e.clone(), wanted.to_string());
        until(move || {
            reload_changed_documents(mtm);
            e.source_text() == wanted
        })
    };
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(86_400);
    std::fs::write(&path, "Alt, von außen\n").expect("a test file");
    let file = std::fs::File::options().write(true).open(&path).expect("a test file");
    file.set_modified(long_ago).expect("a date");
    drop(file);
    check(
        "a file with an older date reloads",
        shows("Alt, von außen\n"),
        &e.source_text(),
    );
    type_in_source(&e, "x");
    check("the reloaded document can be saved", saved(&document), "");
    std::fs::write(&path, "Alt, von außen\n").expect("a test file");
    check(
        "an earlier version brought back by another program reloads",
        shows("Alt, von außen\n"),
        &e.source_text(),
    );
    document.close();
}

/// A read that fails is tried again, or the file's change would stay unnoticed until it changes once more.
fn a_read_that_fails_for_a_moment_is_tried_again(mtm: MainThreadMarker, dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("Gesperrt.md");
    std::fs::write(&path, "Vorher\n").expect("a test file");
    let (document, e) = opened(mtm, &path);
    let permissions = |mode| std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("a mode");
    std::fs::write(&path, "Nachher\n").expect("a test file");
    permissions(0o000);
    check(
        "a file that cannot be read for a moment keeps the text",
        looked_at_files(mtm) && e.source_text() == "Vorher\n",
        &e.source_text(),
    );
    permissions(0o644);
    let reloaded = until(|| {
        reload_changed_documents(mtm);
        e.source_text() == "Nachher\n"
    });
    check("a file that can be read again reloads", reloaded, &e.source_text());
    document.close();
}

/// A helper can die or stop answering because of what it decoded before, which the next picture must not notice.
fn a_helper_that_fails_is_replaced() {
    const BMP: [u8; 62] = {
        let mut b = [0u8; 62];
        (b[0], b[1], b[2]) = (b'B', b'M', 62);
        b[10] = 54;
        b[14] = 40;
        b[18] = 2;
        b[22] = 1;
        b[26] = 1;
        b[28] = 24;
        (b[56], b[57]) = (255, 255);
        b
    };
    let decoded = || decoder::decode(&BMP, 16).is_some();
    check("a helper decodes a picture", decoded(), "");
    decoder::end_helpers();
    check("a helper that died is replaced for the next picture", decoded(), "");
    decoder::stop_helpers();
    let started = std::time::Instant::now();
    let _ = decoded();
    check(
        "a helper that stopped answering ends at its time limit",
        started.elapsed() < decoder::LONGEST_WAIT,
        &format!("{:.1} s", started.elapsed().as_secs_f64()),
    );
    check(
        "the picture after a helper that stopped answering decodes",
        decoded(),
        "",
    );
    decoder::release_idle();
}

fn a_restored_draft_stays_marked(mtm: MainThreadMarker, dir: &Path) {
    let draft = dir.join("Entwurf.md");
    std::fs::write(&draft, "Entwurf").expect("a test file");
    let (document, e) = restored(mtm, None, &draft);
    check("a restored draft is marked", document.isDocumentEdited(), "");
    at_end(&e);
    type_in_source(&e, "x");
    let undo = e.undo_manager().expect("an undo manager");
    undo.undo();
    check(
        "undoing back to the text of a restored draft keeps the mark",
        e.source_text() == "Entwurf" && document.isDocumentEdited(),
        &e.source_text(),
    );
    type_in_source(&e, "y");
    at_end(&e);
    // SAFETY: The view's own action, which takes any sender and none as well.
    unsafe { e.source().deleteBackward(None) };
    check(
        "deleting back to the text of a restored draft keeps the mark",
        e.source_text() == "Entwurf" && document.isDocumentEdited(),
        &e.source_text(),
    );
    document.updateChangeCount(objc2_app_kit::NSDocumentChangeType::ChangeCleared);
    document.close();

    let file = dir.join("Benannt.md");
    std::fs::write(&file, "Auf der Platte").expect("a test file");
    std::fs::write(&draft, "Entwurf").expect("a test file");
    let (document, e) = restored(mtm, Some(&file), &draft);
    at_end(&e);
    type_in_source(&e, "x");
    e.undo_manager().expect("an undo manager").undo();
    check(
        "the draft of a named document stays marked until it is saved",
        e.source_text() == "Entwurf" && document.isDocumentEdited(),
        &e.source_text(),
    );
    check(
        "a look at its file leaves a draft alone",
        looked_at_files(mtm) && e.source_text() == "Entwurf",
        &e.source_text(),
    );
    check("a draft can be saved", saved(&document), "");
    check(
        "saving a draft writes it and clears its mark",
        !document.isDocumentEdited() && std::fs::read(&file).is_ok_and(|b| b == b"Entwurf"),
        "",
    );
    document.close();
}

/// A copy elsewhere leaves what the document's own file holds as it was, and a document whose editor is gone holds no text.
fn what_a_save_leaves_of_the_file(mtm: MainThreadMarker, dir: &Path) {
    let path = dir.join("Kopie.md");
    std::fs::write(&path, "Text").expect("a test file");
    let (document, e) = opened(mtm, &path);
    at_end(&e);
    type_in_source(&e, "A");
    let copy = url_of(&dir.join("Kopie 2.md"));
    let answer = Rc::new(Cell::new(None));
    let noted = answer.clone();
    let handler = crate::util::error_handler(move |error| noted.set(Some(error.is_none())));
    let kind = document.fileType().expect("a document type");
    document.saveToURL_ofType_forSaveOperation_completionHandler(
        &copy,
        &kind,
        NSSaveOperationType::SaveToOperation,
        &handler,
    );
    until(|| answer.get().is_some());
    check(
        "a copy elsewhere is written with the text",
        std::fs::read(dir.join("Kopie 2.md")).is_ok_and(|b| b == b"TextA"),
        "",
    );
    check(
        "a copy elsewhere leaves the document marked and its file alone",
        document.isDocumentEdited() && std::fs::read(&path).is_ok_and(|b| b == b"Text"),
        &e.source_text(),
    );
    e.undo_manager().expect("an undo manager").undo();
    check(
        "undoing marks a document after a copy just when the text is not the one on file",
        document.isDocumentEdited() == (e.source_text() != "Text"),
        &e.source_text(),
    );
    document.close();

    // As after its window closed: neither the editor nor the text it took over are there anymore.
    let without = MdDocument::without_text(mtm);
    check(
        "a document whose editor is gone cannot empty its file",
        saved_to(&without, &url_of(&path)) == Some(false) && std::fs::read(&path).is_ok_and(|b| b == b"Text"),
        "",
    );
    // Only once the run loop went on: until then its autorelease pool holds what AppKit handed out while closing.
    let editor = objc2::rc::Weak::from_retained(&e);
    drop(e);
    super::wait(
        0.0,
        6.0,
        1,
        move || editor.load().is_none(),
        |gone| check("a closed document lets go of its editor and all it holds", gone, ""),
    );
}

/// A folder that takes nothing is what a document's own file meets on a disk or share that became read-only: neither the
/// save the user asked for nor the autosave may lose anything, and the next save works once the folder takes files again.
fn a_folder_that_takes_nothing_costs_nothing(mtm: MainThreadMarker, dir: &Path) {
    let folder = dir.join("Nur lesen");
    std::fs::create_dir_all(&folder).expect("a test folder");
    let path = folder.join("Text.md");
    std::fs::write(&path, "Alt\n").expect("a test file");
    let (document, e) = opened(mtm, &path);
    at_end(&e);
    type_in_source(&e, "Neu");
    set_mode(&folder, 0o555);
    check(
        "a save into a folder that takes nothing reports its failure",
        !saved(&document),
        "",
    );
    let untouched = || file_holds(&path, "Alt\n") && e.source_text() == "Alt\nNeu" && document.isDocumentEdited();
    check(
        "a save that the folder refuses leaves file, text and mark as they were",
        untouched(),
        &e.source_text(),
    );
    let autosaved = Rc::new(Cell::new(false));
    let noted = autosaved.clone();
    let handler = crate::util::error_handler(move |_| noted.set(true));
    document.autosaveWithImplicitCancellability_completionHandler(false, &handler);
    until(|| autosaved.get());
    check(
        "an autosave that the folder refuses leaves file, text and mark as they were",
        autosaved.get() && untouched(),
        &e.source_text(),
    );
    set_mode(&folder, 0o755);
    check(
        "a save works once the folder takes files again",
        saved(&document) && file_holds(&path, "Alt\nNeu") && !document.isDocumentEdited(),
        "",
    );
    document.close();
}

/// A file whose own mode says read-only is a decision of the user, which a save must not go around.
fn a_read_only_file_stays_as_it_is(mtm: MainThreadMarker, dir: &Path) {
    let path = dir.join("Schreibgeschützt.md");
    std::fs::write(&path, "Alt\n").expect("a test file");
    let (document, e) = opened(mtm, &path);
    at_end(&e);
    type_in_source(&e, "Neu");
    set_mode(&path, 0o444);
    let answer = saved(&document);
    check(
        "a save over a read-only file fails and keeps the text unsaved",
        !answer && file_holds(&path, "Alt\n") && e.source_text() == "Alt\nNeu" && document.isDocumentEdited(),
        &format!("saved {answer}, on file {:?}", std::fs::read_to_string(&path)),
    );
    set_mode(&path, 0o644);
    check(
        "a save works once the file may be written",
        saved(&document) && file_holds(&path, "Alt\nNeu"),
        "",
    );
    document.close();
}

/// The folder of a document can go, with a share or by another program: the text stays, and it can go somewhere else.
fn a_folder_that_has_gone_costs_nothing(mtm: MainThreadMarker, dir: &Path) {
    let folder = dir.join("Verschwindet");
    std::fs::create_dir_all(&folder).expect("a test folder");
    let path = folder.join("Text.md");
    std::fs::write(&path, "Alt\n").expect("a test file");
    let (document, e) = opened(mtm, &path);
    at_end(&e);
    type_in_source(&e, "Neu");
    let url = url_of(&path);
    std::fs::remove_dir_all(&folder).expect("a test folder");
    check(
        "a look at a file whose folder has gone keeps the text",
        looked_at_files(mtm) && e.source_text() == "Alt\nNeu",
        &e.source_text(),
    );
    check(
        "a save into a folder that has gone reports its failure and keeps the text unsaved",
        saved_to(&document, &url) == Some(false) && e.source_text() == "Alt\nNeu" && document.isDocumentEdited(),
        &e.source_text(),
    );
    let elsewhere = dir.join("Gerettet.md");
    check(
        "the text of a folder that has gone can be saved elsewhere",
        saved_as(&document, &url_of(&elsewhere), NSSaveOperationType::SaveAsOperation) == Some(true)
            && file_holds(&elsewhere, "Alt\nNeu")
            && !document.isDocumentEdited(),
        "",
    );
    document.close();
}

/// A disk that runs full in the middle of a save, on a small disk image of its own.
fn a_full_disk_leaves_the_file_whole(mtm: MainThreadMarker, dir: &Path) {
    const NAME: &str = "a full disk leaves the file whole and the text unsaved";
    let hdiutil = |args: &[&std::ffi::OsStr]| {
        std::process::Command::new("/usr/bin/hdiutil")
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    let os = std::ffi::OsStr::new;
    let image = dir.join("Klein.dmg");
    let mount = dir.join("Klein");
    let made = hdiutil(&[
        os("create"),
        os("-size"),
        os("2m"),
        os("-fs"),
        os("HFS+"),
        os("-volname"),
        os("SplatProbe"),
        image.as_os_str(),
    ]) && hdiutil(&[
        os("attach"),
        image.as_os_str(),
        os("-nobrowse"),
        os("-noautoopen"),
        os("-mountpoint"),
        mount.as_os_str(),
    ]);
    if !made {
        super::skip(NAME, "no disk image could be made here");
        return;
    }
    let path = mount.join("Text.md");
    std::fs::write(&path, "Alt\n").expect("a test file");
    let (document, e) = opened(mtm, &path);
    let large = "x".repeat(8 << 20);
    at_end(&e);
    type_in_source(&e, &large);
    let answer = saved(&document);
    check(
        NAME,
        !answer
            && file_holds(&path, "Alt\n")
            && e.source_text().len() == 4 + large.len()
            && document.isDocumentEdited(),
        &format!("saved {answer}"),
    );
    e.updater().replace_all_text("Kurz");
    check(
        "a save works once the text fits on the disk",
        saved(&document) && file_holds(&path, "Kurz"),
        "",
    );
    document.close();
    hdiutil(&[os("detach"), mount.as_os_str(), os("-force")]);
}

/// Save As moves the document, and its old file stays what it was.
fn a_document_moves_with_save_as(mtm: MainThreadMarker, dir: &Path) {
    let (old, new) = (dir.join("Vorher.md"), dir.join("Nachher.md"));
    std::fs::write(&old, "Alt").expect("a test file");
    let (document, e) = opened(mtm, &old);
    at_end(&e);
    type_in_source(&e, "A");
    check(
        "Save As writes the text to its new file and clears the mark",
        saved_as(&document, &url_of(&new), NSSaveOperationType::SaveAsOperation) == Some(true)
            && file_holds(&new, "AltA")
            && !document.isDocumentEdited(),
        "",
    );
    check("Save As leaves the old file as it was", file_holds(&old, "Alt"), "");
    let at_new = document.fileURL().and_then(|u| u.path()).map(|p| p.to_string());
    check(
        "after Save As the document is its new file",
        at_new.as_deref() == new.to_str(),
        &format!("{at_new:?}"),
    );
    type_in_source(&e, "B");
    check(
        "the save after Save As goes to the new file",
        saved(&document) && file_holds(&new, "AltAB") && file_holds(&old, "Alt"),
        "",
    );
    document.close();
}

/// A new document without a file: its first save names one, and what follows goes there.
fn a_new_document_gets_its_first_file(mtm: MainThreadMarker, dir: &Path) {
    let (document, e) = untitled(mtm, "");
    type_in_source(&e, "Neu");
    let path = dir.join("Erste Datei.md");
    check(
        "the first save of a new document writes its text and clears the mark",
        saved_as(&document, &url_of(&path), NSSaveOperationType::SaveAsOperation) == Some(true)
            && file_holds(&path, "Neu")
            && !document.isDocumentEdited(),
        "",
    );
    at_end(&e);
    type_in_source(&e, "er");
    check(
        "the save after the first goes to the same file",
        saved(&document) && file_holds(&path, "Neuer"),
        "",
    );
    document.close();
}

/// A file moved by another program takes its document along: the next save must not bring the old file back.
fn a_file_moved_by_another_program_takes_the_document_along(mtm: MainThreadMarker, dir: &Path) {
    let (here, there) = (dir.join("Hier.md"), dir.join("Dort.md"));
    std::fs::write(&here, "Alt").expect("a test file");
    let (document, e) = opened(mtm, &here);
    at_end(&e);
    std::fs::rename(&here, &there).expect("a test file");
    let followed = until(|| {
        document
            .fileURL()
            .and_then(|u| u.path())
            .is_some_and(|p| p.to_string() == there.to_str().unwrap_or_default())
    });
    check(
        "a file moved by another program takes its document along",
        followed,
        &format!("{:?}", document.fileURL().and_then(|u| u.path())),
    );
    type_in_source(&e, "Neu");
    check(
        "the save after a move goes to where the file is now",
        saved(&document) && file_holds(&there, "AltNeu") && !here.exists(),
        "",
    );
    document.close();
}

/// What another program does to the file while its look is on the way: the keys that came meanwhile count.
fn keys_between_a_look_and_its_answer_stay(mtm: MainThreadMarker, dir: &Path) {
    let path = dir.join("Dazwischen.md");
    std::fs::write(&path, "Alt\n").expect("a test file");
    let (document, e) = opened(mtm, &path);
    at_end(&e);
    std::fs::write(&path, "Von außen\n").expect("a test file");
    reload_changed_documents(mtm);
    type_in_source(&e, "x");
    check(
        "what is typed while a look is on its way stays",
        looked_at_files(mtm) && e.source_text() == "Alt\nx" && document.isDocumentEdited(),
        &e.source_text(),
    );
    check(
        "a look that came too late leaves the other program's file alone",
        file_holds(&path, "Von außen\n"),
        "",
    );
    document.updateChangeCount(objc2_app_kit::NSDocumentChangeType::ChangeCleared);
    document.close();
}

/// A bug in any callback of a save may cost that save, never the text, and the next save writes all there is.
fn a_panic_in_a_save_costs_no_text(mtm: MainThreadMarker, dir: &Path) {
    let strike = |place| *FAULT_AT.lock().expect("the fault") = Some(place);
    for place in [
        "the start of a save",
        "the text a save takes",
        "the bytes a save writes",
        "the mark after a save",
        "the end of a save",
    ] {
        let path = dir.join(format!("{place}.md"));
        std::fs::write(&path, "Alt").expect("a test file");
        let (document, e) = opened(mtm, &path);
        at_end(&e);
        type_in_source(&e, "Neu");
        strike(place);
        let answer = saved_to(&document, &document.fileURL().expect("a file"));
        let held = FAULT_AT.lock().expect("the fault").take();
        check(
            &format!("a panic in {place} ends the save and keeps the text"),
            answer.is_some() && held.is_none() && e.source_text() == "AltNeu",
            &format!("{answer:?}, {held:?}, {}", e.source_text()),
        );
        check(
            &format!("a save after a panic in {place} writes all there is"),
            saved(&document) && file_holds(&path, "AltNeu") && !document.isDocumentEdited(),
            &format!("{:?}", std::fs::read_to_string(&path)),
        );
        document.close();
    }

    let path = dir.join("Neu laden.md");
    std::fs::write(&path, "Alt\n").expect("a test file");
    let (document, e) = opened(mtm, &path);
    std::fs::write(&path, "Von außen\n").expect("a test file");
    strike("a reload");
    let struck = until(|| {
        reload_changed_documents(mtm);
        FAULT_AT.lock().expect("the fault").is_none()
    });
    // The next look may have come back in the same turn, and reads the file then.
    check(
        "a panic in a reload leaves one whole text",
        struck && ["Alt\n", "Von außen\n"].contains(&e.source_text().as_str()),
        &e.source_text(),
    );
    let reloaded = until(|| {
        reload_changed_documents(mtm);
        e.source_text() == "Von außen\n"
    });
    check(
        "the look after a panic in a reload reads the file",
        reloaded,
        &e.source_text(),
    );
    at_end(&e);
    type_in_source(&e, "x");
    check(
        "what is typed after a panic in a reload marks the document",
        document.isDocumentEdited(),
        "",
    );
    check(
        "the save after a panic in a reload writes all there is",
        saved(&document) && file_holds(&path, "Von außen\nx"),
        "",
    );
    document.close();
}

pub(super) fn checks(mtm: MainThreadMarker) {
    let dir: PathBuf = std::env::temp_dir().join(format!("splat-probe-saving-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a test folder");
    a_save_writes_the_format_the_file_came_in(mtm, &dir);
    a_save_that_fails_changes_nothing(mtm, &dir);
    a_folder_that_takes_nothing_costs_nothing(mtm, &dir);
    a_read_only_file_stays_as_it_is(mtm, &dir);
    a_folder_that_has_gone_costs_nothing(mtm, &dir);
    a_full_disk_leaves_the_file_whole(mtm, &dir);
    a_document_moves_with_save_as(mtm, &dir);
    a_new_document_gets_its_first_file(mtm, &dir);
    a_file_moved_by_another_program_takes_the_document_along(mtm, &dir);
    keys_between_a_look_and_its_answer_stay(mtm, &dir);
    a_panic_in_a_save_costs_no_text(mtm, &dir);
    typing_goes_on_through_a_save(mtm, &dir);
    a_save_asked_for_during_an_autosave_writes_all_there_is(mtm, &dir);
    a_restored_draft_stays_marked(mtm, &dir);
    what_a_save_leaves_of_the_file(mtm, &dir);
    another_program_decides_about_a_file_whatever_its_date(mtm, &dir);
    a_read_that_fails_for_a_moment_is_tried_again(mtm, &dir);
    a_helper_that_fails_is_replaced();
    let _ = std::fs::remove_dir_all(&dir);
}
