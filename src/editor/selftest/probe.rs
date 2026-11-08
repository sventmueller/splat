//! What needs the app but no visible window: rendering, remote images, exports, the sharing menu and what injected
//! content can do in the formula editor. Every result is a check, so a broken one fails the run.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSAttachmentAttributeName, NSBitmapImageFileType, NSBitmapImageRep, NSDocumentChangeType,
    NSDocumentController, NSImage, NSPasteboard, NSPasteboardTypePNG, NSTextAttachment,
};
use objc2_foundation::{
    NSData, NSDictionary, NSError, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize, NSString, NSURL,
};
use objc2_web_kit::{WKURLSchemeHandler, WKURLSchemeTask, WKWebView};

use super::super::EditorController;
use super::super::decoder;
use super::super::images::ImageStore;
use super::super::js::{self, Key, PageAnswer};
use super::super::mathfield::{self, MathField};
use super::super::picture::{Rendered, Sizing};
use super::super::render::{Drawn, RenderCtx};
use super::super::safe_image::Decoded;
use super::super::webkit;
use super::mock_web::{HOST, requests_of};
use super::{after, check, finish, guarded, once};
use crate::document::MdDocument;
use crate::markdown::doc::DiagramKind;
use crate::markdown::reader::parse;
use crate::preview::layout::{Resolver, layout};
use crate::util::{constant, ns};

/// How long rendering, the reload and the downloads may take together.
const PATIENCE: f64 = 45.0;

const REMOTE_PICTURES: [&str; 12] = [
    "/badge.svg",
    "/fehlt.png",
    "/kein-bild.png",
    "/zu-gross.png",
    "/umleitung.png",
    "/umleitung-nach-hause.png",
    "/umleitung-weiter.svg",
    "/umleitung-ins-leere.svg",
    "/umleitung-zaeh.svg",
    "/wackelig.svg",
    "/ohne-netz.svg",
    "/nicht-gezeigt.svg",
];

fn url_in_probe(path: &str) -> String {
    format!("{HOST}{path}")
}

/// A scheme in capitals is the same scheme.
fn at_home() -> String {
    format!("{}/router.png", super::mock_web::HOME_HOST)
}

fn in_capitals() -> String {
    url_in_probe("/badge.svg").replacen("https", "HTTPS", 1)
}

