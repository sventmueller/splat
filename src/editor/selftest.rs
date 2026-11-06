//! Scripted end-to-end check of the running app, compiled only with `--features selftest`.

#![allow(
    clippy::undocumented_unsafe_blocks,
    clippy::too_many_lines,
    reason = "a script of the app's own calls, one step after the other, whose outcome is what it checks"
)]

mod bench;
mod clickthrough;
mod controller;
mod documents;
mod mock_web;
mod notes;
mod probe;
mod resize;
mod saving;
mod scenarios;
mod typing;

use std::cell::Cell;
use std::path::{Path, PathBuf};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSApplication, NSDocumentController, NSPrintInfo, NSPrintJobDisposition, NSPrintJobSavingURL, NSPrintSaveJob,
    NSTextInputClient, NSWindow,
};
use objc2_foundation::{NSCopying, NSNotFound, NSRange, NSString, NSTimer, NSURL};

pub use mock_web::protocol_classes;

use super::EditorController;
use crate::document::MdDocument;
use crate::markdown::text::{Byte, TextAt, Utf16Map, bytes};
use crate::util::{ns, obj, storage};

thread_local! {
    static FAILURES: Cell<usize> = const { Cell::new(0) };
    /// Steps have no names; this tells where one that fails to run stands.
    static LAST_CHECK: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// The checks act like the user: what they do to the views is meant to reach the delegate callbacks.
impl EditorController {
    pub(in crate::editor::selftest) fn source(&self) -> &crate::editor::views::SourceTextView {
        self.panes().for_appkit().source
    }

    pub(in crate::editor::selftest) fn preview(&self) -> &crate::editor::views::PreviewTextView {
        self.panes().for_appkit().preview
    }
}

fn check(name: &str, ok: bool, detail: &str) {
    if ok {
        eprintln!("SELFTEST PASS {name}");
    } else {
        FAILURES.with(|f| f.set(f.get() + 1));
        eprintln!("SELFTEST FAIL {name}: {detail}");
    }
    reported(name);
}

/// A check that cannot say anything in this run: it neither passes nor fails, and xtask lists it.
fn skip(name: &str, reason: &str) {
    eprintln!("SELFTEST SKIP {name}: {reason}");
    reported(name);
}

/// Whether the screen is locked right now, as it can become during a run.
fn screen_is_locked() -> bool {
    std::process::Command::new("ioreg")
        .args(["-n", "Root", "-d1", "-a"])
        .output()
        .is_ok_and(|output| {
            let plist = String::from_utf8_lossy(&output.stdout);
            plist
                .split_once("<key>CGSSessionScreenIsLocked</key>")
                .is_some_and(|(_, behind)| behind.trim_start().starts_with("<true/>"))
        })
}

/// A check of what AppKit does only for the app in front, which a locked screen or another app takes away. A run leaves it
/// out on a locked screen, or if it was told to run behind other apps.
fn check_in_front(mtm: MainThreadMarker, name: &str, ok: bool, detail: &str) {
    if NSApplication::sharedApplication(mtm).isActive() {
        check(name, ok, detail);
    } else if screen_is_locked() {
        skip(name, "the screen is locked");
    } else if std::env::var_os("SPLAT_SELFTEST_IN_BACK").is_some() {
        skip(name, "the app is not active");
    } else {
        check(name, false, "the app is not active");
    }
}

fn reported(name: &str) {
    LAST_CHECK.with(|c| name.clone_into(&mut c.borrow_mut()));
    // A shorter run for one area: SPLAT_SELFTEST_STOP_AFTER names the last check.
    if std::env::var("SPLAT_SELFTEST_STOP_AFTER").is_ok_and(|last| last == name) {
        finish();
    }
}

/// Runs a part of the run that makes checks. One that cannot go on is a failed check, and what stands behind it still runs.
fn guarded(what: &str, f: impl FnOnce()) {
    if crate::util::contained(f).is_none() {
        check(&format!("{what} runs to its end"), false, "it panicked, see above");
    }
}

fn after(seconds: f64, f: impl Fn() + 'static) {
    let block = RcBlock::new(move |_timer: std::ptr::NonNull<NSTimer>| guarded("a timed part", &f));
    unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(seconds, false, &block) };
}

/// The one way the self-test waits without blocking: from `seconds` on, looks at `holds` every 50 ms until it held
/// `in_a_row` looks in a row or `timeout` seconds passed, then tells `done` whether it held. A run-loop turn lies between
/// two looks, so what the first look started is seen by a later one; `controller::until` is the blocking form.
fn wait(
    seconds: f64,
    timeout: f64,
    in_a_row: usize,
    holds: impl Fn() -> bool + 'static,
    done: impl Fn(bool) + 'static,
) {
    fn look(
        holds: std::rc::Rc<dyn Fn() -> bool>,
        done: std::rc::Rc<dyn Fn(bool)>,
        started: std::time::Instant,
        timeout: f64,
        needed: usize,
        held: usize,
    ) {
        let held = if holds() { held + 1 } else { 0 };
        if held >= needed {
            done(true);
        } else if started.elapsed().as_secs_f64() > timeout {
            done(false);
        } else {
            after(0.05, move || {
                look(holds.clone(), done.clone(), started, timeout, needed, held)
            });
        }
    }
    let holds: std::rc::Rc<dyn Fn() -> bool> = std::rc::Rc::new(holds);
    let done: std::rc::Rc<dyn Fn(bool)> = std::rc::Rc::new(done);
    after(seconds, move || {
        look(
            holds.clone(),
            done.clone(),
            std::time::Instant::now(),
            timeout,
            in_a_row.max(1),
            0,
        );
    });
}

/// Calls `then` once `cond` holds, `seconds` from now at the earliest. Waiting in vain is a failed check that names what
/// never came, and `then` still runs, as its checks tell what is missing.
fn once(seconds: f64, name: &'static str, cond: impl Fn() -> bool + 'static, then: impl Fn() + 'static) {
    wait(seconds, UNTIL_TIMEOUT, 1, cond, move |held| {
        if !held {
            check(name, false, &format!("waited {UNTIL_TIMEOUT} s for it"));
        }
        then();
    });
}

