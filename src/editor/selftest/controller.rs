//! The controller under real text input, without a window on screen: what goes wrong between a composition, the renderer,
//! the undo history and an open formula editor shows here, wherever `make test` runs.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use objc2::AnyThread;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBezierPath, NSBitmapImageRep, NSColor, NSDeviceRGBColorSpace, NSGraphicsContext, NSTextInputClient, NSTextView,
};
use objc2_foundation::{NSDate, NSNotFound, NSPoint, NSRange, NSRect, NSRunLoop, NSSize, NSURL};

use super::scenarios::{self, COMMANDS, SECTION, TABLE_COMMANDS};
use super::source;
use super::{after, check, compose, guarded, type_in_preview, type_in_source};
use crate::document::MdDocument;
use crate::editor::actions::Action;
use crate::editor::{EditorController, Invalidate, js};
use crate::markdown::text::Byte;
use crate::util::ns;

thread_local! {
    static KEEP_EDITOR: Cell<bool> = const { Cell::new(false) };
}

/// A popover cannot show without a window on screen; these checks are about what its editor does to the text.
pub(crate) fn keeps_unshown_editor() -> bool {
    KEEP_EDITOR.get()
}

fn open(mtm: MainThreadMarker, text: &str) -> (Retained<MdDocument>, Retained<EditorController>) {
    let (document, editor) = super::documents::untitled(mtm, text);
    if let Some(undo) = editor.undo_manager() {
        undo.setGroupsByEvent(false);
    }
    (document, editor)
}

/// One step of the undo history.
fn step(e: &EditorController, f: impl FnOnce()) {
    let undo = e.undo_manager().expect("an undo manager");
    undo.beginUndoGrouping();
    f();
    undo.endUndoGrouping();
}

/// Lets what waits for the next turn of the run loop happen; a turn that lays out a new window outlasts any fixed time,
/// so this waits for a timer that fires behind whatever is due already.
pub(super) fn next_turn() {
    let turned = Rc::new(Cell::new(false));
    let seen = turned.clone();
    after(0.0, move || seen.set(true));
    while !turned.get() {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.05));
    }
}

fn preview_offset_behind(e: &EditorController, what: &str) -> usize {
    let shown = e.preview().string().to_string();
    let at = shown.find(what).expect("shown in the preview") + what.len();
    shown[..at].encode_utf16().count()
}

fn caret_in_preview(e: &EditorController, at: usize) {
    e.window().expect("a window").makeFirstResponder(Some(e.preview()));
    e.preview().setSelectedRange(NSRange::new(at, 0));
}

fn a_formula_arriving_during_a_composition_leaves_the_text_alone(mtm: MainThreadMarker) {
    let text = "Dritter $x$ Absatz\n\nVierter Absatz\n";
    let (document, e) = open(mtm, text);
    caret_in_preview(&e, preview_offset_behind(&e, "Dritter "));
    let behind = preview_offset_behind(&e, "Absatz");
    e.preview().setSelectedRange(NSRange::new(behind, 0));
    compose(e.preview(), "にほ");
    js::set_ready(
        &js::Key::math("x", false, e.is_dark(), &e.model().read(|st| st.macros())),
        true,
    );
    e.invalidate(Invalidate::Arrivals);
    check(
        "a formula arriving during a composition leaves the composition alone",
        e.preview().hasMarkedText() && source(&e) == text,
        &source(&e),
    );
    step(&e, || type_in_preview(&e, "日本"));
    let written = source(&e);
    check(
        "what a composition ends with lands where it was typed",
        written == "Dritter $x$ Absatz日本\n\nVierter Absatz\n",
        &written,
    );
    document.close();
}

pub(super) fn a_cancelled_composition_ends_the_edit_it_announced(mtm: MainThreadMarker) {
    let (document, e) = open(mtm, "Erster Absatz\n\nZweiter Absatz\n");
    caret_in_preview(&e, 5);
    compose(e.preview(), "に");
    // SAFETY: An empty text ends the composition, as an input method does on Escape; the range is NSTextInputClient's "none".
    unsafe {
        e.preview().setMarkedText_selectedRange_replacementRange(
            &ns(""),
            NSRange::new(0, 0),
            NSRange::new(crate::util::to_usize(NSNotFound), 0),
        );
    }
    e.preview().unmarkText();
    next_turn();
    check(
        "a cancelled composition does not leave the preview waiting for its edit",
        !e.editing().preview_editing(),
        "",
    );
    e.preview().setSelectedRange(NSRange::new(20, 0));
    check(
        "the caret mirrors to the source after a cancelled composition",
        e.source().selectedRange().location > 14,
        &format!("{:?}", e.source().selectedRange()),
    );
    document.close();
}