struct Probe {
    mtm: MainThreadMarker,
    keys: Vec<(&'static str, Key)>,
    document: Retained<MdDocument>,
    editor: Option<Retained<EditorController>>,
    local: PathBuf,
    changed: PathBuf,
    changed_url: Retained<NSURL>,
    field: std::rc::Rc<MathField>,
}

/// What the preview shows but does not make comes only from what rendering is given.
fn rendering_takes_what_it_is_given(mtm: MainThreadMarker) {
    struct Given {
        picture: Rendered,
        asked_for_web: Cell<usize>,
    }
    impl Drawn for Given {
        fn web_picture(&self, _: &str) -> Option<Rendered> {
            self.asked_for_web.set(self.asked_for_web.get() + 1);
            Some(self.picture.clone())
        }
        fn drawing(&self, _: &Key) -> Option<Rendered> {
            Some(self.picture.clone())
        }
    }
    struct Ready;
    impl Resolver for Ready {
        fn math_ready(&self, _: &str, _: bool) -> bool {
            true
        }
        fn diagram_ready(&self, _: &str, _: DiagramKind) -> bool {
            true
        }
    }
    let size = NSSize::new(4.0, 4.0);
    let given = Given {
        picture: Rendered {
            image: NSImage::initWithSize(NSImage::alloc(), size),
            sizing: Sizing::Points { size, descent: 0.0 },
        },
        asked_for_web: Cell::new(0),
    };
    let shown = layout(&parse("$x$ ![a](https://example.com/a.png)\n"), &Ready, None);
    let images = RefCell::new(ImageStore::default());
    let pictures_given = |remote_images: bool| {
        let ctx = RenderCtx {
            mtm,
            zoom: 1.0,
            doc_dir: None,
            images: &images,
            max_image_width: 400.0,
            dark: false,
            controller: None,
            remote_images,
            leaves_the_mac: false,
            macros: std::sync::Arc::from(""),
            drawn: &given,
        };
        let text = ctx.render_block(&shown.blocks[0]);
        let mut found = 0;
        let mut at = 0;
        while at < text.length() {
            let mut range = NSRange::new(0, 0);
            // SAFETY: `at` lies inside the text and `range` outlives the call.
            let value = unsafe { text.attribute_atIndex_effectiveRange(NSAttachmentAttributeName, at, &mut range) };
            if let Some(attachment) = value.and_then(|v| v.downcast::<NSTextAttachment>().ok())
                && attachment
                    .image()
                    .is_some_and(|image| std::ptr::eq(&*image, &*given.picture.image))
            {
                found += 1;
            }
            at = (range.location + range.length).max(at + 1);
        }
        found
    };
    let without_web = pictures_given(false);
    let asked_without = given.asked_for_web.get();
    let with_web = pictures_given(true);
    check(
        "the preview shows formulas and web pictures from what it is given, and asks for web pictures only when allowed",
        (without_web, asked_without, with_web, given.asked_for_web.get()) == (1, 0, 2, 1),
        &format!("without web {without_web} pictures, {asked_without} asked; with web {with_web}"),
    );
}

pub(super) fn run(mtm: MainThreadMarker) {
    rendering_takes_what_it_is_given(mtm);
    undo_is_back_on_after_a_panic(mtm);
    another_thread_gets_an_answer_from_the_main_thread();
    let name = "a document without formulas or diagrams starts no web view";
    // SAFETY: NSDocument's initializer, called once on the newly allocated object.
    let document: Retained<MdDocument> = unsafe { objc2::msg_send![MdDocument::alloc(mtm), init] };
    document.set_initial_text("Nur Text\n");
    document.makeWindowControllers();
    let Some(editor) = document.editor() else {
        check(name, false, "no editor");
        with_web_views_started(mtm);
        return;
    };
    let shown = editor.clone();
    once(
        2.0,
        "the document without formulas to show",
        move || super::quiet(&shown),
        move || {
            check(name, !js::started() && !mathfield::started(), "");
            super::type_in_source(&editor, "$x^2$\n\n");
            let rendered = editor.clone();
            let (editor, document) = (editor.clone(), document.clone());
            once(
                0.0,
                "the first formula of the document to render",
                move || js::started() && super::settled(&rendered),
                move || {
                    check(
                        "the first web view loads its page in a later turn of the main loop",
                        !js::had_its_page_at_once(),
                        "",
                    );
                    check(
                        "the first formula starts the web view and shows",
                        crate::util::storage(editor.preview())
                            .string()
                            .to_string()
                            .contains('\u{FFFC}'),
                        "no picture in the preview",
                    );
                    document.updateChangeCount(NSDocumentChangeType::ChangeCleared);
                    document.close();
                    with_web_views_started(mtm);
                },
            );
        },
    );
}

/// An undo manager belongs to the main thread, so this runs here and not in a unit test.
fn undo_is_back_on_after_a_panic(mtm: MainThreadMarker) {
    let undo = objc2_foundation::NSUndoManager::new(mtm);
    let off_inside = Cell::new(false);
    let panicked = crate::util::contained(|| {
        crate::util::without_undo(Some(&undo), || {
            off_inside.set(!undo.isUndoRegistrationEnabled());
            panic!("a panic the checks provoke");
        });
    })
    .is_none();
    let back_on = undo.isUndoRegistrationEnabled();
    check(
        "undo is back on when what ran without it panics",
        panicked && off_inside.get() && back_on && crate::util::without_undo(None, || 1) == 1,
        &format!(
            "panicked {panicked}, off inside {}, on after {back_on}",
            off_inside.get()
        ),
    );
}

/// `on_main` from a thread the main thread does not wait for, as the Word export calls it for pictures.
fn another_thread_gets_an_answer_from_the_main_thread() {
    let answer = std::sync::Arc::new(std::sync::Mutex::new(None));
    let given = answer.clone();
    std::thread::spawn(move || {
        let on_main = crate::util::on_main(|| MainThreadMarker::new().is_some());
        *given.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(on_main);
    });
    let asked = answer.clone();
    let lock = move || *asked.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let heard = lock.clone();
    once(
        0.0,
        "the answer of the main thread to another thread",
        move || heard().is_some(),
        move || {
            check(
                "another thread gets the answer of work it hands to the main thread",
                lock() == Some(Some(true)),
                &format!("{:?}", lock()),
            );
        },
    );
}

fn with_web_views_started(mtm: MainThreadMarker) {
    let keys = render_keys();
    js::renderer(mtm).request(
        super::super::render_queue::Owner::unique(),
        keys.iter().map(|(_, k)| k.clone()).collect(),
    );
    let field = mathfield::field(mtm);
    let local = std::env::temp_dir().join(format!("splat-probe-{}.bmp", std::process::id()));
    let _ = std::fs::write(&local, two_pixel_bmp());
    // SAFETY: NSDocument's initializer, called once on the newly allocated object.
    let document: Retained<MdDocument> = unsafe { objc2::msg_send![MdDocument::alloc(mtm), init] };
    document.set_initial_text(&format!(
        "# Probe\n\n## Teil\n\n![a](https://192.168.0.1/x.png) ![b](https://localhost/y.png)\n\n![c](https://intranet/z.png)\n\n![w]({0}) ![n]({1}) ![u]({2})\n\n![lokal](<{3}>)\n\nFormel $E = mc^2$\n\n| a | b |\n|---|---|\n| 1 | 2 |",
        url_in_probe("/wackelig.svg"),
        url_in_probe("/ohne-netz.svg"),
        url_in_probe("/umleitung-ins-leere.svg"),
        local.display()
    ));
    document.makeWindowControllers();
    sandbox_of_the_helper();
    blocked_addresses();
    // With a document asking, which keeps them in memory and tries failures again.
    let editor = document.editor();
    for path in REMOTE_PICTURES {
        super::super::remote::get(&url_in_probe(path), editor.as_deref());
    }
    super::super::remote::get(&in_capitals(), editor.as_deref());
    super::super::remote::get(&at_home(), editor.as_deref());
    let documents = NSDocumentController::sharedDocumentController(mtm);
    let recent = std::env::temp_dir().join("splat-probe-recent.md");
    let _ = std::fs::write(&recent, "# Recent\n");
    documents.noteNewRecentDocumentURL(&NSURL::fileURLWithPath(&ns(recent.to_str().unwrap_or_default())));
    let (changed, changed_url) = open_and_change_after_saving(mtm);
    let probe = std::rc::Rc::new(Probe {
        mtm,
        keys,
        editor,
        document,
        local,
        changed,
        changed_url,
        field,
    });
    wait_until_done(probe, std::time::Instant::now());
}

fn render_keys() -> Vec<(&'static str, Key)> {
    vec![
        (
            "a formula",
            Key::Math {
                tex: "E = mc^2".into(),
                display: false,
                dark: false,
                macros: "".into(),
            },
        ),
        (
            "a Mermaid diagram",
            Key::Diagram {
                source: "flowchart LR\n    A[Markdown] --> B[PDF]".into(),
                kind: DiagramKind::Mermaid,
                dark: false,
            },
        ),
        (
            "a Graphviz diagram",
            Key::Diagram {
                source: "digraph { Markdown -> PDF; Markdown -> HTML }".into(),
                kind: DiagramKind::Graphviz,
                dark: true,
            },
        ),
        (
            "a PlantUML diagram",
            Key::Diagram {
                source: "Alice -> Bob : Hallo\nBob --> Alice : Antwort\n".into(),
                kind: DiagramKind::PlantUml,
                dark: false,
            },
        ),
    ]
}

/// A 2 × 1 BMP, a format the system decoder would accept as well.
fn two_pixel_bmp() -> Vec<u8> {
    let mut bmp = b"BM".to_vec();
    for v in [62u32, 0, 54, 40, 2, 1] {
        bmp.extend_from_slice(&v.to_le_bytes());
    }
    bmp.extend_from_slice(&[1, 0, 24, 0]);
    bmp.extend_from_slice(&[0; 24]);
    bmp.extend_from_slice(&[0, 0, 255, 255, 0, 0, 0, 0]);
    bmp
}

fn sandbox_of_the_helper() {
    let secret = std::env::temp_dir().join(format!("splat-probe-secret-{}", std::process::id()));
    let _ = std::fs::write(&secret, "secret");
    for (name, arg) in [
        ("the decoder helper", super::super::decoder::ARG),
        (
            "the helper for the system's decoders",
            super::super::decoder::NATIVE_ARG,
        ),
    ] {
        let escaped = super::super::decoder::self_check(arg, secret.to_str().unwrap_or_default());
        check(
            &format!("{name} reads no files and reaches no network"),
            escaped == Some(false),
            &format!("{escaped:?}"),
        );
    }
    let _ = std::fs::remove_file(&secret);
}

fn blocked_addresses() {
    let blocked = [
        "https://192.168.0.1/x.png",
        "https://localhost/y.png",
        "https://intranet/z.png",
    ]
    .iter()
    .filter(|u| matches!(super::super::remote::get(u, None), Some(None)))
    .count();
    check(
        "images on private hosts count as failed at once",
        blocked == 3,
        &format!("{blocked} of 3"),
    );
}

/// Another program changes an open file right after Splat saved it, without file coordination, as Git or a shell would:
/// the hardest case, since macOS reports no change of its own then.
fn open_and_change_after_saving(mtm: MainThreadMarker) -> (PathBuf, Retained<NSURL>) {
    let changed = std::env::temp_dir().join(format!("splat-probe-reload-{}.md", std::process::id()));
    let _ = std::fs::write(&changed, "# Vorher\n");
    let done = crate::util::open_handler(|_, _, _| {});
    let url = NSURL::fileURLWithPath(&ns(changed.to_str().unwrap_or_default()));
    NSDocumentController::sharedDocumentController(mtm)
        .openDocumentWithContentsOfURL_display_completionHandler(&url, false, &done);
    let (opened, rewrite) = (url.clone(), changed.clone());
    let asked = url.clone();
    once(
        0.0,
        "the document to reload to open",
        move || {
            NSDocumentController::sharedDocumentController(mtm)
                .documentForURL(&asked)
                .is_some()
        },
        move || {
            let document = NSDocumentController::sharedDocumentController(mtm)
                .documentForURL(&opened)
                .and_then(|d| d.downcast::<MdDocument>().ok());
            if let Some(document) = document {
                document.makeWindowControllers();
                document.updateChangeCount(NSDocumentChangeType::ChangeDone);
                // SAFETY: NSDocument's action method, called on the main thread without a sender.
                unsafe { document.saveDocument(None) };
                write_once_saved(mtm, opened.clone(), rewrite.clone());
            }
        },
    );
    (changed, url)
}

/// Writes new text to `path` once the document at `url` has been saved.
fn write_once_saved(mtm: MainThreadMarker, url: Retained<NSURL>, path: PathBuf) {
    let edited = NSDocumentController::sharedDocumentController(mtm)
        .documentForURL(&url)
        .is_none_or(|d| d.isDocumentEdited());
    if edited {
        after(0.05, move || write_once_saved(mtm, url.clone(), path.clone()));
    } else {
        let _ = std::fs::write(&path, "# Nachher\n");
    }
}

impl Probe {
    fn reloaded_document(&self) -> Option<Retained<MdDocument>> {
        NSDocumentController::sharedDocumentController(self.mtm)
            .documentForURL(&self.changed_url)
            .and_then(|d| d.downcast::<MdDocument>().ok())
    }