/// Writes the visible part of both panes as PDF (text views draw reliably through the printing path).
fn snapshot(window: &NSWindow, path: &Path) {
    let Some(e) = window
        .windowController()
        .and_then(|c| c.downcast::<EditorController>().ok())
    else {
        return;
    };
    for (name, view) in [
        ("source", e.source() as &objc2_app_kit::NSTextView),
        ("preview", e.preview() as &objc2_app_kit::NSTextView),
    ] {
        let rect = view.visibleRect();
        let data = view.dataWithPDFInsideRect(rect);
        let file = path.with_file_name(format!("{}-{name}.pdf", path.file_stem().unwrap().to_string_lossy()));
        data.writeToFile_atomically(&ns(file.to_str().unwrap()), true);
        eprintln!("SELFTEST SNAPSHOT {}", file.display());
    }
}

/// The window of the test file; windows macOS restores from earlier runs are ignored.
fn editor(mtm: MainThreadMarker) -> Option<Retained<EditorController>> {
    let file = std::env::var("SPLAT_SELFTEST_FILE").ok()?;
    let docs = NSDocumentController::sharedDocumentController(mtm).documents();
    docs.iter()
        .filter(|d| {
            d.fileURL()
                .and_then(|u| u.path())
                .is_some_and(|p| p.to_string() == file)
        })
        .find_map(|d| d.downcast::<MdDocument>().ok())
        .and_then(|d| d.editor())
}

/// Background colour on a screenshot of the window, just below the top of the character at `index`.
fn background_at(image: &Path, view: &objc2_app_kit::NSTextView, index: usize) -> Option<(f64, f64, f64)> {
    let data = objc2_foundation::NSData::dataWithContentsOfFile(&ns(image.to_str()?))?;
    let rep = objc2_app_kit::NSBitmapImageRep::imageRepWithData(&data)?;
    let rect = super::scroll::char_rect(view, index);
    let point = view.convertPoint_toView(
        objc2_foundation::NSPoint::new(rect.origin.x + rect.size.width / 2.0, rect.origin.y + 2.0),
        None,
    );
    let window = view.window()?;
    let scale = window.backingScaleFactor();
    #[expect(clippy::cast_possible_truncation, reason = "a pixel inside a window")]
    let (x, y) = (
        (point.x * scale) as isize,
        ((window.frame().size.height - point.y) * scale) as isize,
    );
    let c = rep
        .colorAtX_y(x, y)?
        .colorUsingColorSpace(&objc2_app_kit::NSColorSpace::sRGBColorSpace())?;
    Some((
        c.redComponent() * 255.0,
        c.greenComponent() * 255.0,
        c.blueComponent() * 255.0,
    ))
}

/// The highlight in the other view has the colour of the real selection in the focused one, and differs from text
/// that is not selected.
fn same_selection_colour(
    name: &str,
    image: &Path,
    (focused, focused_at): (&objc2_app_kit::NSTextView, usize),
    (other, other_at): (&objc2_app_kit::NSTextView, usize),
) {
    let close = |a: (f64, f64, f64), b: (f64, f64, f64)| {
        (a.0 - b.0).abs() < 12.0 && (a.1 - b.1).abs() < 12.0 && (a.2 - b.2).abs() < 12.0
    };
    let selected = background_at(image, focused, focused_at + 1);
    let marked = background_at(image, other, other_at + 1);
    let plain = background_at(image, other, other_at + 8);
    let ok = match (selected, marked, plain) {
        (Some(s), Some(m), Some(p)) => close(s, m) && !close(m, p),
        _ => false,
    };
    check_in_front(
        focused.mtm(),
        name,
        ok,
        &format!("selection {selected:?}, highlight {marked:?}, unselected {plain:?}"),
    );
}

fn editable_fields(view: &objc2_app_kit::NSView, out: &mut Vec<Retained<objc2_app_kit::NSTextField>>) {
    for sub in view.subviews().iter() {
        if let Ok(field) = sub.clone().downcast::<objc2_app_kit::NSTextField>()
            && field.isEditable()
        {
            out.push(field);
        }
        editable_fields(&sub, out);
    }
}

fn check_link_dialog(mtm: MainThreadMarker, image: &Path) {
    let (alert, url, text) = super::dialogs::link_alert(mtm, "Beispieltext");
    let Some(content) = alert.window().contentView() else {
        check("link dialog fields are full width", false, "no content view");
        return;
    };
    let width = |f: &objc2_app_kit::NSTextField| f.convertRect_toView(f.bounds(), Some(&content)).size.width;
    check(
        "link dialog fields are full width",
        width(&url) >= 300.0 && width(&text) >= 300.0,
        &format!("address {:.0}, text {:.0}", width(&url), width(&text)),
    );
    let bounds = content.bounds();
    if let Some(rep) = content.bitmapImageRepForCachingDisplayInRect(bounds) {
        content.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
        let png = unsafe {
            rep.representationUsingType_properties(
                objc2_app_kit::NSBitmapImageFileType::PNG,
                &objc2_foundation::NSDictionary::new(),
            )
        };
        if let (Some(png), Some(path)) = (png, image.to_str()) {
            png.writeToFile_atomically(&ns(path), true);
            eprintln!("SELFTEST SNAPSHOT {path}");
        }
    }
}

/// UTF-16 offsets of "Neuer Absatz" in the preview and in the source.
fn neuer_absatz(e: &EditorController) -> (usize, usize) {
    let p = storage(e.preview()).string().to_string();
    let s = source(e);
    (
        p[..p.find("Neuer Absatz").unwrap()].encode_utf16().count(),
        s[..s.find("Neuer Absatz").unwrap()].encode_utf16().count(),
    )
}

pub(crate) use controller::keeps_unshown_editor;

