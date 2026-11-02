//! What needs the app but no visible window: rendering, remote images, exports, the sharing menu and what injected
//! content can do in the formula editor. Every result is a check, so a broken one fails the run.

use std::path::{Path, PathBuf};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSAttachmentAttributeName, NSBitmapImageFileType, NSBitmapImageRep, NSDocument,
    NSDocumentChangeType, NSDocumentController, NSPasteboard, NSPasteboardTypePNG, NSTextAttachment,
};
use objc2_foundation::{NSData, NSDictionary, NSError, NSRange, NSString, NSURL};
use objc2_web_kit::WKContentWorld;

use super::super::EditorController;
use super::super::js::{self, Key};
use super::super::mathfield::{self, MathField};
use super::mock_web::{HOST, requests_of};
use super::{after, check, finish};
use crate::document::MdDocument;
use crate::markdown::doc::DiagramKind;
use crate::util::{constant, ns};

/// How long rendering, the reload and the downloads may take together.
const PATIENCE: f64 = 45.0;

const REMOTE_PICTURES: [&str; 7] = [
    "/badge.svg",
    "/fehlt.png",
    "/kein-bild.png",
    "/zu-gross.png",
    "/umleitung.png",
    "/wackelig.svg",
    "/ohne-netz.svg",
];

fn url_in_probe(path: &str) -> String {
    format!("{HOST}{path}")
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

pub(super) fn run(mtm: MainThreadMarker) {
    let keys = render_keys();
    js::renderer(mtm).request(keys.iter().map(|(_, k)| k.clone()).collect());
    let field = mathfield::field(mtm);
    let local = std::env::temp_dir().join(format!("splat-probe-{}.bmp", std::process::id()));
    let _ = std::fs::write(&local, two_pixel_bmp());
    // SAFETY: NSDocument's initializer, called once on the newly allocated object.
    let document: Retained<MdDocument> = unsafe { objc2::msg_send![MdDocument::alloc(mtm), init] };
    document.set_initial_text(&format!(
        "# Probe\n\n## Teil\n\n![a](https://192.168.0.1/x.png) ![b](https://localhost/y.png)\n\n![c](https://intranet/z.png)\n\n![lokal](<{}>)\n\nFormel $E = mc^2$\n\n| a | b |\n|---|---|\n| 1 | 2 |",
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
    let done = RcBlock::new(|_: *mut NSDocument, _: Bool, _: *mut NSError| {});
    let url = NSURL::fileURLWithPath(&ns(changed.to_str().unwrap_or_default()));
    NSDocumentController::sharedDocumentController(mtm)
        .openDocumentWithContentsOfURL_display_completionHandler(&url, false, &done);
    let (opened, rewrite) = (url.clone(), changed.clone());
    after(1.0, move || {
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
    });
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
    fn reloaded_text(&self) -> Option<String> {
        NSDocumentController::sharedDocumentController(self.mtm)
            .documentForURL(&self.changed_url)
            .and_then(|d| d.downcast::<MdDocument>().ok())
            .map(|d| d.text())
    }

    fn local_image_shown(&self) -> Option<bool> {
        self.editor
            .as_ref()
            .and_then(|e| e.ivars().images.borrow().shown(&self.local))
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
    }
}

/// A failure that passes is tried again, by the timer that also watches files.
fn answered(path: &str) -> bool {
    super::super::remote::try_failed_again();
    matches!(super::super::remote::get(&url_in_probe(path), None), Some(Some(_)))
}

fn wait_until_done(probe: std::rc::Rc<Probe>, started: std::time::Instant) {
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
    remote_answers();
    check(
        "a local image is decoded in a helper",
        probe.local_image_shown() == Some(true),
        "",
    );
    if let Some(e) = &probe.editor {
        exports(e);
        sharing(e, probe.mtm);
    }
    let _ = std::fs::remove_file(&probe.local);
    if let Some(e) = &probe.editor {
        outline_and_web_permission(e);
        pasting_a_picture(e, &probe.document);
    }
    super::controller::checks(probe.mtm);
    formula_editor(probe);
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
        "a server error passes: the picture loads on the next try",
        shown("/wackelig.svg"),
        "",
    );
    check(
        "a lost connection passes: the picture loads on the next try",
        shown("/ohne-netz.svg"),
        "",
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
        .html("Probe", &std::env::temp_dir().join("probe.html"));
    check(
        "the HTML export embeds the local image and the formula",
        page.matches("src=\"data:image/png;base64,").count() >= 2 && page.contains("class=\"math\""),
        "",
    );
    check("the HTML export holds no script", !page.contains("<script"), "");
    let word = e.export_source().word();
    let listing = String::from_utf8_lossy(&word);
    check(
        "the Word export holds its document and pictures",
        listing.contains("word/document.xml") && listing.contains("word/media/"),
        &format!("{} bytes", word.len()),
    );
    // The 2 × 1 px image shrinks with the text to 0.75 pt per pixel, on paper and in Word.
    let printed = e.printable(400.0);
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
    check(
        "an image is scaled like the text on paper",
        sizes
            .iter()
            .any(|s| (s.width - 1.5).abs() < 0.01 && (s.height - 0.75).abs() < 0.01),
        &format!("{sizes:?}"),
    );
    check(
        "an image is scaled like the text in Word",
        listing.contains("<wp:extent cx=\"19050\" cy=\"9525\"/>"),
        "",
    );
}

fn sharing(e: &EditorController, mtm: MainThreadMarker) {
    let titles: Vec<String> = e
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
}

fn outline_and_web_permission(e: &EditorController) {
    e.toggle_outline();
    let rows = e.ivars().outline.get().map(|o| o.view.numberOfRows());
    check(
        "the outline lists two nested headings",
        rows == Some(2),
        &format!("{rows:?}"),
    );
    let banner = |e: &EditorController| e.ivars().remote_banner.get().map(|b| !b.isHidden());
    check(
        "web images wait for permission",
        banner(e) == Some(true),
        &format!("{:?}", banner(e)),
    );
    e.allow_remote_images();
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

fn string_of(result: *mut AnyObject) -> Option<String> {
    // SAFETY: WebKit passes nil or a valid object for the duration of the call.
    unsafe { result.as_ref() }.and_then(|r| r.downcast_ref::<NSString>().map(|s| s.to_string()))
}

/// What injected content would try: load from the network, run an inline handler, navigate away.
const ATTACK: &str = "let net; try { await fetch('https://example.com/'); net = 'fetch allowed'; } catch (e) { net = 'fetch blocked'; } \
    document.body.insertAdjacentHTML('beforeend', '<img src=x onerror=\"window.pwned = 1\">'); \
    await new Promise(r => setTimeout(r, 300)); \
    location.href = 'https://example.com/'; \
    await new Promise(r => setTimeout(r, 1500)); \
    return net + ' / inline handler ' + (window.pwned ? 'ran' : 'blocked') + ' / page ' + location.protocol;";

/// What MathJax makes of text outside its fonts: letters that join or run from right to left must stay one piece,
/// while text it has glyphs for stays in its own font.
const FORMULA_TEXT: &str = "const svg = async (tex) => (await renderMath(tex, false, '#000')).svg; \
    const pieces = (svg) => svg.split('<text').length - 1; \
    const hebrew = await svg('\\\\text{שלום}'); \
    const arabic = await svg('\\\\textbf{العربية}'); \
    const latin = await svg('\\\\text{abc}'); \
    return [hebrew.includes('>שלום</text>'), arabic.includes('>العربية</text>'), arabic.includes('bold'), pieces(latin)].join(' / ');";

fn formula_text(mtm: MainThreadMarker) {
    let report = RcBlock::new(move |result: *mut AnyObject, error: *mut NSError| {
        let text = string_of(result)
            .or_else(|| {
                // SAFETY: WebKit passes nil or a valid error for the duration of the call.
                unsafe { error.as_ref() }.map(|e| format!("error {}", e.localizedDescription()))
            })
            .unwrap_or_default();
        check(
            "a formula keeps text that joins its letters in one piece",
            text == "true / true / true / 0",
            &text,
        );
        attack_the_renderer(mtm);
    });
    // SAFETY: The script is a constant, and the block outlives the call as WebKit retains it.
    unsafe {
        js::renderer(mtm)
            .web()
            .callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                &ns(FORMULA_TEXT),
                None,
                None,
                &WKContentWorld::pageWorld(mtm),
                Some(&report),
            )
    };
}

/// The renderer's page gets formulas and diagrams from the document, so whatever slips through its libraries runs here.
fn attack_the_renderer(mtm: MainThreadMarker) {
    let report = RcBlock::new(|result: *mut AnyObject, error: *mut NSError| {
        let text = string_of(result)
            .or_else(|| {
                // SAFETY: WebKit passes nil or a valid error for the duration of the call.
                unsafe { error.as_ref() }.map(|e| format!("error {}", e.localizedDescription()))
            })
            .unwrap_or_default();
        check(
            "injected content in the renderer can do nothing",
            text == "fetch blocked / inline handler blocked / page file:",
            &text,
        );
        finish();
    });
    // SAFETY: The script is a constant, and the block outlives the call as WebKit retains it.
    unsafe {
        js::renderer(mtm)
            .web()
            .callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                &ns(ATTACK),
                None,
                None,
                &WKContentWorld::pageWorld(mtm),
                Some(&report),
            )
    };
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
    let done = RcBlock::new(move |result: *mut AnyObject, _: *mut NSError| {
        let script = string_of(result).unwrap_or_default();
        check(
            "the formula editor's script runs",
            script == "function/function/true",
            &script,
        );
        let report = RcBlock::new(move |result: *mut AnyObject, error: *mut NSError| {
            let text = string_of(result)
                .or_else(|| {
                    // SAFETY: WebKit passes nil or a valid error for the duration of the call.
                    unsafe { error.as_ref() }.map(|e| format!("error {}", e.localizedDescription()))
                })
                .unwrap_or_default();
            check(
                "injected content in the formula editor can do nothing",
                text == "fetch blocked / inline handler blocked / page file:",
                &text,
            );
            formula_text(mtm);
        });
        // SAFETY: The script is a constant, and the block outlives the call as WebKit retains it.
        unsafe {
            web.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                &ns(ATTACK),
                None,
                None,
                &WKContentWorld::pageWorld(mtm),
                Some(&report),
            )
        };
    });
    // SAFETY: The script is a constant, and the block outlives the call as WebKit retains it.
    unsafe {
        probe.field.web.evaluateJavaScript_completionHandler(
            &ns("typeof MathfieldElement + '/' + typeof setLatex + '/' + (document.getElementById('mf') instanceof MathfieldElement)"),
            Some(&done),
        )
    };
    after(8.0, || {
        check("the formula editor answers", false, "no answer within 8 s");
        finish();
    });
}