    fn reloaded_text(&self) -> Option<String> {
        self.reloaded_document().map(|d| d.text())
    }

    fn local_image_shown(&self) -> Option<bool> {
        self.editor
            .as_ref()
            .and_then(|e| e.pictures().store().borrow().shown(&self.local))
    }

    /// Everything the checks below look at has happened.
    fn done(&self) -> bool {
        self.reloaded_text().as_deref() == Some("# Nachher\n")
            && self.keys.iter().all(|(_, key)| js::outcome(key).is_some())
            && self.local_image_shown().is_some()
            && self.field.loaded.get()
            && super::super::remote::idle()
            && answered("/wackelig.svg")
            && answered("/ohne-netz.svg")
            && answered("/umleitung-ins-leere.svg")
    }
}

/// A failure that passes is tried again, by the timer that also watches files.
fn answered(path: &str) -> bool {
    super::super::remote::try_failed_again();
    matches!(super::super::remote::get(&url_in_probe(path), None), Some(Some(_)))
}

/// Asks for a picture while the slow name is being looked up, and ends the lookup once that picture is in.
fn meanwhile_another_download(editor: Option<&EditorController>) {
    if super::mock_web::slow_name_asked()
        && matches!(
            super::super::remote::get(&url_in_probe("/waehrend-der-suche.svg"), editor),
            Some(Some(_))
        )
    {
        super::mock_web::free_slow_name();
    }
}

fn wait_until_done(probe: std::rc::Rc<Probe>, started: std::time::Instant) {
    meanwhile_another_download(probe.editor.as_deref());
    if probe.done() || started.elapsed().as_secs_f64() > PATIENCE {
        report(&probe);
    } else {
        after(0.1, move || wait_until_done(probe.clone(), started));
    }
}

fn report(probe: &Probe) {
    let reloaded = probe.reloaded_text();
    check(
        "a file changed right after Splat saved it reloads",
        reloaded.as_deref() == Some("# Nachher\n"),
        &format!("{reloaded:?}"),
    );
    check(
        "a saved and reloaded document holds its text in its editor only",
        probe.reloaded_document().is_some_and(|d| !d.keeps_a_copy_of_its_text()),
        "",
    );
    let _ = std::fs::remove_file(&probe.changed);
    let recent: Vec<String> = NSDocumentController::sharedDocumentController(probe.mtm)
        .recentDocumentURLs()
        .iter()
        .filter_map(|u| u.path().map(|p| p.to_string()))
        .collect();
    check(
        "the recent documents list a document just noted",
        recent.iter().any(|p| p.ends_with("splat-probe-recent.md")),
        &format!("{recent:?}"),
    );
    for (name, key) in &probe.keys {
        let outcome = js::outcome(key);
        check(
            &format!("{name} renders"),
            outcome == Some(Ok(())),
            &format!("{outcome:?}"),
        );
        write_picture(name, key);
    }
    guarded("the remote answers", remote_answers);
    check(
        "a local image is decoded in a helper",
        probe.local_image_shown() == Some(true),
        "",
    );
    if let Some(e) = &probe.editor {
        guarded("the exports", || exports(e));
        guarded("the sharing menu", || sharing(e, probe.mtm));
    }
    let _ = std::fs::remove_file(&probe.local);
    if let Some(e) = &probe.editor {
        guarded("the outline and web permission", || outline_and_web_permission(e));
        guarded("pasting a picture", || pasting_a_picture(e, &probe.document));
    }
    guarded("a closed document", || a_closed_document_stops_asking(probe.mtm));
    super::controller::checks(probe.mtm);
    formula_editor(probe);
}

/// A document decides which hosts hear from Splat, so one that closes takes back what it still waits for.
fn a_closed_document_stops_asking(mtm: MainThreadMarker) {
    const IMAGES: usize = 40;
    let text: String = (0..IMAGES)
        .map(|n| {
            format!(
                "![{n}]({})\n\n",
                url_in_probe(&format!("{}-{n}.svg", super::mock_web::SLOW))
            )
        })
        .collect();
    let (document, e) = super::documents::untitled(mtm, &text);
    e.pictures().allow_remote_images();
    let asked = super::controller::until(|| super::mock_web::requests_holding(super::mock_web::SLOW) > 0);
    document.updateChangeCount(objc2_app_kit::NSDocumentChangeType::ChangeCleared);
    e.stop_showing_for_a_check();
    document.close();
    let quiet = super::controller::until(js_free_of_downloads);
    let requests = super::mock_web::requests_holding(super::mock_web::SLOW);
    check(
        "a closed document stops asking hosts for the images it waited for",
        asked && quiet && requests < IMAGES,
        &format!("asked {asked}, ended {quiet}, {requests} of {IMAGES} requests"),
    );
}

fn js_free_of_downloads() -> bool {
    super::super::remote::idle()
}

/// What a person compares by eye after changing a renderer.
fn write_picture(name: &str, key: &Key) {
    let png = js::cached(key)
        .and_then(|r| r.image.TIFFRepresentation())
        .and_then(|tiff| NSBitmapImageRep::imageRepWithData(&tiff))
        .and_then(|rep| {
            // SAFETY: The dictionary of properties is empty.
            unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
        });
    let path = std::env::temp_dir().join(format!("splat-probe-{}.png", name.replace(' ', "-")));
    if let Some(png) = png {
        png.writeToFile_atomically(&ns(path.to_str().unwrap_or_default()), true);
    }
}

fn remote_answers() {
    let shown = |path: &str| matches!(super::super::remote::get(&url_in_probe(path), None), Some(Some(_)));
    let failed = |path: &str| matches!(super::super::remote::get(&url_in_probe(path), None), Some(None));
    check("a remote picture loads", shown("/badge.svg"), "");
    check(
        "a remote picture that is missing counts as failed",
        failed("/fehlt.png"),
        "",
    );
    check(
        "a remote answer that is no picture counts as failed",
        failed("/kein-bild.png"),
        "",
    );
    check(
        "a remote picture over the size limit is not loaded",
        failed("/zu-gross.png"),
        "",
    );
    check(
        "a redirect to a private host is not followed",
        failed("/umleitung.png"),
        "",
    );
    check(
        "nothing reaches the private host a redirect names",
        requests_of("https://127.0.0.1/heimlich.png") == 0,
        "",
    );
    check(
        "a redirect to a public name that leads into the local network is not followed",
        failed("/umleitung-nach-hause.png"),
        "",
    );
    check(
        "a redirect to a public host is followed",
        shown("/umleitung-weiter.svg"),
        "",
    );
    check(
        "a name that is slow to resolve after a redirect holds up no other download",
        !super::mock_web::slow_name_waited_in_vain()
            && shown("/waehrend-der-suche.svg")
            && shown("/umleitung-zaeh.svg"),
        "",
    );
    check(
        "a redirect to a name that does not resolve passes: the picture loads on the next try",
        shown("/umleitung-ins-leere.svg"),
        "",
    );
    check(
        "a public name that leads into the local network is not asked",
        matches!(super::super::remote::get(&at_home(), None), Some(None))
            && super::mock_web::requests_holding("router.splat-test.example") == 0,
        "",
    );
    check(
        "a server error passes: the picture loads on the next try",
        shown("/wackelig.svg"),
        "",
    );
    check(
        "an address with its scheme in capitals loads",
        matches!(super::super::remote::get(&in_capitals(), None), Some(Some(_))),
        "",
    );
    check(
        "a lost connection passes: the picture loads on the next try",
        shown("/ohne-netz.svg"),
        "",
    );
    check(
        "a failure no document shows any more is not asked for again",
        failed("/nicht-gezeigt.svg") && requests_of(&url_in_probe("/nicht-gezeigt.svg")) == 1,
        &requests_of(&url_in_probe("/nicht-gezeigt.svg")).to_string(),
    );
    check(
        "a final failure is not asked for again",
        requests_of(&url_in_probe("/fehlt.png")) == 1,
        &requests_of(&url_in_probe("/fehlt.png")).to_string(),
    );
}

fn exports(e: &EditorController) {
    let page = e
        .export_source()
        .beside(std::env::temp_dir())
        .html("Probe", &std::env::temp_dir().join("probe.html"));
    check(
        "the HTML export embeds the local image and the formula",
        page.matches("src=\"data:image/png;base64,").count() >= 2 && page.contains("class=\"math\""),
        "",
    );
    check("the HTML export holds no script", !page.contains("<script"), "");
    let word = e
        .export_source()
        .beside(std::env::temp_dir())
        .word("Probe")
        .expect("a small document");
    let listing = String::from_utf8_lossy(&word);
    check(
        "the Word export holds its document and pictures",
        listing.contains("word/document.xml") && listing.contains("word/media/"),
        &format!("{} bytes", word.len()),
    );
    // The 2 × 1 px image shrinks with the text to 0.75 pt per pixel, on paper and in Word.
    let the_image_on_paper = |doc_dir: PathBuf| {
        let printed = e.sharing().printable_from(Some(doc_dir), 400.0);
        let mut sizes = Vec::new();
        let mut at = 0;
        while at < printed.length() {
            let mut range = NSRange::new(0, 0);
            // SAFETY: `at` lies inside the text and `range` outlives the call.
            let value = unsafe { printed.attribute_atIndex_effectiveRange(NSAttachmentAttributeName, at, &mut range) };
            if let Some(attachment) = value.and_then(|v| v.downcast::<NSTextAttachment>().ok()) {
                sizes.push(attachment.bounds().size);
            }
            at = (range.location + range.length).max(at + 1);
        }
        sizes
            .iter()
            .any(|s| (s.width - 1.5).abs() < 0.01 && (s.height - 0.75).abs() < 0.01)
    };
    check(
        "an image is scaled like the text on paper",
        the_image_on_paper(std::env::temp_dir()),
        "",
    );
    let elsewhere = std::env::temp_dir().join(format!("splat-probe-anderswo-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&elsewhere);
    check(
        "paper takes along no image from outside the folder of its document",
        !the_image_on_paper(elsewhere.clone()),
        "",
    );
    let _ = std::fs::remove_dir_all(elsewhere);
    check(
        "an image is scaled like the text in Word",
        listing.contains("<wp:extent cx=\"19050\" cy=\"9525\"/>"),
        "",
    );
}

fn sharing(e: &EditorController, mtm: MainThreadMarker) {
    let titles: Vec<String> = e
        .sharing()
        .export_sharing_services()
        .iter()
        .map(|s| s.title().to_string())
        .collect();
    check(
        "the share menu offers the three exports",
        titles
            == [
                "Als PDF exportieren …",
                "Als HTML exportieren …",
                "Als Word-Dokument exportieren …",
            ],
        &format!("{titles:?}"),
    );
    let file_menu: Vec<String> = NSApplication::sharedApplication(mtm)
        .mainMenu()
        .and_then(|m| m.itemAtIndex(1))
        .and_then(|i| i.submenu())
        .map(|m| m.itemArray().iter().map(|i| i.title().to_string()).collect())
        .unwrap_or_default();
    check(
        "the share item stands in the file menu once, right before the exports",
        file_menu.iter().filter(|t| *t == "Teilen").count() == 1
            && file_menu
                .windows(2)
                .any(|w| w[0] == "Teilen" && w[1] == "Als PDF exportieren …"),
        &format!("{file_menu:?}"),
    );
    let help_menu = NSApplication::sharedApplication(mtm).helpMenu();
    if let Some(menu) = &help_menu {
        menu.update();
    }
    let enabled = |action: objc2::runtime::Sel| {
        help_menu.as_ref().is_some_and(|menu| {
            menu.itemArray()
                .iter()
                .any(|i| i.action() == Some(action) && i.isEnabled())
        })
    };
    check(
        "the help menu leads to the problem form, the feature form and the repository",
        enabled(objc2::sel!(mdReportProblem:))
            && enabled(objc2::sel!(mdSuggestFeature:))
            && enabled(objc2::sel!(mdOpenRepository:)),
        "",
    );
    let form = crate::feedback::problem_form();
    check(
        "the problem form names the version of Splat and of macOS",
        !form.contains("version=&") && !form.contains("macos=0."),
        &form,
    );
    let opened = |open: fn()| {
        open();
        crate::test_hooks::LAST_OPENED
            .lock()
            .map(|address| address.clone())
            .unwrap_or_default()
    };
    check(
        "the help menu's commands lead where they say",
        opened(crate::feedback::report_problem) == form
            && opened(crate::feedback::suggest_feature).ends_with("/discussions/new?category=ideas")
            && opened(crate::feedback::show_repository).ends_with("/sventmueller/splat"),
        "",
    );
}

fn outline_and_web_permission(e: &EditorController) {
    e.panes().toggle_outline();
    let rows = e.panes().outline_rows();
    check(
        "the outline lists two nested headings",
        rows == Some(2),
        &format!("{rows:?}"),
    );
    let banner = |e: &EditorController| e.pictures().banner_shown();
    check(
        "web images wait for permission",
        banner(e) == Some(true),
        &format!("{:?}", banner(e)),
    );
    e.pictures().allow_remote_images();
    check(
        "allowing web images hides the question",
        banner(e) == Some(false),
        &format!("{:?}", banner(e)),
    );
}

/// Pasting a picture writes it next to the document and links it.
fn pasting_a_picture(e: &EditorController, document: &MdDocument) {
    let dir = std::env::temp_dir().join(format!("splat-probe-paste-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    document.setFileURL(Some(&NSURL::fileURLWithPath(&ns(in_dir(&dir, "Probe.md")
        .to_str()
        .unwrap_or_default()))));
    let mut png = Vec::new();
    let _ = image::RgbaImage::new(2, 2).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    let pasteboard = NSPasteboard::pasteboardWithUniqueName();
    pasteboard.clearContents();
    pasteboard.setData_forType(Some(&NSData::with_bytes(&png)), constant!(NSPasteboardTypePNG));
    let handled = e.paste_image(&pasteboard);
    super::controller::until(|| e.source_text().contains("![Bild "));
    let written = std::fs::read_dir(&dir).map_or(0, |d| {
        d.flatten()
            .filter(|f| f.path().extension().is_some_and(|x| x == "png"))
            .count()
    });
    check(
        "a pasted picture lies next to the document and is linked",
        handled && written == 1 && e.source_text().contains("![Bild "),
        &format!("handled {handled}, files {written}"),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn in_dir(dir: &Path, name: &str) -> PathBuf {
    dir.join(name)
}

/// What injected content would try. A `data:` address needs no network and has no origin for CORS to refuse, so only the
/// page's policy can stop this fetch; and the image is known to fail, so a handler that stays silent was stopped as well.
const INJECTED: &str = "const violated = new Set(); \
    document.addEventListener('securitypolicyviolation', (e) => violated.add(e.effectiveDirective)); \
    let net; \
    try { await (await fetch('data:text/plain,splat')).text(); net = 'fetch allowed'; } catch (e) { net = 'fetch blocked'; } \
    document.body.insertAdjacentHTML('beforeend', '<img id=\"splat-probe\" src=\"data:,\" onerror=\"window.pwned = 1\">'); \
    const image = document.getElementById('splat-probe'); \
    const failed = await new Promise((done) => { \
        image.addEventListener('error', () => done('failed')); \
        setTimeout(() => done('did not fail'), 5000); \
    }); \
    await new Promise((done) => setTimeout(done, 100)); \
    const said = [net, 'image ' + failed, 'inline handler ' + (window.pwned ? 'ran' : 'blocked'), \
        'violated ' + [...violated].sort().join()].join(' / ');";

/// Leads a page to a file beside it, which takes no network either, so only the navigation policy can keep it there. Nothing
/// reports a navigation that did not happen, so it gets a moment.
const LEAVE_THE_PAGE: &str = "const name = () => location.pathname.split('/').pop(); \
    const page = name(); \
    location.href = page === 'render.html' ? 'mathfield.html' : 'render.html'; \
    await new Promise((done) => setTimeout(done, 1500)); \
    return said + ' / ' + (name() === page ? 'stayed' : 'left');";

const STOPPED: &str =
    "fetch blocked / image failed / inline handler blocked / violated connect-src,script-src-attr / stayed";

/// What MathJax makes of text outside its fonts: letters that join or run from right to left must stay one piece,
/// while text it has glyphs for stays in its own font.
const FORMULA_TEXT: &str = "const svg = async (tex) => (await renderMath(tex, false, '#000')).svg; \
    const pieces = (svg) => svg.split('<text').length - 1; \
    const hebrew = await svg('\\\\text{שלום}'); \
    const arabic = await svg('\\\\textbf{العربية}'); \
    const latin = await svg('\\\\text{abc}'); \
    return [hebrew.includes('>שלום</text>'), arabic.includes('>العربية</text>'), arabic.includes('bold'), pieces(latin)].join(' / ');";

/// MathJax 4 finds its packages and fonts in the bundle, and a formula stays one picture of its own width.
const FORMULA_PACKAGES: &str = "const render = (tex) => renderMath(tex, false, '#000'); \
    const chemistry = await render('\\\\ce{CO2 -> CO}'); \
    const required = await render('\\\\require{physics}\\\\dv{f}{x}'); \
    const broken = await render('a\\\\\\\\b'); \
    const long = await render('x+'.repeat(300) + 'x'); \
    return [chemistry.svg.includes('<text'), required.error, broken.w < 10, long.w > 400].join(' / ');";

/// The lines of a table without content, in both colors of the renderer, and a formula that is an error.
const FORMULA_STYLES: &str = "const svg = async (tex, color) => (await renderMath(tex, false, color)).svg; \
    const table = '\\\\begin{array}{|c|}\\\\hline\\\\phantom{x}\\\\\\\\\\\\hline\\\\end{array}'; \
    return [await svg(table, '#1D1D1F'), await svg(table, '#E8E8E8'), await svg('\\\\frac{a', '#1D1D1F')].join('@@@');";

/// Of a formula's picture, the share of pixels that are drawn, and of those the share that is dark.
fn drawn_and_dark(svg: &str) -> Option<(f64, f64)> {
    let Some(Decoded::Rgba { pixels, .. }) = decoder::svg_to_pixels(svg, crate::editor::safe_image::SHOWN_SIDE) else {
        return None;
    };
    let (pixels, _) = pixels.as_chunks::<4>();
    let drawn: Vec<_> = pixels.iter().filter(|[.., alpha]| *alpha > 128).collect();
    let dark = drawn
        .iter()
        .filter(|[r, g, b, _]| [r, g, b].iter().all(|&&c| c < 64))
        .count();
    Some((
        drawn.len() as f64 / pixels.len().max(1) as f64,
        dark as f64 / drawn.len().max(1) as f64,
    ))
}

thread_local! {
    /// The last script asked, so that only the watch on the one still unanswered fires.
    static ASKED: Cell<u32> = const { Cell::new(0) };
}

/// One script in a web view gets this long to answer, on a machine that may be busy with anything.
const ANSWER_PATIENCE: f64 = 30.0;

/// Runs `script` in `web` and hands on what it returned or the message of its failure. A script that does not answer is a
/// failed check that names it, and ends the run: what stands behind it follows only from its answer.
fn ask(web: &WKWebView, name: &'static str, script: &str, answered: impl Fn(String) + 'static) {
    let number = ASKED.get() + 1;
    ASKED.set(number);
    after(ANSWER_PATIENCE, move || {
        if ASKED.get() == number {
            check(name, false, &format!("no answer within {ANSWER_PATIENCE} s"));
            finish();
        }
    });
    webkit::call_script(web, script, &[], move |result| {
        ASKED.set(number + 1);
        let text = match result {
            Ok(value) => value
                .and_then(|v| v.downcast_ref::<NSString>())
                .map_or_else(String::new, ToString::to_string),
            Err(message) => format!("error {message}"),
        };
        guarded(name, || answered(text.clone()));
    });
}

fn formula_editor(probe: &Probe) {
    check("the formula editor's page loads", probe.field.loaded.get(), "");
    let web = probe.field.web.clone();
    let object: &objc2::runtime::NSObject = &web;
    let background = objc2_foundation::NSObjectNSKeyValueCoding::valueForKey(object, &ns(mathfield::DRAWS_BACKGROUND));
    check(
        "the formula editor's web view draws no background",
        background
            .and_then(|value| value.downcast::<objc2_foundation::NSNumber>().ok())
            .is_some_and(|value| !value.boolValue()),
        "",
    );
    let mtm = probe.mtm;
    let target = web.clone();
    let editor = probe.editor.clone();
    ask(
        &web,
        "the formula editor's script runs",
        "return typeof MathfieldElement + '/' + typeof setLatex + '/' + (document.getElementById('mf') instanceof MathfieldElement);",
        move |script| {
            check(
                "the formula editor's script runs",
                script == "function/function/true",
                &script,
            );
            let editor = editor.clone();
            ask(
                &target,
                "injected content in the formula editor can do nothing",
                &format!("{INJECTED} {LEAVE_THE_PAGE}"),
                move |text| {
                    check(
                        "injected content in the formula editor can do nothing",
                        text == STOPPED,
                        &text,
                    );
                    the_formula_survives_its_web_process(mtm, editor.clone());
                },
            );
        },
    );
}

/// The number of the web process behind `web`, private in WebKit like the way `js.rs` uses it.
fn web_process(web: &objc2_web_kit::WKWebView) -> i32 {
    // SAFETY: A WKWebView method without arguments that returns a pid_t, which `js.rs` relies on as well.
    unsafe { objc2::msg_send![web, _webProcessIdentifier] }
}

/// The page that comes back after the web process ended starts empty, and the next input would replace the formula.
fn the_formula_survives_its_web_process(mtm: MainThreadMarker, editor: Option<Retained<EditorController>>) {
    let name = "the formula editor shows its formula again after its web process ended";
    let Some(editor) = editor else {
        check(name, false, "no editor to attach");
        the_same_attack_without_a_policy(mtm);
        return;
    };
    let field = mathfield::field(mtm);
    field.attach(&editor, "x^2");
    let ended = web_process(&field.web);
    js::end_web_process(&field.web);
    let watched = field.clone();
    let back = move || watched.loaded.get() && ![0, ended].contains(&web_process(&watched.web));
    once(0.0, "the formula editor's page to load again", back, move || {
        let web = field.web.clone();
        let (field, editor) = (field.clone(), editor.clone());
        ask(&web, name, "return document.getElementById('mf').value;", move |text| {
            check(name, text == "x^2", &text);
            field.detach(&editor);
            the_same_attack_without_a_policy(mtm);
        });
    });
}

/// The positive control of the attacks: in a page without the policy the fetch and the handler work, so where they do not,
/// the policy stopped them.
fn the_same_attack_without_a_policy(mtm: MainThreadMarker) {
    let name = "without the policy, injected content can fetch and run its handler";
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, 100.0));
    let web = webkit::new_view(mtm, frame, &webkit::configuration(mtm));
    // SAFETY: A constant page without a base address.
    let _ = unsafe { web.loadHTMLString_baseURL(&ns("<!doctype html><body></body>"), None) };
    let watched = web.clone();
    // SAFETY: Reading whether the view is loading asks for nothing.
    let loaded = move || !unsafe { watched.isLoading() };
    once(0.0, "the page without a policy to load", loaded, move || {
        // WebKit answers only while its view lives.
        let keep = web.clone();
        ask(&web, name, &format!("{INJECTED} return said;"), move |text| {
            let _ = &keep;
            check(
                name,
                text == "fetch allowed / image failed / inline handler ran / violated ",
                &text,
            );
            offline_rules_without_a_policy(mtm, false);
        });
    });
}

/// A page without a policy that asks a scheme of its own for a picture: only the offline rules stand in its way.
const ASKS_ITS_SCHEME: &str = "<!doctype html><body><img src=\"splat-probe:/bild.png\"></body>";

static SCHEME_ASKED: AtomicUsize = AtomicUsize::new(0);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDProbeScheme"]
    struct ProbeScheme;