fn head(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub fn start(mtm: MainThreadMarker) {
    super::panes::forget_sidebar();
    // The checks look at the count in the corner, which windows hide until someone asks for it.
    super::counter::set_on(true);
    if std::env::var_os("SPLAT_RENDER_PROBE").is_some() {
        probe::run(mtm);
        return;
    }
    if std::env::var_os("SPLAT_PRINT_MENUS").is_some() {
        if let Some(menu) = NSApplication::sharedApplication(mtm).mainMenu() {
            for top in menu.itemArray().iter() {
                let items: Vec<String> = top
                    .submenu()
                    .map(|m| {
                        m.itemArray()
                            .iter()
                            .filter(|i| !i.isHidden() && !i.isAlternate())
                            .map(|i| {
                                if i.isSeparatorItem() {
                                    "—".to_string()
                                } else {
                                    i.title().to_string()
                                }
                            })
                            .filter(|t| !t.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                eprintln!("MENU {} | {}", top.title(), items.join(" | "));
            }
        }
        std::process::exit(0);
    }
    if let Ok(path) = std::env::var("SPLAT_DIALOG_PNG") {
        check_link_dialog(mtm, Path::new(&path));
        std::process::exit(0);
    }
    let Ok(file) = std::env::var("SPLAT_SELFTEST_FILE") else {
        return;
    };
    if std::env::var_os("SPLAT_RESIZE_PROBE").is_some() {
        open_document(mtm, &file);
        once(
            2.5,
            "the document window to open",
            move || window_rendered(mtm),
            move || resize::resize_probe(mtm),
        );
        return;
    }
    if std::env::var_os("SPLAT_TYPING_PROBE").is_some() {
        open_document(mtm, &file);
        once(
            2.5,
            "the document window to open",
            move || window_rendered(mtm),
            move || typing::run(mtm),
        );
        return;
    }
    if std::env::var("SPLAT_BENCH").is_ok() {
        let opened = std::time::Instant::now();
        open_document(mtm, &file);
        bench::after_first_render(mtm, opened);
        return;
    }
    if std::env::var("SPLAT_SELFTEST_EXPECT_OPEN").is_ok() {
        // Launched by LaunchServices with the file as an open-document event, like a Finder double-click.
        let opened = {
            let file = file.clone();
            move || {
                NSDocumentController::sharedDocumentController(mtm)
                    .documents()
                    .iter()
                    .any(|d| {
                        d.fileURL()
                            .and_then(|u| u.path())
                            .is_some_and(|p| p.to_string() == file)
                            && d.downcast::<MdDocument>().is_ok()
                    })
            }
        };
        let rendered = move || editor(mtm).is_some_and(|e| storage(e.preview()).length() > 100);
        let (opened_then, rendered_then) = (opened.clone(), rendered);
        once(
            0.0,
            "the file of the open-document event to open and render",
            move || opened() && rendered(),
            move || {
                check(
                    "double-click opens file in cold-started app",
                    opened_then() && rendered_then(),
                    &format!("opened={} rendered={}", opened_then(), rendered_then()),
                );
                finish();
            },
        );
        return;
    }
    let out = PathBuf::from(std::env::var("SPLAT_SELFTEST_OUT").unwrap_or_else(|_| ".".into()));
    forget_web_image_permission(&file);
    // AppKit opens its untitled window after this call, and says so to nobody.
    after(1.0, move || {
        let (file, out) = (file.clone(), out.clone());
        dock_click_opens_a_document(mtm, move || {
            open_document(mtm, &file);
            let out = out.clone();
            once(
                1.5,
                "the test file to open and render",
                move || window_rendered(mtm),
                move || run(mtm, &out),
            );
        });
    });
}

fn open_document(mtm: MainThreadMarker, file: &str) {
    super::panes::forget_sidebar();
    let controller = NSDocumentController::sharedDocumentController(mtm);
    let url = NSURL::fileURLWithPath(&ns(file));
    let done = RcBlock::new(
        |_: *mut objc2_app_kit::NSDocument, _: objc2::runtime::Bool, _: *mut objc2_foundation::NSError| {},
    );
    controller.openDocumentWithContentsOfURL_display_completionHandler(&url, true, &done);
}

/// The window of the test file shows, with the text applied.
fn window_rendered(mtm: MainThreadMarker) -> bool {
    editor(mtm).is_some_and(|e| e.window().is_some_and(|w| w.isVisible()) && quiet(&e))
}

/// User defaults keep the permission an earlier run gave its copy of the document.
fn forget_web_image_permission(file: &str) {
    let paths: Vec<_> = super::images::allowed_documents()
        .iter()
        .filter(|p| *p != file)
        .map(|p| ns(p))
        .collect();
    let list = objc2_foundation::NSArray::from_retained_slice(&paths);
    unsafe {
        objc2_foundation::NSUserDefaults::standardUserDefaults()
            .setObject_forKey(Some(&list), &ns(super::images::ALLOWED_KEY))
    };
}

fn document_window_shown(mtm: MainThreadMarker) -> bool {
    NSDocumentController::sharedDocumentController(mtm)
        .documents()
        .iter()
        .flat_map(|d| d.windowControllers().to_vec())
        .any(|c| c.window().is_some_and(|w| w.isVisible()))
}

/// Closes every document, with the formula renderer's hidden window in place, and sends the event a click on the
/// Dock icon sends; AppKit must answer it with a new document window. Then closes that one too and calls `then`.
fn dock_click_opens_a_document(mtm: MainThreadMarker, then: impl Fn() + 'static) {
    super::js::renderer(mtm);
    let controller = NSDocumentController::sharedDocumentController(mtm);
    for doc in controller.documents().to_vec() {
        doc.close();
    }
    let then = std::rc::Rc::new(then);
    let no_window = move || !document_window_shown(mtm);
    once(0.5, "the open documents to close", no_window, move || {
        let closed = !document_window_shown(mtm);
        let target = objc2_foundation::NSAppleEventDescriptor::currentProcessDescriptor();
        let event: Retained<objc2_foundation::NSAppleEventDescriptor> = unsafe {
            objc2::msg_send![
                <objc2_foundation::NSAppleEventDescriptor as objc2::ClassType>::class(),
                appleEventWithEventClass: u32::from_be_bytes(*b"aevt"),
                eventID: u32::from_be_bytes(*b"rapp"),
                targetDescriptor: &*target,
                returnID: -1i16,
                transactionID: 0i32
            ]
        };
        let sent = event
            .sendEventWithOptions_timeout_error(objc2_foundation::NSAppleEventSendOptions::NoReply, 5.0)
            .map_err(|e| e.localizedDescription().to_string());
        let then = then.clone();
        wait(
            0.0,
            5.0,
            1,
            move || document_window_shown(mtm),
            move |shown| {
                check(
                    "a click on the Dock icon opens a document",
                    closed && sent.is_ok() && shown,
                    &format!("closed before={closed} sent={sent:?} window shown={shown}"),
                );
                for doc in NSDocumentController::sharedDocumentController(mtm).documents().to_vec() {
                    doc.close();
                }
                let then = then.clone();
                once(0.3, "the document of the Dock icon to close", no_window, move || then());
            },
        );
    });
}

fn source(e: &EditorController) -> String {
    e.source().string().to_string()
}

fn type_in_preview(e: &EditorController, text: &str) {
    let preview = e.preview();
    unsafe { preview.insertText_replacementRange(&ns(text), NSRange::new(crate::util::to_usize(NSNotFound), 0)) };
}

/// What `--only` calls the click-through in front of the scenarios.
const CLICK_THROUGH: &str = "click-through";

struct Ctx {
    e: Retained<EditorController>,
    out: PathBuf,
    /// The test file as it was before the first edit, which each group of checks starts from.
    template: std::rc::Rc<str>,
}

/// The steps from here on work on `e`.
fn work_on(e: Retained<EditorController>) {
    CTX.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        if let Some(old) = ctx.take() {
            *ctx = Some(std::rc::Rc::new(Ctx {
                e,
                out: old.out.clone(),
                template: old.template.clone(),
            }));
        }
    });
}

/// What the runner waits for before a step, named for the failure when it never comes.
struct Wait {
    /// Before the condition is looked at, for what no condition can tell.
    at_least: f64,
    name: String,
    holds: Box<dyn Fn(&Ctx) -> bool>,
    /// How many looks in a row must find it, so work the first look triggered is seen as well.
    in_a_row: usize,
}

type Step = (Wait, Box<dyn Fn(&Ctx)>);

/// Longest wait for a condition. Waiting longer fails the run; the step still runs, as its checks tell what is missing.
const UNTIL_TIMEOUT: f64 = 15.0;

thread_local! {
    static STEPS: std::cell::RefCell<Vec<Step>> = std::cell::RefCell::new(Vec::new());
    static CTX: std::cell::RefCell<Option<std::rc::Rc<Ctx>>> = const { std::cell::RefCell::new(None) };
    static MONITOR: std::cell::RefCell<Option<Retained<objc2::runtime::AnyObject>>> = const { std::cell::RefCell::new(None) };
}

/// Posts an application-defined event that runs step `i`. AppKit closes the automatic undo group after
/// every event, so each step becomes its own undo step exactly like a keystroke or menu command.
fn post_step(i: usize) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let event = objc2_app_kit::NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        objc2_app_kit::NSEventType::ApplicationDefined,
        objc2_foundation::NSPoint::new(0.0, 0.0),
        objc2_app_kit::NSEventModifierFlags::empty(),
        0.0,
        0,
        None,
        0,
        crate::util::to_isize(i),
        0,
    );
    if let Some(event) = event {
        NSApplication::sharedApplication(mtm).postEvent_atStart(&event, false);
    }
}