fn a_preview_of_many_tables_answers_at_once(mtm: MainThreadMarker) {
    let table = format!("| A | B | C |\n|---|---|---|\n{}\n", "| 1 | 2 | 3 |\n".repeat(50));
    let (document, e) = open(mtm, &table.repeat(771));
    let count = e.model().read(|st| st.blocks().placeholders.len());
    e.progressive().render_blocks(0..count, 0, None);
    let preview = e.preview();
    let layout = crate::util::layout_manager(preview).expect("a layout manager");
    let len = crate::util::storage(preview).length();
    let started = Instant::now();
    for quarter in 1..4 {
        let glyph = layout.glyphIndexForCharacterAtIndex(len / 4 * quarter);
        // SAFETY: A null range pointer is allowed when the effective range is not wanted.
        let _ = unsafe { layout.lineFragmentRectForGlyphAtIndex_effectiveRange(glyph, std::ptr::null_mut()) };
    }
    let took = started.elapsed().as_secs_f64();
    check(
        "a far line of a preview of many tables has its place at once",
        took < 5.0,
        &format!("{took:.1} s"),
    );
    document.close();
}

fn the_notice_of_a_huge_document_takes_no_input(mtm: MainThreadMarker) {
    let (document, e) = open(mtm, "Absatz\n");
    let huge = "| Zeile |\n".repeat(3 << 20);
    e.updater().replace_all_text(&huge);
    e.updater().update_now();
    let notice = e.preview().string().to_string();
    caret_in_preview(&e, 3);
    type_in_preview(&e, "x");
    next_turn();
    check(
        "a keystroke in the notice of a huge document changes nothing",
        !e.preview().isEditable() && source(&e) == huge && e.preview().string().to_string() == notice,
        &notice,
    );
    e.window().expect("a window").makeFirstResponder(Some(e.source()));
    e.updater().replace_all_text("Absatz\n");
    e.updater().update_now();
    caret_in_preview(&e, 3);
    step(&e, || type_in_preview(&e, "x"));
    check(
        "the preview takes input again once the text is small enough",
        e.preview().isEditable() && source(&e) == "Absxatz\n",
        &source(&e),
    );
    document.close();
}

fn a_click_on_a_task_behind_the_text_changes_nothing(mtm: MainThreadMarker) {
    let (document, e) = open(mtm, "Absatz\n\n- [ ] Aufgabe\n");
    let at = e.preview().string().to_string().encode_utf16().count() - 3;
    step(&e, || {
        type_in_source(&e, "Hallo Welt ");
        let view: &NSTextView = e.preview();
        e.links().link_clicked(view, &ns("splat-task"), at);
    });
    let written = source(&e);
    check(
        "a click on a task the preview shows from before the last keystroke changes nothing",
        written == "Hallo Welt Absatz\n\n- [ ] Aufgabe\n",
        &written,
    );
    document.close();
}

fn a_space_behind_bold_and_the_next_word_stay_bold(mtm: MainThreadMarker) {
    let (document, e) = open(mtm, "**ab** c\n");
    caret_in_preview(&e, 2);
    step(&e, || type_in_preview(&e, " "));
    let after_space = source(&e);
    check(
        "a space typed behind bold shows no markers",
        after_space == "**ab**  c\n",
        &after_space,
    );
    step(&e, || type_in_preview(&e, "d"));
    let after_word = source(&e);
    check(
        "the word behind that space is bold as well",
        after_word == "**ab** **d** c\n",
        &after_word,
    );
    document.close();
}

/// A formula editor open on `$x$`, with `abc ` typed before it, and the text of its view changed to `y`.
fn editor_with_edit(mtm: MainThreadMarker) -> (Retained<MdDocument>, Retained<EditorController>) {
    let (document, e) = open(mtm, "Formel $x$ und Rest des Absatzes\n");
    step(&e, || type_in_source(&e, "abc "));
    e.updater().update_now();
    open_editor(&e);
    (document, e)
}

fn open_editor(e: &EditorController) {
    let at = source(e).find('$').expect("a formula");
    let range = e.atom_editor().starting_at(Byte::new(at)).expect("a formula there");
    e.atom_editor().open(range, None);
}

fn type_in_editor(e: &EditorController) {
    let view = e.atom_editor().code_view().expect("an open editor");
    let all = NSRange::new(0, view.string().length());
    view.setSelectedRange(all);
    crate::util::insert_text(&view, &ns("y"), all);
}

fn is_open(e: &EditorController) -> bool {
    e.atom_editor().is_open()
}