    unsafe impl NSObjectProtocol for ProbeScheme {}

    unsafe impl WKURLSchemeHandler for ProbeScheme {
        #[unsafe(method(webView:startURLSchemeTask:))]
        fn start(&self, _web: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            SCHEME_ASKED.fetch_add(1, Ordering::SeqCst);
            // SAFETY: The task is running, and failing it is its last callback.
            unsafe { task.didFailWithError(&NSError::new(-1, &ns("splat-probe"))) };
        }

        #[unsafe(method(webView:stopURLSchemeTask:))]
        fn stop(&self, _web: &WKWebView, _task: &ProtocolObject<dyn WKURLSchemeTask>) {}
    }
);

/// The positive control first: without the rules the page's picture is asked for, so where it is not, the rules held.
fn offline_rules_without_a_policy(mtm: MainThreadMarker, rules: bool) {
    let config = webkit::configuration(mtm);
    // SAFETY: NSObject's initializer, called once on the newly allocated object.
    let scheme: Retained<ProbeScheme> = unsafe { msg_send![ProbeScheme::alloc(mtm), init] };
    // SAFETY: A scheme WebKit does not handle itself, set before the configuration is used.
    unsafe { config.setURLSchemeHandler_forURLScheme(Some(ProtocolObject::from_ref(&*scheme)), &ns("splat-probe")) };
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, 100.0));
    let web = webkit::new_view(mtm, frame, &config);
    let before = SCHEME_ASKED.load(Ordering::SeqCst);
    let page = web.clone();
    // SAFETY: A constant page without a base address.
    let load = move || drop(unsafe { page.loadHTMLString_baseURL(&ns(ASKS_ITS_SCHEME), None) });
    if rules {
        webkit::when_offline(&web, load);
    } else {
        load();
    }
    let watched = web.clone();
    // SAFETY: Reading whether the view is loading asks for nothing.
    let loaded = move || !unsafe { watched.isLoading() };
    once(0.5, "the page that asks its own scheme to load", loaded, move || {
        let _ = (&web, &scheme);
        let asked = SCHEME_ASKED.load(Ordering::SeqCst) - before;
        if rules {
            let name = "the offline rules keep a page without a policy from loading anything";
            check(name, asked == 0, &format!("{asked} requests"));
            formula_text(mtm);
        } else {
            let name = "without the offline rules, a page without a policy loads what it names";
            check(name, asked == 1, &format!("{asked} requests"));
            offline_rules_without_a_policy(mtm, true);
        }
    });
}