/// Runs step `i` once its wait is over.
fn wait_for(i: usize) {
    let Some(ctx) = CTX.with(|c| c.borrow().clone()) else {
        return;
    };
    let (seconds, in_a_row, name) = STEPS.with(|s| {
        s.borrow().get(i).map_or((0.0, 1, String::new()), |(w, _)| {
            (w.at_least, w.in_a_row, w.name.clone())
        })
    });
    let looked_at = ctx.clone();
    let holds = move || STEPS.with(|s| s.borrow().get(i).is_none_or(|(w, _)| (w.holds)(&looked_at)));
    wait(seconds, UNTIL_TIMEOUT, in_a_row, holds, move |held| {
        let last = LAST_CHECK.with(|c| c.borrow().clone());
        let step = format!("the step after \"{last}\" that waits for {name}");
        if held {
            copy_is_the_source(&ctx.e, &step);
        } else {
            check(&format!("{step} gets it"), false, &format!("waited {UNTIL_TIMEOUT} s"));
        }
        post_step(i);
    });
}

/// After every step: what the editor thinks the text is, is what the view holds. Going apart is what makes edits land in
/// the wrong place, long before anything shows.
fn copy_is_the_source(e: &EditorController, step: &str) {
    if e.source().hasMarkedText() {
        return;
    }
    let shown = e.source().string().to_string();
    if e.source_text() != shown {
        FAILURES.with(|f| f.set(f.get() + 1));
        eprintln!("SELFTEST FAIL {step}: the copy of the text differs from the source view");
    }
}

/// The editor has applied the current text, or holds it back while an input method composes.
fn quiet(e: &EditorController) -> bool {
    if e.source().hasMarkedText() {
        return true;
    }
    let hidden = !e.panes().preview_visible();
    !e.model().update_is_scheduled()
        && !e.updater().arrivals_scheduled()
        // A hidden preview is laid out when it comes back.
        && e.with_text(move |snap, st| st.is_current(snap) && (!st.stale_layout() || hidden))
}

/// The editor has applied the current text, rendered its formulas and diagrams and loaded its images.
fn settled(e: &EditorController) -> bool {
    if !quiet(e) {
        return false;
    }
    let dark = e.is_dark();
    let macros = e.model().read(|st| st.macros());
    super::js::idle()
        && super::remote::idle()
        && e.model().read(move |st| {
            crate::markdown::doc::figures(st.doc())
                .iter()
                .all(|(_, f)| super::js::outcome(&super::update::render_key(f, dark, &macros)).is_some())
        })
}