fn the_formula_editor_keeps_the_text_whole(mtm: MainThreadMarker) {
    KEEP_EDITOR.set(true);

    let (document, e) = editor_with_edit(mtm);
    let separate = e.atom_editor().code_view().is_some_and(|view| {
        view.undoManager()
            .zip(e.undo_manager())
            .is_some_and(|(own, shared)| !std::ptr::eq(&*own, &*shared))
    });
    check("the formula editor has an undo history of its own", separate, "");
    e.undo_manager().expect("an undo manager").undo();
    check(
        "undoing in the document with the editor open closes it",
        !is_open(&e) && source(&e) == "Formel $x$ und Rest des Absatzes\n",
        &source(&e),
    );
    document.close();

    let (document, e) = editor_with_edit(mtm);
    type_in_editor(&e);
    step(&e, || e.perform(Action::BulletList));
    check(
        "a command with the editor open commits the formula first",
        !is_open(&e) && source(&e) == "- abc Formel $y$ und Rest des Absatzes\n",
        &source(&e),
    );
    document.close();

    let (document, e) = open(mtm, "Formel $x$ und Rest\n");
    open_editor(&e);
    type_in_editor(&e);
    check(
        "the source shows the formula as it is typed",
        source(&e).contains("$y$"),
        &source(&e),
    );
    e.atom_editor().mark_cancelled();
    step(&e, || e.atom_editor().editor_closed());
    check(
        "a discarded formula leaves text and document as they were",
        source(&e) == "Formel $x$ und Rest\n" && !document.isDocumentEdited(),
        &format!("{:?} edited {}", source(&e), document.isDocumentEdited()),
    );
    document.close();

    let (document, e) = open(mtm, "Formel $x$ und Rest\n");
    open_editor(&e);
    type_in_editor(&e);
    step(&e, || e.atom_editor().editor_closed());
    check(
        "closing the editor commits the formula",
        source(&e) == "Formel $y$ und Rest\n",
        &source(&e),
    );
    let undo = e.undo_manager().expect("an undo manager");
    undo.undo();
    check(
        "one undo takes the formula back",
        source(&e) == "Formel $x$ und Rest\n" && !document.isDocumentEdited() && !undo.canUndo(),
        &format!(
            "{:?} edited {} more {}",
            source(&e),
            document.isDocumentEdited(),
            undo.canUndo()
        ),
    );
    document.close();

    KEEP_EDITOR.set(false);
}

/// Half transparent white over black, as the decoder's straight alpha asks for it.
fn a_translucent_pixel_stays_translucent() {
    let image = crate::editor::decoder::image(crate::editor::decoder::Decoded::Rgba {
        width: 1,
        height: 1,
        pixels: vec![255, 255, 255, 128],
        natural: (1, 1),
    })
    .expect("an image");
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
    // SAFETY: Without plane pointers the image rep allocates its own buffer.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            1,
            1,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }
    .expect("a bitmap");
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep).expect("a context");
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    NSColor::blackColor().setFill();
    NSBezierPath::fillRect(rect);
    image.drawInRect(rect);
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    let red = rep.colorAtX_y(0, 0).map_or(1.0, |c| c.redComponent());
    check(
        "half transparent white over black comes out grey",
        (0.4..0.6).contains(&red),
        &format!("{red}"),
    );
}

/// What a real panic outside of `contained` leaves behind: the text of edited documents and of nothing else.
fn a_panic_saves_what_is_unsaved_and_only_that(mtm: MainThreadMarker) {
    let (edited, e) = open(mtm, "Ungesicherter Text\n");
    step(&e, || type_in_source(&e, "x"));
    edited.updateChangeCount(objc2_app_kit::NSDocumentChangeType::ChangeDone);
    let (saved, _saved_editor) = open(mtm, "Gesicherter Text\n");
    let folder = crate::editor::recovery_dir().expect("a recovery folder");
    let listed = || -> Vec<std::path::PathBuf> {
        std::fs::read_dir(&folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|f| f.path())
            .collect()
    };
    let before = listed();
    crate::util::uncontained(|| std::panic::catch_unwind(|| panic!("a panic the checks provoke"))).ok();
    let new: Vec<_> = listed().into_iter().filter(|f| !before.contains(f)).collect();
    let texts: Vec<String> = new.iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect();
    check(
        "a panic saves the text of an edited document",
        texts.iter().any(|t| t.contains("Ungesicherter Text")),
        &format!("{texts:?}"),
    );
    check(
        "a panic does not copy a document without changes",
        !texts
            .iter()
            .any(|t| t.contains("Gesicherter Text\n") && !t.contains("Ungesichert")),
        &format!("{texts:?}"),
    );
    for file in new {
        let _ = std::fs::remove_file(file);
    }
    edited.updateChangeCount(objc2_app_kit::NSDocumentChangeType::ChangeCleared);
    edited.close();
    saved.close();
}