fn formula_text(mtm: MainThreadMarker) {
    let name = "a formula keeps text that joins its letters in one piece";
    ask(&js::renderer(mtm).web(), name, FORMULA_TEXT, move |text| {
        check(name, text == "true / true / true / 0", &text);
        formula_packages(mtm);
    });
}

fn formula_packages(mtm: MainThreadMarker) {
    let name = "a formula finds its packages and fonts in the app and stays one picture";
    ask(&js::renderer(mtm).web(), name, FORMULA_PACKAGES, move |text| {
        check(name, text == "false /  / true / true", &text);
        formulas_stand_alone(mtm);
    });
}

/// What one formula does to MathJax's labels and definitions stays out of the next, and a document's definitions reach its
/// other formulas in whatever order they render.
const FORMULAS_ALONE: &str = "const draw = (tex, macros = '') => renderMath(tex, true, '#000000', macros); \
    await draw('a\\\\label{same}'); \
    const again = await draw('a\\\\label{same}'); \
    const alpha = (await draw('\\\\alpha')).w; \
    await draw('\\\\renewcommand{\\\\alpha}{xxxxxxxx}\\\\alpha'); \
    const after = (await draw('\\\\alpha')).w; \
    const own = (await draw('\\\\Rr', '\\\\newcommand{\\\\Rr}{xxxxxxxx}')).w; \
    const eight = (await draw('xxxxxxxx')).w; \
    const unknown = (await draw('\\\\dv{f}{x}')).w; \
    const known = (await draw('\\\\require{physics}\\\\dv{f}{x}')).w; \
    const unknownAgain = (await draw('\\\\dv{f}{x}')).w; \
    const fromDocument = (await draw('\\\\dv{f}{x}', '\\\\require{physics}')).w; \
    const notFromIt = (await draw('\\\\dv{f}{x}')).w; \
    const brokenMacros = await draw('\\\\alpha', '\\\\require{nosuchpackage}'); \
    return [again.error === '', alpha === after, own === eight, known < unknown, unknownAgain === unknown, \
        fromDocument === known, notFromIt === unknown, brokenMacros.w === alpha].join(' / ');";