fn run_steps(ctx: Ctx, list: Vec<Step>) {
    STEPS.with(|s| *s.borrow_mut() = list);
    CTX.with(|c| *c.borrow_mut() = Some(std::rc::Rc::new(ctx)));
    let block = RcBlock::new(
        |event: std::ptr::NonNull<objc2_app_kit::NSEvent>| -> *mut objc2_app_kit::NSEvent {
            let event = unsafe { event.as_ref() };
            if event.r#type() != objc2_app_kit::NSEventType::ApplicationDefined {
                return event as *const _ as *mut _;
            }
            let i = crate::util::to_usize(event.data1());
            let Some(ctx) = CTX.with(|c| c.borrow().clone()) else {
                return event as *const _ as *mut _;
            };
            let last = LAST_CHECK.with(|c| c.borrow().clone());
            let name = STEPS.with(|s| s.borrow().get(i).map(|(w, _)| w.name.clone()).unwrap_or_default());
            guarded(&format!("the step after \"{last}\" that waits for {name}"), || {
                STEPS.with(|s| (s.borrow()[i].1)(&ctx))
            });
            match STEPS.with(|s| s.borrow().len()) {
                n if i + 1 < n => wait_for(i + 1),
                _ => {
                    let copy = ctx.e.source_text();
                    check(
                        "the patched text copy matches the source view",
                        copy == ctx.e.exact_text(),
                        &format!("{} vs {} bytes", copy.len(), ctx.e.exact_text().len()),
                    );
                    finish()
                }
            }
            std::ptr::null_mut()
        },
    );
    let monitor = unsafe {
        objc2_app_kit::NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            objc2_app_kit::NSEventMask::ApplicationDefined,
            &block,
        )
    };
    MONITOR.with(|m| *m.borrow_mut() = monitor);
    wait_for(0);
}

/// A step that runs once `cond` holds. `name` completes "waited for ...".
fn when(name: &str, cond: impl Fn(&Ctx) -> bool + 'static, f: impl Fn(&Ctx) + 'static) -> Step {
    at_least(0.0, name, cond, f)
}

/// Like `when`, not before `seconds` have passed either, for a condition that alone may be too early.
fn at_least(seconds: f64, name: &str, cond: impl Fn(&Ctx) -> bool + 'static, f: impl Fn(&Ctx) + 'static) -> Step {
    (
        Wait {
            at_least: seconds,
            name: name.to_string(),
            holds: Box::new(cond),
            in_a_row: 3,
        },
        Box::new(f),
    )
}

const APPLIED: &str = "the editor to apply the text";

/// A step that runs once the editor has applied the text.
fn applied(f: impl Fn(&Ctx) + 'static) -> Step {
    when(APPLIED, |c| quiet(&c.e), f)
}

/// Like `applied`, not before `seconds` have passed either.
fn applied_after(seconds: f64, f: impl Fn(&Ctx) + 'static) -> Step {
    at_least(seconds, APPLIED, |c| quiet(&c.e), f)
}

/// A fixed delay, only where no condition exists, as `why` says.
fn pause(seconds: f64, why: &str, f: impl Fn(&Ctx) + 'static) -> Step {
    let (mut wait, f) = at_least(
        seconds,
        &format!("{APPLIED}, after a pause since {why}"),
        |c| quiet(&c.e),
        f,
    );
    wait.in_a_row = 1;
    (wait, f)
}

/// A step that runs once the editor has settled.
fn settle(f: impl Fn(&Ctx) + 'static) -> Step {
    when(
        "the editor to settle, with its formulas, diagrams and images in",
        |c| settled(&c.e),
        f,
    )
}

/// A step that runs once the formula or diagram editor has closed and the editor has settled.
fn closed(f: impl Fn(&Ctx) + 'static) -> Step {
    when(
        "the formula or diagram editor to close and the editor to settle",
        |c| !c.e.atom_editor().is_open() && settled(&c.e),
        f,
    )
}

thread_local! {
    static IGNORING: std::cell::RefCell<Option<Retained<objc2::runtime::AnyObject>>> = const { std::cell::RefCell::new(None) };
}

/// A key or click of whoever works at the Mac meanwhile would land in the window in front, move its caret and fail checks
/// that have nothing wrong with them. The run's own events go to the window directly and pass no monitor.
fn ignore_people(_mtm: MainThreadMarker) {
    use objc2_app_kit::{NSEvent, NSEventMask};
    let drop_it = RcBlock::new(|_: std::ptr::NonNull<NSEvent>| std::ptr::null_mut::<NSEvent>());
    let input = NSEventMask::KeyDown
        | NSEventMask::LeftMouseDown
        | NSEventMask::RightMouseDown
        | NSEventMask::OtherMouseDown
        | NSEventMask::ScrollWheel;
    // SAFETY: The handler touches nothing and answers on the main thread, and the monitor stays for the life of the run.
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(input, &drop_it) };
    IGNORING.with(|m| *m.borrow_mut() = monitor);
}

