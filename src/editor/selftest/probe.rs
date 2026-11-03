//! What needs the app but no visible window: rendering, remote images, exports, the sharing menu and what injected
//! content can do in the formula editor. Every result is a check, so a broken one fails the run.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSAttachmentAttributeName, NSBitmapImageFileType, NSBitmapImageRep, NSDocument,
    NSDocumentChangeType, NSDocumentController, NSPasteboard, NSPasteboardTypePNG, NSTextAttachment,
};
use objc2_foundation::{NSData, NSDictionary, NSError, NSPoint, NSRange, NSRect, NSSize, NSString, NSURL};
use objc2_web_kit::WKWebView;

use super::super::EditorController;
use super::super::js::{self, Key};
use super::super::mathfield::{self, MathField};
use super::super::webkit;
use super::mock_web::{HOST, requests_of};
use super::{after, check, finish, guarded, once};
use crate::document::MdDocument;
use crate::markdown::doc::DiagramKind;
use crate::util::{constant, ns};

/// How long rendering, the reload and the downloads may take together.
const PATIENCE: f64 = 45.0;

const REMOTE_PICTURES: [&str; 8] = [
    "/badge.svg",
    "/fehlt.png",
    "/kein-bild.png",
    "/zu-gross.png",
    "/umleitung.png",
    "/wackelig.svg",
    "/ohne-netz.svg",
    "/nicht-gezeigt.svg",
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
        "# Probe\n\n## Teil\n\n![a](https://192.168.0.1/x.png) ![b](https://localhost/y.png)\n\n![c](https://intranet/z.png)\n\n![w]({0}) ![n]({1})\n\n![lokal](<{2}>)\n\nFormel $E = mc^2$\n\n| a | b |\n|---|---|\n| 1 | 2 |",
        url_in_probe("/wackelig.svg"),
        url_in_probe("/ohne-netz.svg"),
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
        .sharing()
        .export_source()
        .html("Probe", &std::env::temp_dir().join("probe.html"));
    check(
        "the HTML export embeds the local image and the formula",
        page.matches("src=\"data:image/png;base64,").count() >= 2 && page.contains("class=\"math\""),
        "",
    );
    check("the HTML export holds no script", !page.contains("<script"), "");
    let word = e.sharing().export_source().word().expect("a small document");
    let listing = String::from_utf8_lossy(&word);
    check(
        "the Word export holds its document and pictures",
        listing.contains("word/document.xml") && listing.contains("word/media/"),
        &format!("{} bytes", word.len()),
    );
    // The 2 × 1 px image shrinks with the text to 0.75 pt per pixel, on paper and in Word.
    let printed = e.sharing().printable(400.0);
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
    let handled = e.editing().paste_image(&pasteboard);
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
    crate::util::end_process(ended);
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
            formula_text(mtm);
        });
    });
}

fn formula_text(mtm: MainThreadMarker) {
    let name = "a formula keeps text that joins its letters in one piece";
    ask(&js::renderer(mtm).web(), name, FORMULA_TEXT, move |text| {
        check(name, text == "true / true / true / 0", &text);
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
    let formula = || Key::math("a^2 + b^2 = c^2", false, false);
    renderer.request(vec![formula()]);
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
    super::poll_once(
        "the renderer to give up on a page that stopped answering",
        limit + super::UNTIL_TIMEOUT,
        std::rc::Rc::new(answered),
        std::rc::Rc::new(then),
        started,
    );
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