/// A tag asks for the whole width of a container this page does not have.
const TAGGED_FORMULA: &str = "const draw = (tex) => renderMath(tex, true, '#000000', ''); \
    const plain = await draw('E = mc^2'); \
    const tagged = await draw('E = mc^2 \\\\tag{1}'); \
    const opens = tagged.svg.match(/<svg[^>]*>/)[0]; \
    return [tagged.w > plain.w, tagged.w < plain.w + 30, !opens.includes('100%')].join(' / ');";

const MATH_FIELDS: &str = "return Object.keys(await renderMath('a', false, '#000000', '')).sort().join();";
const DIAGRAM_FIELDS: &str =
    "return Object.keys(await renderDiagram('graphviz', 'digraph { a }', false)).sort().join();";

/// What the app reads of the page's answers is what the page answers.
fn answers_of_the_page(mtm: MainThreadMarker) {
    let name = "the page answers a formula with the fields the app reads";
    ask(&js::renderer(mtm).web(), name, MATH_FIELDS, move |text| {
        check(name, text == js::MathAnswer::FIELDS.join(","), &text);
        let name = "the page answers a diagram with the fields the app reads";
        ask(&js::renderer(mtm).web(), name, DIAGRAM_FIELDS, move |text| {
            check(name, text == js::PlacedPicture::FIELDS.join(","), &text);
            formula_styles(mtm);
        });
    });
}