fn run(mtm: MainThreadMarker, out: &Path) {
    let Some((e, window)) = editor(mtm).and_then(|e| e.window().map(|w| (e, w))) else {
        check("open document", false, "no editor window");
        finish();
        return;
    };
    ignore_people(mtm);
    let template: std::rc::Rc<str> = source(&e).into();
    // How a run goes with another app in front, which macOS decides and a run has no say in: `--in-back` makes it certain.
    if std::env::var_os("SPLAT_SELFTEST_IN_BACK").is_some() {
        NSApplication::sharedApplication(mtm).deactivate();
    } else {
        #[expect(deprecated, reason = "the replacement does not bring a test run to the front")]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }
    window.makeKeyAndOrderFront(None);
    snapshot(&window, &out.join("01-opened.png"));
    let tab_plus = unsafe {
        NSApplication::sharedApplication(mtm).targetForAction_to_from(sel!(newWindowForTab:), None, Some(&window))
    };
    check(
        "tab bar shows no plus button of its own",
        tab_plus.is_none(),
        &format!("{tab_plus:?}"),
    );
    let content = window.contentView().map_or(0.0, |v| v.frame().size.height);
    let layout_height = window.contentLayoutRect().size.height;
    check(
        "content stays below toolbar and tab bar",
        content <= layout_height + 0.5,
        &format!("content {content}, layout rect {layout_height}"),
    );
    let original = source(&e);
    check(
        "document loaded",
        original.starts_with("# Markdown-Kurzreferenz"),
        &head(&original, 40),
    );
    let preview_len = storage(e.preview()).length();
    check(
        "preview rendered",
        preview_len > 500,
        &format!("preview length {preview_len}"),
    );
    let ctx = Ctx {
        e,
        out: out.to_path_buf(),
        template,
    };
    let only = std::env::var("SPLAT_SELFTEST_ONLY").ok();
    let click_through_alone = only.as_deref() == Some(CLICK_THROUGH);
    let one_group = clickthrough::groups()
        .iter()
        .any(|(name, _)| only.as_deref() == Some(*name));
    if std::env::var_os("SPLAT_SELFTEST_SCENARIOS").is_some() && !click_through_alone && !one_group {
        run_steps(ctx, scenarios::steps());
        return;
    }
    // The first step waits until formulas and diagrams are rendered, so the edits below do not race them.
    let mut steps = Vec::new();
    for (i, (name, group)) in clickthrough::groups().into_iter().enumerate() {
        if !one_group || only.as_deref() == Some(name) {
            if i > 0 {
                steps.extend(clickthrough::entry(name));
            }
            steps.extend(group);
        }
    }
    if !click_through_alone && !one_group {
        steps.extend(scenarios::steps());
    }
    run_steps(ctx, steps);
}

/// The UTF-16 index of the first character in the preview that links to `target`.
fn preview_link(e: &EditorController, target: &str) -> Option<usize> {
    let text = storage(e.preview());
    (0..text.length()).find(|&i| {
        let value = unsafe {
            text.attribute_atIndex_effectiveRange(objc2_app_kit::NSLinkAttributeName, i, std::ptr::null_mut())
        };
        value.is_some_and(|v| {
            v.downcast_ref::<NSURL>()
                .and_then(|u| u.absoluteString())
                .or_else(|| v.downcast_ref::<NSString>().map(|s| s.copy()))
                .is_some_and(|s| s.to_string() == target)
        })
    })
}

/// A click on the preview character at UTF-16 index `at`, delivered the way AppKit gets one from the mouse.
fn click_preview(e: &EditorController, at: usize, flags: objc2_app_kit::NSEventModifierFlags) {
    let preview = e.preview();
    let (Some(lm), Some(container), Some(window)) = (
        crate::util::layout_manager(preview),
        unsafe { preview.textContainer() },
        e.window(),
    ) else {
        return;
    };
    let glyphs =
        unsafe { lm.glyphRangeForCharacterRange_actualCharacterRange(NSRange::new(at, 1), std::ptr::null_mut()) };
    let rect = lm.boundingRectForGlyphRange_inTextContainer(glyphs, &container);
    let origin = preview.textContainerOrigin();
    let point = preview.convertPoint_toView(
        objc2_foundation::NSPoint::new(
            rect.origin.x + rect.size.width / 2.0 + origin.x,
            rect.origin.y + rect.size.height / 2.0 + origin.y,
        ),
        None,
    );
    let event = |kind| {
        objc2_app_kit::NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind,
            point,
            flags,
            objc2_foundation::NSProcessInfo::processInfo().systemUptime(),
            window.windowNumber(),
            None,
            0,
            1,
            1.0,
        )
    };
    if let (Some(down), Some(up)) = (
        event(objc2_app_kit::NSEventType::LeftMouseDown),
        event(objc2_app_kit::NSEventType::LeftMouseUp),
    ) {
        // The text view follows the mouse until the button goes up and takes the release from the queue.
        NSApplication::sharedApplication(e.mtm()).postEvent_atStart(&up, false);
        window.sendEvent(&down);
    }
}

fn perform(e: &EditorController, action: objc2::runtime::Sel, tag: isize) {
    let sender = crate::util::tagged(e.mtm(), "", action, "", tag);
    let _ = unsafe { e.source().tryToPerform_with(action, Some(&sender)) };
}

fn set_source_caret(e: &EditorController, at: usize) {
    let s = source(e);
    e.source()
        .setSelectedRange(NSRange::new(Utf16Map::new(&s).to_u16(Byte::new(at)).get(), 0));
}

fn select_in_source(e: &EditorController, needle: &str) {
    let s = source(e);
    // A check, so that one step that went wrong does not end the run and hide the others.
    let Some(at) = s.rfind(needle) else {
        check(&format!("the source holds {needle:?} to select"), false, "");
        return;
    };
    let map = Utf16Map::new(&s);
    e.window().unwrap().makeFirstResponder(Some(e.source()));
    e.source().setSelectedRange(NSRange::new(
        map.to_u16(Byte::new(at)).get(),
        needle.encode_utf16().count(),
    ));
}

/// Shows `text` as marked text at the caret, as an input method does while a word is being composed.
fn compose(view: &objc2_app_kit::NSTextView, text: &str) {
    let caret = NSRange::new(text.encode_utf16().count(), 0);
    unsafe {
        view.setMarkedText_selectedRange_replacementRange(
            &ns(text),
            caret,
            NSRange::new(crate::util::to_usize(NSNotFound), 0),
        )
    };
}

fn type_in_source(e: &EditorController, text: &str) {
    unsafe {
        e.source()
            .insertText_replacementRange(&ns(text), NSRange::new(crate::util::to_usize(NSNotFound), 0))
    };
}

/// A key as the window gets it from the keyboard: unlike `insertText`, it merges into the undo step the view has open.
fn press_key(window: &NSWindow, character: &str) {
    let event = objc2_app_kit::NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        objc2_app_kit::NSEventType::KeyDown,
        objc2_foundation::NSPoint::new(0.0, 0.0),
        objc2_app_kit::NSEventModifierFlags::empty(),
        objc2_foundation::NSProcessInfo::processInfo().systemUptime(),
        window.windowNumber(),
        None,
        &ns(character),
        &ns(character),
        false,
        0,
    );
    if let Some(event) = event {
        window.sendEvent(&event);
    }
}