/// Waits until `condition` holds, for what comes back from another thread.
pub(super) fn until(condition: impl Fn() -> bool) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed().as_secs_f64() < super::UNTIL_TIMEOUT {
        if condition() {
            return true;
        }
        next_turn();
    }
    false
}

/// A file that changes under an open document in the ways a file can.
fn the_file_under_a_document_may_do_anything(mtm: MainThreadMarker) {
    let path = std::env::temp_dir().join(format!("splat-probe-file-{}.md", std::process::id()));
    std::fs::write(&path, "Text\n").expect("a test file");
    let (document, e) = super::documents::opened(mtm, &path);

    let look = || crate::document::reload_changed_documents(mtm);
    let shows = |text: &str| {
        let wanted = text.to_string();
        let e = e.clone();
        move || {
            look();
            source(&e) == wanted
        }
    };
    std::fs::write(&path, "Neu\n").expect("a test file");
    check("a changed file reloads", until(shows("Neu\n")), &source(&e));
    std::fs::write(&path, b"\xFF\xFE\0\0nicht lesbar").expect("a test file");
    check(
        "a file that cannot be read keeps the text it replaced",
        super::saving::looked_at_files(mtm) && source(&e) == "Neu\n",
        &source(&e),
    );
    std::fs::write(&path, "Wieder da\n").expect("a test file");
    check(
        "a file that can be read again reloads",
        until(shows("Wieder da\n")),
        &source(&e),
    );
    let _ = std::fs::remove_file(&path);
    check(
        "a deleted file leaves its text open",
        super::saving::looked_at_files(mtm) && source(&e) == "Wieder da\n",
        &source(&e),
    );
    document.close();
}

/// The text the command steps of the window self-test find, once its click-through has edited the quick reference.
fn text_of_the_command_steps() -> String {
    let reference = include_str!("../../../resources/de.lproj/QuickReference.md");
    format!(
        "{}\n\n## Neue Formate\n\nEin wichtiges Wort hier.\n{SECTION}",
        reference.replacen("# Markdown-Kurzreferenz", "# Markdown-Kurzreferenz X", 1)
    )
}

/// The table of what each command writes, on the text the window self-test runs it on, and each one's undo.
fn every_command_writes_exactly_its_text(mtm: MainThreadMarker) {
    let (document, e) = open(mtm, &text_of_the_command_steps());
    let undo = e.undo_manager().expect("an undo manager");
    for command in COMMANDS.iter().chain(&TABLE_COMMANDS) {
        let old = source(&e);
        step(&e, || command.run(&e));
        command.check_text(&e, &old);
        undo.undo();
        e.updater().update_now();
        check(
            &format!("{} is undone in one step", command.name),
            source(&e) == old,
            &format!("{} bytes, before {}", source(&e).len(), old.len()),
        );
    }
    document.close();
}

/// The images of the test section in every format, through both exports.
fn the_exports_carry_every_image_format(mtm: MainThreadMarker) {
    let dir = std::env::temp_dir().join(format!("splat-probe-section-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    scenarios::write_files(&dir);
    let (document, e) = open(mtm, SECTION);
    let path = dir.join("Abschnitt.md");
    document.setFileURL(Some(&NSURL::fileURLWithPath(&ns(path.to_str().unwrap_or_default()))));
    scenarios::check_html_export(&e, &dir);
    scenarios::check_word_export(&e, &dir);
    document.close();
    let _ = std::fs::remove_dir_all(&dir);
}

pub(super) fn checks(mtm: MainThreadMarker) {
    type Part = (&'static str, fn(MainThreadMarker));
    let parts: [Part; 12] = [
        ("the panic check", a_panic_saves_what_is_unsaved_and_only_that),
        ("the file checks", the_file_under_a_document_may_do_anything),
        ("the saving checks", super::saving::checks),
        ("the transparency check", |_| a_translucent_pixel_stays_translucent()),
        (
            "the composition check",
            a_formula_arriving_during_a_composition_leaves_the_text_alone,
        ),
        ("the huge document check", the_notice_of_a_huge_document_takes_no_input),
        ("the many tables check", a_preview_of_many_tables_answers_at_once),
        (
            "the task click check",
            a_click_on_a_task_behind_the_text_changes_nothing,
        ),
        ("the bold typing check", a_space_behind_bold_and_the_next_word_stay_bold),
        ("the formula editor checks", the_formula_editor_keeps_the_text_whole),
        ("the command table", every_command_writes_exactly_its_text),
        ("the image formats of the exports", the_exports_carry_every_image_format),
    ];
    for (name, part) in parts {
        guarded(name, || part(mtm));
    }
    KEEP_EDITOR.set(false);
}