fn formulas_stand_alone(mtm: MainThreadMarker) {
    let name = "a formula depends on nothing but itself and the definitions of its document";
    ask(&js::renderer(mtm).web(), name, FORMULAS_ALONE, move |text| {
        check(name, text == ["true"; 8].join(" / "), &text);
        let name = "a formula with a tag is as wide as it needs to be";
        ask(&js::renderer(mtm).web(), name, TAGGED_FORMULA, move |text| {
            check(name, text == "true / true / true", &text);
            answers_of_the_page(mtm);
        });
    });
}

fn formula_styles(mtm: MainThreadMarker) {
    let name = "a table draws its lines in the color of the text and an error draws no black block";
    ask(&js::renderer(mtm).web(), name, FORMULA_STYLES, move |text| {
        let shares: Vec<_> = text.split("@@@").map(drawn_and_dark).collect();
        let lines = |share: (f64, f64)| (0.01..0.6).contains(&share.0);
        let right = matches!(
            shares[..],
            [Some(dark), Some(light), Some(error)]
                if lines(dark) && dark.1 > 0.5 && lines(light) && light.1 < 0.5 && error.0 > 0.5 && error.1 < 0.05
        );
        check(name, right, &format!("{shares:?}"));
        // A render already under way would run out of time before this one.
        once(
            0.0,
            "the renderer to finish what the documents asked for",
            move || js::renderer(mtm).idle(),
            move || the_defaults_of_a_graph_find_its_brace(mtm),
        );
    });
}