fn tail(s: &str, n: usize) -> String {
    let n = n.max(200);
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

/// A real screen capture: WebKit draws in another process, so only the window server sees its current output.
fn screen_image(window: &NSWindow, path: &Path) {
    let status = std::process::Command::new("screencapture")
        .args(["-x", "-o", &format!("-l{}", window.windowNumber())])
        .arg(path)
        .status();
    if status.is_ok_and(|s| s.success()) {
        eprintln!("SELFTEST SNAPSHOT {}", path.display());
    }
}

/// Renders the whole window including the toolbar, which the text-view snapshots leave out.
fn window_image(window: &NSWindow, path: &Path) {
    let Some(frame) = window.contentView().and_then(|v| unsafe { v.superview() }) else {
        return;
    };
    let bounds = frame.bounds();
    let Some(rep) = frame.bitmapImageRepForCachingDisplayInRect(bounds) else {
        return;
    };
    frame.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    let data = unsafe {
        rep.representationUsingType_properties(
            objc2_app_kit::NSBitmapImageFileType::PNG,
            &objc2_foundation::NSDictionary::new(),
        )
    };
    if let Some(data) = data {
        data.writeToFile_atomically(&ns(path.to_str().unwrap()), true);
        eprintln!("SELFTEST SNAPSHOT {}", path.display());
    }
}

fn append(e: &EditorController, text: &str) {
    let end = source(e).len();
    let edit = crate::markdown::commands::Edit {
        range: bytes(end..end),
        replacement: text.to_string(),
        selection: bytes(end..end),
    };
    e.editing().apply_edit(edit, None, false);
}

fn export_pdf(e: &EditorController, pdf: &Path) {
    let _ = std::fs::remove_file(pdf);
    let Some(doc) = e.document() else { return };
    let info: Retained<NSPrintInfo> = doc.printInfo().copy();
    let dict = unsafe { info.dictionary() };
    unsafe {
        dict.setObject_forKey(
            &obj(NSPrintSaveJob.copy()),
            objc2::runtime::ProtocolObject::from_ref(NSPrintJobDisposition),
        );
        dict.setObject_forKey(
            &obj(NSURL::fileURLWithPath(&ns(pdf.to_str().unwrap()))),
            objc2::runtime::ProtocolObject::from_ref(NSPrintJobSavingURL),
        );
    }
    let op = e.print_operation(&info);
    op.setShowsPrintPanel(false);
    op.setShowsProgressPanel(false);
    let ok = op.runOperation();
    let size = std::fs::metadata(pdf).map(|m| m.len()).unwrap_or(0);
    check("pdf export", ok && size > 5000, &format!("ok={ok} size={size}"));
}

fn open_formula_editor(e: &EditorController) -> Option<Retained<objc2_app_kit::NSTextView>> {
    let src = source(e);
    let formula = src.find("$E = mc^2$")? + 1;
    let map = crate::markdown::text::Utf16Map::new(&src);
    let window = e.window()?;
    window.makeFirstResponder(Some(e.source()));
    e.source()
        .setSelectedRange(NSRange::new(map.to_u16(Byte::new(formula)).get(), 0));
    e.atom_editor().edit_at_selection();
    e.atom_editor().code_view()
}

fn close_popover(e: &EditorController) {
    let popover = e.atom_editor().open_popover();
    if let Some(p) = popover {
        p.close();
    }
}

/// A file type only the self-test bundle declares; see xtask.
const PROBE_TYPE: &str = "local.splat.selftest.probe";

fn check_app_menu(mtm: MainThreadMarker) {
    use objc2_app_kit::NSControlStateValueOn;
    let app = NSApplication::sharedApplication(mtm);
    let menu = app
        .mainMenu()
        .and_then(|m| m.itemAtIndex(0))
        .and_then(|i| i.submenu())
        .unwrap();
    let state = |action: objc2::runtime::Sel| {
        menu.update();
        menu.itemArray()
            .iter()
            .find(|i| i.action() == Some(action))
            .map(|i| (i.isEnabled(), i.state() == NSControlStateValueOn))
    };
    let is_default = crate::app::opens_markdown();
    check(
        "the app menu offers to open Markdown files in Splat, ticked once it does",
        state(sel!(mdOpenMarkdownHere:)) == Some((!is_default, is_default)),
        &format!("{:?}, default {is_default}", state(sel!(mdOpenMarkdownHere:))),
    );
    let checks = menu
        .itemArray()
        .iter()
        .find(|i| i.action() == Some(sel!(checkForUpdates:)));
    check(
        "the app menu checks for updates through Sparkle",
        checks.is_some_and(|i| i.target().is_some()) && state(sel!(checkForUpdates:)) == Some((true, false)),
        &format!("{:?}", state(sel!(checkForUpdates:))),
    );
    let toggle = |action: objc2::runtime::Sel| unsafe { app.sendAction_to_from(action, None, None) };
    let was_on = crate::updates::checks_automatically();
    if was_on {
        toggle(sel!(mdToggleUpdateChecks:));
    }
    let off = (
        state(sel!(mdToggleUpdateChecks:)),
        state(sel!(mdToggleUpdateDownloads:)),
    );
    toggle(sel!(mdToggleUpdateChecks:));
    let on = (
        state(sel!(mdToggleUpdateChecks:)),
        state(sel!(mdToggleUpdateDownloads:)),
    );
    toggle(sel!(mdToggleUpdateDownloads:));
    let installing = (
        crate::updates::installs_automatically(),
        state(sel!(mdToggleUpdateDownloads:)),
    );
    toggle(sel!(mdToggleUpdateDownloads:));
    toggle(sel!(mdToggleUpdateChecks:));
    let restored = !crate::updates::checks_automatically() && !crate::updates::installs_automatically();
    if was_on {
        toggle(sel!(mdToggleUpdateChecks:));
    }
    check(
        "the update settings in the app menu switch Sparkle's options, installing only with automatic checks",
        off == (Some((true, false)), Some((false, false)))
            && on == (Some((true, true)), Some((true, false)))
            && installing == (true, Some((true, true)))
            && restored,
        &format!("off {off:?}, on {on:?}, installing {installing:?}, restored {restored}"),
    );
    // Only for a type it already opens does macOS change the default without asking the user.
    if crate::app::opens(PROBE_TYPE) {
        crate::app::make_default(PROBE_TYPE);
    }
}

fn check_about_window(mtm: MainThreadMarker) {
    #[derive(Default, Clone)]
    struct Shown {
        texts: Vec<String>,
        links: Vec<(String, String)>,
    }
    fn links(view: &objc2_app_kit::NSTextView, out: &mut Vec<(String, String)>) {
        let text = storage(view);
        let mut at = 0;
        while at < text.length() {
            let mut range = NSRange::new(at, 0);
            let value = unsafe {
                text.attribute_atIndex_effectiveRange(objc2_app_kit::NSLinkAttributeName, at, &raw mut range)
            };
            if let Some(url) = value
                .and_then(|v| v.downcast::<NSURL>().ok())
                .and_then(|u| u.absoluteString())
            {
                out.push((text.string().substringWithRange(range).to_string(), url.to_string()));
            }
            at = (at + 1).max(range.location + range.length);
        }
    }
    fn walk(view: &objc2_app_kit::NSView, out: &mut Shown) {
        if let Some(field) = view.downcast_ref::<objc2_app_kit::NSTextField>() {
            out.texts.push(field.stringValue().to_string());
        }
        if let Some(text) = view.downcast_ref::<objc2_app_kit::NSTextView>() {
            out.texts.push(text.string().to_string());
            links(text, &mut out.links);
        }
        for sub in view.subviews().iter() {
            walk(&sub, out);
        }
    }
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: the app delegate's action takes any sender.
    unsafe { app.sendAction_to_from(sel!(mdShowAbout:), None, None) };
    let panel = app.windows().iter().find_map(|window| {
        let mut shown = Shown::default();
        walk(&*window.contentView()?, &mut shown);
        shown
            .texts
            .iter()
            .any(|t| t.contains("Sven Müller"))
            .then_some((window, shown))
    });
    let bundle = objc2_foundation::NSBundle::mainBundle();
    let version = bundle
        .objectForInfoDictionaryKey(&ns("CFBundleShortVersionString"))
        .and_then(|v| v.downcast::<NSString>().ok())
        .map(|v| v.to_string())
        .unwrap_or_default();
    let acknowledgements = bundle
        .pathForResource_ofType(Some(&ns("Acknowledgements")), Some(&ns("txt")))
        .and_then(|p| std::fs::read_to_string(p.to_string()).ok())
        .unwrap_or_default();
    let shown = panel.as_ref().map(|(_, shown)| shown.clone()).unwrap_or_default();
    let repository = crate::feedback::REPOSITORY;
    let expected_links = [
        ("github.com/sventmueller/splat".to_string(), repository.to_string()),
        ("MIT".to_string(), format!("{repository}/blob/main/LICENSE-MIT")),
        (
            "Apache 2.0".to_string(),
            format!("{repository}/blob/main/LICENSE-APACHE"),
        ),
    ];
    check(
        "the About window names the version once, the copyright and the license, and the bundle carries the license \
         texts of all it contains",
        !version.is_empty()
            && shown.texts.iter().any(|t| t.ends_with(&format!(" {version}")))
            && !shown.texts.iter().any(|t| t.contains(&format!("({version})")))
            && shown.texts.iter().any(|t| t == "© 2026 Sven Müller")
            && shown.links == expected_links
            && ["## Splat\n", "\n## comrak ", "\n## mathjax ", "\n## Sparkle "]
                .iter()
                .all(|part| acknowledgements.contains(part)),
        &format!(
            "version {version}, texts {:?}, links {:?}, {} bytes of license texts",
            shown.texts,
            shown.links,
            acknowledgements.len()
        ),
    );
    if let Some((window, _)) = panel {
        window.close();
    }
}

fn check_default_probe(mtm: MainThreadMarker) {
    let modal = NSApplication::sharedApplication(mtm).modalWindow();
    check(
        "making the app the default for a type it opens needs no confirmation and shows no error",
        crate::app::opens(PROBE_TYPE) && modal.is_none(),
        &format!(
            "opens {}, modal window {}",
            crate::app::opens(PROBE_TYPE),
            modal.is_some()
        ),
    );
}

fn check_context_menu(e: &EditorController) {
    let menu: Option<Retained<objc2_app_kit::NSMenu>> =
        unsafe { objc2::msg_send![<objc2_app_kit::NSTextView as objc2::ClassType>::class(), defaultMenu] };
    let menu = menu.map(|m| m.copy()).unwrap();
    let p = storage(e.preview()).string().to_string();
    let formula = p[..p.find("Inline-Formel: ").unwrap()].encode_utf16().count() + "Inline-Formel: ".len();
    let adjusted = e.editing().adjust_context_menu(&menu, e.preview(), formula);
    let titles: Vec<String> = adjusted.itemArray().iter().map(|i| i.title().to_string()).collect();
    check(
        "context menu on a formula offers to edit it",
        titles.iter().any(|t| t == "Formel bearbeiten …"),
        &format!("{titles:?}"),
    );
    let font_menu = adjusted.itemArray().iter().any(|i| {
        i.submenu().is_some_and(|s| {
            s.itemArray()
                .iter()
                .any(|x| x.action() == Some(sel!(orderFrontFontPanel:)))
        })
    });
    check(
        "context menu offers Markdown commands",
        titles.iter().any(|t| t == "Fett") && !font_menu,
        &format!("{titles:?}"),
    );
}

fn finish() {
    let failures = FAILURES.with(Cell::get);
    if let Some(mtm) = MainThreadMarker::new()
        && !NSApplication::sharedApplication(mtm).isActive()
    {
        // An inactive app does not close popovers, so the formula and diagram editor checks fail on their own.
        eprintln!("SELFTEST WARN the app was not active (screen locked or another app in front)");
    }
    if let (Ok(file), Ok(_)) = (
        std::env::var("SPLAT_SELFTEST_FILE"),
        std::env::var("SPLAT_SELFTEST_OUT"),
    ) {
        forget_web_image_permission(&file);
    }
    eprintln!("SELFTEST DONE failures={failures}");
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