const GRAPH_BRACES: &str = "const opens = ['// {\\ndigraph { a }', '/* { */ digraph { a }', '# {\\ndigraph { a }', \
    'digraph \"x{\\\\\"{\" { a }', 'digraph { a }']; \
    const closed = ['digraph', '/* {', '// {', '\"{']; \
    const wrong = [...opens.filter((c) => graphBrace(c) !== c.lastIndexOf('{')), ...closed.filter((c) => graphBrace(c) !== -1)]; \
    return 'wrong: ' + wrong.join(' | ');";

/// The colors of the page go into the graph as defaults, so a graph without them shows black on a dark window.
const GRAPH_COLORS: &str = "await renderGraphviz('// {\\ndigraph { a }', true); \
    return document.getElementById('out').innerHTML.includes('#e8e8ed') ? 'colored' : 'not colored';";

fn the_defaults_of_a_graph_find_its_brace(mtm: MainThreadMarker) {
    let name = "the defaults of a graph go behind its brace, not into a comment or name";
    ask(&js::renderer(mtm).web(), name, GRAPH_BRACES, move |text| {
        check(name, text == "wrong: ", &text);
        let name = "a graph behind a comment with a brace takes the colors of the page";
        ask(&js::renderer(mtm).web(), name, GRAPH_COLORS, move |text| {
            check(name, text == "colored", &text);
            a_page_that_stops_answering(mtm);
        });
    });
}

/// Whatever a document's formula or diagram sets off in the page, the renderer gives up on it at its time limit and goes on
/// in a new page. The formula is one no other check asks for, so its answer can only come from there.
fn a_page_that_stops_answering(mtm: MainThreadMarker) {
    let renderer = js::renderer(mtm);
    webkit::run_script(&renderer.web(), "while (true) {}", &[]);
    let formula = || Key::math("a^2 + b^2 = c^2", false, false, &"".into());
    renderer.request(super::super::render_queue::Owner::unique(), vec![formula()]);
    let limit = js::RENDER_TIMEOUT.as_secs_f64();
    let started = std::time::Instant::now();
    let answered = move || js::outcome(&formula()).is_some();
    let then = move || {
        let outcome = js::outcome(&formula());
        let waited = started.elapsed().as_secs_f64();
        check(
            "a formula renders in a new page after the old one stopped answering",
            outcome == Some(Ok(())) && waited >= limit,
            &format!("{outcome:?} after {waited:.1} s, the limit is {limit} s"),
        );
        attack_the_renderer(mtm);
    };
    super::wait(0.0, limit + super::UNTIL_TIMEOUT, 1, answered, move |held| {
        if !held {
            check(
                "the renderer to give up on a page that stopped answering",
                false,
                "waited for it",
            );
        }
        then();
    });
}

/// The renderer's page gets formulas and diagrams from the document, so whatever slips through its libraries runs here.
fn attack_the_renderer(mtm: MainThreadMarker) {
    let name = "injected content in the renderer can do nothing";
    ask(
        &js::renderer(mtm).web(),
        name,
        &format!("{INJECTED} {LEAVE_THE_PAGE}"),
        move |text| {
            check(name, text == STOPPED, &text);
            finish();
        },
    );
}
