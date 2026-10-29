//! Scripted end-to-end check of the running app, compiled only with `--features selftest`.

#![allow(clippy::undocumented_unsafe_blocks)]

mod scenarios;

use std::cell::Cell;
use std::path::{Path, PathBuf};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSApplication, NSDocumentController, NSPrintInfo, NSPrintJobDisposition, NSPrintJobSavingURL, NSPrintSaveJob,
    NSTextInputClient, NSWindow,
};
use objc2_foundation::{NSCopying, NSNotFound, NSRange, NSString, NSTimer, NSURL};

use super::EditorController;
use crate::document::MdDocument;
use crate::markdown::text::Utf16Map;
use crate::util::{ns, obj, storage};

thread_local! {
    static FAILURES: Cell<usize> = const { Cell::new(0) };
}

fn check(name: &str, ok: bool, detail: &str) {
    if ok {
        eprintln!("SELFTEST PASS {name}");
    } else {
        FAILURES.with(|f| f.set(f.get() + 1));
        eprintln!("SELFTEST FAIL {name}: {detail}");
    }
    // A shorter run for one area: SPLAT_SELFTEST_STOP_AFTER names the last check.
    if std::env::var("SPLAT_SELFTEST_STOP_AFTER").is_ok_and(|last| last == name) {
        finish();
    }
}

fn after(seconds: f64, f: impl Fn() + 'static) {
    let block = RcBlock::new(move |_timer: std::ptr::NonNull<NSTimer>| f());
    unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(seconds, false, &block) };
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
    let x = (point.x * scale) as isize;
    let y = ((window.frame().size.height - point.y) * scale) as isize;
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
    check(
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

/// Renders a formula and a diagram, loads the visual formula editor and a remote image, all without a visible
/// window, and reports whether each worked.
fn render_probe(mtm: MainThreadMarker) {
    use super::js::{self, Key};
    let math = Key::Math {
        tex: "E = mc^2".into(),
        display: false,
        dark: false,
    };
    let diagram = Key::Diagram {
        source: "flowchart LR\n    A[Markdown] --> B[PDF]".into(),
        kind: crate::markdown::doc::DiagramKind::Mermaid,
        dark: false,
    };
    let graphviz = Key::Diagram {
        source: "digraph { Markdown -> PDF; Markdown -> HTML }".into(),
        kind: crate::markdown::doc::DiagramKind::Graphviz,
        dark: true,
    };
    let plantuml = Key::Diagram {
        source: "Alice -> Bob : Hallo\nBob --> Alice : Antwort\n".into(),
        kind: crate::markdown::doc::DiagramKind::PlantUml,
        dark: false,
    };
    js::renderer(mtm).request(vec![math.clone(), diagram.clone(), graphviz.clone(), plantuml.clone()]);
    let field = super::mathfield::field(mtm);
    let badge = "https://img.shields.io/badge/splat-ok-blue.svg";
    super::remote::load(badge);
    // Images on blocked hosts must neither be requested nor end the app while the preview renders them.
    let blocked: Retained<crate::document::MdDocument> =
        unsafe { objc2::msg_send![crate::document::MdDocument::alloc(mtm), init] };
    // A local image, decoded in a helper: a 2 × 1 BMP, a format the system decoder would accept as well.
    let local = std::env::temp_dir().join(format!("splat-probe-{}.bmp", std::process::id()));
    let mut bmp = b"BM".to_vec();
    for v in [62u32, 0, 54, 40, 2, 1] {
        bmp.extend_from_slice(&v.to_le_bytes());
    }
    bmp.extend_from_slice(&[1, 0, 24, 0]);
    bmp.extend_from_slice(&[0; 24]);
    bmp.extend_from_slice(&[0, 0, 255, 255, 0, 0, 0, 0]);
    let _ = std::fs::write(&local, &bmp);
    blocked.set_initial_text(&format!(
        "# Probe\n\n## Teil\n\n![a](https://192.168.0.1/x.png) ![b](https://localhost/y.png)\n\n![c](https://intranet/z.png)\n\n![lokal](<{}>)\n\nFormel $E = mc^2$\n\n| a | b |\n|---|---|\n| 1 | 2 |",
        local.display()
    ));
    blocked.makeWindowControllers();
    let secret = std::env::temp_dir().join(format!("splat-probe-secret-{}", std::process::id()));
    let _ = std::fs::write(&secret, "secret");
    eprintln!(
        "PROBE decoder helper reads files or reaches the network: {:?}",
        super::decoder::self_check(secret.to_str().unwrap_or_default())
    );
    let _ = std::fs::remove_file(&secret);
    let editor_of_blocked = blocked.editor();
    let blocked_doc = blocked.clone();
    eprintln!(
        "PROBE blocked remote images: {} of 3 shown as failed",
        [
            "https://192.168.0.1/x.png",
            "https://localhost/y.png",
            "https://intranet/z.png"
        ]
        .iter()
        .filter(|u| matches!(super::remote::get(u, None), Some(None)))
        .count()
    );
    // macOS keeps recent documents per signature; this one must still be listed after the next rebuild.
    let recent = std::env::temp_dir().join("splat-probe-recent.md");
    let _ = std::fs::write(&recent, "# Recent\n");
    let documents = NSDocumentController::sharedDocumentController(mtm);
    eprintln!(
        "PROBE recent documents kept from the last run: {}",
        documents.recentDocumentURLs().count()
    );
    documents.noteNewRecentDocumentURL(&NSURL::fileURLWithPath(&ns(recent.to_str().unwrap_or_default())));
    // Another program changes an open file right after Splat saved it, without file coordination, as Git or a shell
    // would: the hardest case, since macOS reports no change of its own then.
    let changed = std::env::temp_dir().join(format!("splat-probe-reload-{}.md", std::process::id()));
    let _ = std::fs::write(&changed, "# Vorher\n");
    let done = RcBlock::new(
        |_: *mut objc2_app_kit::NSDocument, _: objc2::runtime::Bool, _: *mut objc2_foundation::NSError| {},
    );
    let changed_url = NSURL::fileURLWithPath(&ns(changed.to_str().unwrap_or_default()));
    documents.openDocumentWithContentsOfURL_display_completionHandler(&changed_url, false, &done);
    let (opened, rewrite) = (changed_url.clone(), changed.clone());
    after(1.0, move || {
        let document = NSDocumentController::sharedDocumentController(mtm)
            .documentForURL(&opened)
            .and_then(|d| d.downcast::<MdDocument>().ok());
        if let Some(document) = document {
            document.makeWindowControllers();
            document.updateChangeCount(objc2_app_kit::NSDocumentChangeType::ChangeDone);
            unsafe { document.saveDocument(None) };
            write_once_saved(mtm, opened.clone(), rewrite.clone());
        }
    });
    after(12.0, move || {
        let reloaded = NSDocumentController::sharedDocumentController(mtm)
            .documentForURL(&changed_url)
            .and_then(|d| d.downcast::<MdDocument>().ok())
            .map(|d| d.text());
        eprintln!(
            "PROBE a file changed right after Splat saved it reloads: {}",
            reloaded.as_deref() == Some("# Nachher\n")
        );
        let _ = std::fs::remove_file(&changed);
        eprintln!(
            "PROBE recent documents listed: {}",
            NSDocumentController::sharedDocumentController(mtm)
                .recentDocumentURLs()
                .count()
        );
        eprintln!("PROBE formula: {:?}", js::outcome(&math));
        eprintln!("PROBE diagram: {:?}", js::outcome(&diagram));
        eprintln!("PROBE graphviz: {:?}", js::outcome(&graphviz));
        eprintln!("PROBE plantuml: {:?}", js::outcome(&plantuml));
        for (name, key) in [("graphviz", &graphviz), ("plantuml", &plantuml)] {
            let png = js::cached(key)
                .and_then(|r| r.image.TIFFRepresentation())
                .and_then(|tiff| objc2_app_kit::NSBitmapImageRep::imageRepWithData(&tiff))
                .and_then(|rep| unsafe {
                    rep.representationUsingType_properties(
                        objc2_app_kit::NSBitmapImageFileType::PNG,
                        &objc2_foundation::NSDictionary::new(),
                    )
                });
            let path = std::env::temp_dir().join(format!("splat-probe-{name}.png"));
            if let Some(png) = png {
                png.writeToFile_atomically(&ns(path.to_str().unwrap_or_default()), true);
                eprintln!("PROBE {name} picture: {}", path.display());
            }
        }
        eprintln!(
            "PROBE remote badge: {}",
            super::remote::get(badge, None).is_some_and(|r| r.is_some())
        );
        eprintln!(
            "PROBE local image decoded in a helper: {:?}",
            editor_of_blocked
                .as_ref()
                .and_then(|e| e.ivars().images.borrow().shown(&local))
        );
        if let Some(e) = &editor_of_blocked {
            let page = e.html("Probe", &std::env::temp_dir().join("probe.html"));
            eprintln!(
                "PROBE html export: local image embedded {}, formula embedded {}, no script {}",
                page.matches("src=\"data:image/png;base64,").count() >= 2,
                page.contains("class=\"math\""),
                !page.contains("<script")
            );
            let word = e.word();
            if let Ok(w) = &word {
                let _ = std::fs::write(std::env::temp_dir().join("splat-probe.docx"), w);
            }
            let listing = word
                .as_ref()
                .map(|w| String::from_utf8_lossy(w).into_owned())
                .unwrap_or_default();
            eprintln!(
                "PROBE word export: {} bytes, document {}, images {}",
                word.as_ref().map_or(0, Vec::len),
                listing.contains("word/document.xml"),
                listing.contains("word/media/")
            );
            let titles: Vec<String> = e
                .export_sharing_services()
                .iter()
                .map(|s| s.title().to_string())
                .collect();
            eprintln!("PROBE share menu exports: {titles:?}");
            let file_menu: Vec<String> = NSApplication::sharedApplication(mtm)
                .mainMenu()
                .and_then(|m| m.itemAtIndex(1))
                .and_then(|i| i.submenu())
                .map(|m| m.itemArray().iter().map(|i| i.title().to_string()).collect())
                .unwrap_or_default();
            eprintln!(
                "PROBE share item in the file menu once, right before the exports: {}",
                file_menu.iter().filter(|t| *t == "Teilen").count() == 1
                    && file_menu
                        .windows(2)
                        .any(|w| w[0] == "Teilen" && w[1] == "Als PDF exportieren …")
            );
            // The 2 × 1 px image shrinks with the text to 0.75 pt per pixel, on paper and in Word.
            let printed = e.printable(400.0);
            let mut sizes = Vec::new();
            let mut at = 0;
            while at < printed.length() {
                let mut range = NSRange::new(0, 0);
                let value = unsafe {
                    printed.attribute_atIndex_effectiveRange(objc2_app_kit::NSAttachmentAttributeName, at, &mut range)
                };
                if let Some(att) = value.and_then(|v| v.downcast::<objc2_app_kit::NSTextAttachment>().ok()) {
                    sizes.push(att.bounds().size);
                }
                at = (range.location + range.length).max(at + 1);
            }
            eprintln!(
                "PROBE image scaled like the text: printed {}, in Word {}",
                sizes
                    .iter()
                    .any(|s| (s.width - 1.5).abs() < 0.01 && (s.height - 0.75).abs() < 0.01),
                listing.contains("<wp:extent cx=\"19050\" cy=\"9525\"/>")
            );
        }
        let _ = std::fs::remove_file(&local);
        if let Some(e) = &editor_of_blocked {
            e.toggle_outline();
            let rows = e.ivars().outline.get().map(|o| o.view.numberOfRows());
            eprintln!("PROBE outline rows for two nested headings: {rows:?}");
            let banner = |e: &EditorController| e.ivars().remote_banner.get().map(|b| !b.isHidden());
            eprintln!("PROBE web images wait for permission, bar shown: {:?}", banner(e));
            e.allow_remote_images();
            eprintln!("PROBE after allowing, bar shown: {:?}", banner(e));
            // Pasting a picture writes it next to the document and links it.
            let dir = std::env::temp_dir().join(format!("splat-probe-paste-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            blocked_doc.setFileURL(Some(&objc2_foundation::NSURL::fileURLWithPath(&ns(dir
                .join("Probe.md")
                .to_str()
                .unwrap_or_default()))));
            let mut png = Vec::new();
            let _ = image::RgbaImage::new(2, 2).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
            let pasteboard = objc2_app_kit::NSPasteboard::pasteboardWithUniqueName();
            pasteboard.clearContents();
            pasteboard.setData_forType(
                Some(&objc2_foundation::NSData::with_bytes(&png)),
                crate::util::constant!(objc2_app_kit::NSPasteboardTypePNG),
            );
            let handled = e.paste_image(&pasteboard);
            let written = std::fs::read_dir(&dir).map_or(0, |d| {
                d.flatten()
                    .filter(|f| f.path().extension().is_some_and(|x| x == "png"))
                    .count()
            });
            eprintln!(
                "PROBE paste a picture: handled {handled}, files {written}, link {}",
                e.source_text().contains("![Bild ")
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
        eprintln!("PROBE formula editor page loaded: {}", field.loaded.get());
        let web = field.web.clone();
        let done = block2::RcBlock::new(
            move |result: *mut objc2::runtime::AnyObject, _: *mut objc2_foundation::NSError| {
                let text = unsafe { result.as_ref() }
                    .and_then(|r| r.downcast_ref::<NSString>().map(|s| s.to_string()))
                    .unwrap_or_default();
                eprintln!("PROBE formula editor script: {text}");
                // What injected content would try: load from the network, run an inline handler, navigate away.
                let attack = "let net; try { await fetch('https://example.com/'); net = 'fetch allowed'; } catch (e) { net = 'fetch blocked'; } \
                document.body.insertAdjacentHTML('beforeend', '<img src=x onerror=\"window.pwned = 1\">'); \
                await new Promise(r => setTimeout(r, 300)); \
                location.href = 'https://example.com/'; \
                await new Promise(r => setTimeout(r, 1500)); \
                return net + ' / inline handler ' + (window.pwned ? 'ran' : 'blocked') + ' / page ' + location.protocol;";
                let report = block2::RcBlock::new(
                    |result: *mut objc2::runtime::AnyObject, error: *mut objc2_foundation::NSError| {
                        let text = unsafe { result.as_ref() }
                            .and_then(|r| r.downcast_ref::<NSString>().map(|s| s.to_string()))
                            .or_else(|| {
                                unsafe { error.as_ref() }.map(|e| format!("error {}", e.localizedDescription()))
                            })
                            .unwrap_or_default();
                        eprintln!("PROBE attack in the formula editor: {text}");
                        std::process::exit(0);
                    },
                );
                unsafe {
                    web.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                        &ns(attack),
                        None,
                        None,
                        &objc2_web_kit::WKContentWorld::pageWorld(MainThreadMarker::new().unwrap()),
                        Some(&report),
                    )
                };
            },
        );
        unsafe {
            field.web.evaluateJavaScript_completionHandler(
                &ns("typeof MathfieldElement + '/' + typeof setLatex + '/' + (document.getElementById('mf') instanceof MathfieldElement)"),
                Some(&done),
            )
        };
        after(8.0, || std::process::exit(1));
    });
}

/// Lays the link dialog out without showing it, checks that its fields are full width and writes it as a PNG.
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

fn head(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub fn start(mtm: MainThreadMarker) {
    if std::env::var_os("SPLAT_RENDER_PROBE").is_some() {
        render_probe(mtm);
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
        let controller = NSDocumentController::sharedDocumentController(mtm);
        let url = NSURL::fileURLWithPath(&ns(&file));
        let done = RcBlock::new(
            |_: *mut objc2_app_kit::NSDocument, _: objc2::runtime::Bool, _: *mut objc2_foundation::NSError| {},
        );
        controller.openDocumentWithContentsOfURL_display_completionHandler(&url, true, &done);
        after(2.5, move || resize_probe(mtm));
        return;
    }
    if std::env::var("SPLAT_BENCH").is_ok() {
        let controller = NSDocumentController::sharedDocumentController(mtm);
        let url = NSURL::fileURLWithPath(&ns(&file));
        let done = RcBlock::new(
            |_: *mut objc2_app_kit::NSDocument, _: objc2::runtime::Bool, _: *mut objc2_foundation::NSError| {},
        );
        let opened = std::time::Instant::now();
        controller.openDocumentWithContentsOfURL_display_completionHandler(&url, true, &done);
        eprintln!(
            "BENCH open+first render: {:.0} ms",
            opened.elapsed().as_secs_f64() * 1000.0
        );
        if std::env::var_os("SPLAT_BENCH_IDLE").is_some() {
            after(12.0, || std::process::exit(0));
        } else {
            after(2.0, move || bench(mtm));
        }
        return;
    }
    if std::env::var("SPLAT_SELFTEST_EXPECT_OPEN").is_ok() {
        // Launched by LaunchServices with the file as an open-document event, like a Finder double-click.
        after(3.0, move || {
            let docs = NSDocumentController::sharedDocumentController(mtm).documents();
            let opened = docs.iter().any(|d| {
                d.fileURL()
                    .and_then(|u| u.path())
                    .is_some_and(|p| p.to_string() == file)
                    && d.downcast::<MdDocument>().is_ok()
            });
            let rendered = editor(mtm).is_some_and(|e| storage(e.preview()).length() > 100);
            check(
                "double-click opens file in cold-started app",
                opened && rendered,
                &format!("opened={opened} rendered={rendered}"),
            );
            finish();
        });
        return;
    }
    let out = PathBuf::from(std::env::var("SPLAT_SELFTEST_OUT").unwrap_or_else(|_| ".".into()));
    forget_web_image_permission(&file);
    after(1.0, move || {
        let (file, out) = (file.clone(), out.clone());
        dock_click_opens_a_document(mtm, move || {
            let controller = NSDocumentController::sharedDocumentController(mtm);
            let url = NSURL::fileURLWithPath(&ns(&file));
            let done = RcBlock::new(
                |_: *mut objc2_app_kit::NSDocument, _: objc2::runtime::Bool, _: *mut objc2_foundation::NSError| {},
            );
            controller.openDocumentWithContentsOfURL_display_completionHandler(&url, true, &done);
            let out = out.clone();
            after(1.5, move || run(mtm, &out));
        });
    });
}

/// User defaults keep the permission an earlier run gave its copy of the document.
fn forget_web_image_permission(file: &str) {
    let paths: Vec<_> = super::window::allowed_documents()
        .iter()
        .filter(|p| *p != file)
        .map(|p| ns(p))
        .collect();
    let list = objc2_foundation::NSArray::from_retained_slice(&paths);
    unsafe {
        objc2_foundation::NSUserDefaults::standardUserDefaults()
            .setObject_forKey(Some(&list), &ns(super::window::ALLOWED_KEY))
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
    after(0.5, move || {
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
        let started = std::time::Instant::now();
        let then = then.clone();
        wait_for_window(
            mtm,
            started,
            std::rc::Rc::new(move |shown| {
                check(
                    "a click on the Dock icon opens a document",
                    closed && sent.is_ok() && shown,
                    &format!("closed before={closed} sent={sent:?} window shown={shown}"),
                );
                for doc in NSDocumentController::sharedDocumentController(mtm).documents().to_vec() {
                    doc.close();
                }
                let then = then.clone();
                after(0.3, move || then());
            }),
        );
    });
}

fn wait_for_window(mtm: MainThreadMarker, started: std::time::Instant, done: std::rc::Rc<dyn Fn(bool)>) {
    let shown = document_window_shown(mtm);
    if shown || started.elapsed().as_secs_f64() > 5.0 {
        done(shown);
    } else {
        after(0.1, move || wait_for_window(mtm, started, done.clone()));
    }
}

/// Drags the window's lower right corner through AppKit's own resize tracking, as a user does, and checks that the
/// source text follows the width while the mouse is still down.
fn resize_probe(mtm: MainThreadMarker) {
    let Some((e, window)) = editor(mtm).and_then(|e| Some((e.clone(), e.window()?))) else {
        check("open document", false, "no editor window");
        finish();
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.activate();
    window.makeKeyAndOrderFront(None);
    // Halfway down a long text, where NSTextView kept the old width until the mouse was released.
    e.source()
        .scrollRangeToVisible(NSRange::new(e.source_text().len() / 2, 0));
    let (steps, lag) = (std::rc::Rc::new(Cell::new(0usize)), std::rc::Rc::new(Cell::new(0.0f64)));
    let observer = {
        let (e, window, steps, lag) = (e.clone(), window.clone(), steps.clone(), lag.clone());
        RcBlock::new(move |_: std::ptr::NonNull<objc2_foundation::NSNotification>| {
            if !window.inLiveResize() {
                return;
            }
            let source = e.source();
            let expected = source.frame().size.width - 2.0 * source.textContainerInset().width;
            let actual = unsafe { source.textContainer() }.map_or(0.0, |c| c.size().width);
            steps.set(steps.get() + 1);
            lag.set(lag.get().max((expected - actual).abs()));
        })
    };
    let token = unsafe {
        objc2_foundation::NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(objc2_app_kit::NSWindowDidResizeNotification),
            Some(&window),
            None,
            &observer,
        )
    };
    let (width, number) = (window.frame().size.width, window.windowNumber());
    // The divider between source and preview, which the user may have moved before.
    let divider = e
        .source()
        .enclosingScrollView()
        .map(|s| s.convertPoint_toView(objc2_foundation::NSPoint::new(s.frame().size.width + 0.5, 300.0), None))
        .unwrap_or_default();
    let event = move |kind: objc2_app_kit::NSEventType, at: objc2_foundation::NSPoint| {
        objc2_app_kit::NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind,
            at,
            objc2_app_kit::NSEventModifierFlags::empty(),
            objc2_foundation::NSProcessInfo::processInfo().systemUptime(),
            number,
            None,
            0,
            1,
            1.0,
        )
    };
    // The events come one by one from a timer that also runs while AppKit tracks the resize.
    let tick = std::rc::Rc::new(Cell::new(0u32));
    let timer_block = RcBlock::new(move |timer: std::ptr::NonNull<NSTimer>| {
        let t = tick.get() + 1;
        tick.set(t);
        use objc2_app_kit::NSEventType as Kind;
        let at = |x: f64, y: f64| objc2_foundation::NSPoint::new(x, y);
        let (kind, point) = match t {
            2 => (Kind::LeftMouseDown, divider),
            3..=5 => (
                Kind::LeftMouseDragged,
                at(divider.x - f64::from(t - 2) * 50.0, divider.y),
            ),
            6 => (Kind::LeftMouseUp, at(divider.x - 150.0, divider.y)),
            10 => (Kind::LeftMouseDown, at(width - 2.0, 2.0)),
            11..=15 => (Kind::LeftMouseDragged, at(width - 2.0 + f64::from(t - 10) * 40.0, 2.0)),
            16 => (Kind::LeftMouseUp, at(width + 198.0, 2.0)),
            24 => {
                let _ = &token;
                unsafe { timer.as_ref() }.invalidate();
                check(
                    "the source re-wraps while the window is resized",
                    steps.get() > 0 && lag.get() < 0.5,
                    &format!(
                        "{} live steps, text width behind by up to {:.0} pt",
                        steps.get(),
                        lag.get()
                    ),
                );
                finish();
                return;
            }
            _ => return,
        };
        let Some(event) = event(kind, point) else { return };
        if kind == Kind::LeftMouseDown {
            window.sendEvent(&event);
        } else {
            app.postEvent_atStart(&event, false);
        }
    });
    let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(0.12, true, &timer_block) };
    unsafe {
        objc2_foundation::NSRunLoop::currentRunLoop().addTimer_forMode(&timer, objc2_foundation::NSRunLoopCommonModes)
    };
}

fn source(e: &EditorController) -> String {
    e.source_text()
}

fn type_in_preview(e: &EditorController, text: &str) {
    let preview = e.preview();
    unsafe { preview.insertText_replacementRange(&ns(text), NSRange::new(NSNotFound as usize, 0)) };
}

struct Ctx {
    e: Retained<EditorController>,
    out: PathBuf,
}

/// How long the runner waits before a step: a fixed time, for animations and screen captures, or until a condition
/// holds, for work whose duration varies.
enum Wait {
    Fixed(f64),
    Until(Box<dyn Fn(&Ctx) -> bool>),
}

type Step = (Wait, Box<dyn Fn(&Ctx)>);

/// Longest wait for a condition; a step that times out runs anyway and its checks report what is missing.
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
        i as isize,
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
    let until = STEPS.with(|s| match s.borrow().get(i).map(|s| &s.0) {
        Some(Wait::Fixed(seconds)) => Err(*seconds),
        Some(Wait::Until(_)) => Ok(()),
        None => Err(0.0),
    });
    match until {
        Err(seconds) => after(seconds, move || post_step(i)),
        Ok(()) => poll(i, ctx, std::time::Instant::now(), 0),
    }
}

/// Checks the condition of step `i` every 50 ms; it must hold three times in a row, so work the first check triggered
/// is seen as well and the window has been drawn since.
fn poll(i: usize, ctx: std::rc::Rc<Ctx>, started: std::time::Instant, held: usize) {
    let holds = STEPS.with(|s| match &s.borrow()[i].0 {
        Wait::Until(cond) => cond(&ctx),
        Wait::Fixed(_) => true,
    });
    let held = if holds { held + 1 } else { 0 };
    if held >= 3 {
        post_step(i);
    } else if started.elapsed().as_secs_f64() > UNTIL_TIMEOUT {
        eprintln!("SELFTEST SLOW step {i}: its condition did not hold within {UNTIL_TIMEOUT} s");
        post_step(i);
    } else {
        after(0.05, move || poll(i, ctx.clone(), started, held));
    }
}

/// The editor has applied the current text, rendered its formulas and diagrams and loaded its images.
fn settled(e: &EditorController) -> bool {
    let snap = e.snapshot();
    let Ok(st) = e.ivars().state.try_borrow() else {
        return false;
    };
    let dark = e.is_dark();
    !e.ivars().copy.update_scheduled.get()
        && st.is_current(&snap)
        && !st.stale.layout
        && super::js::renderer(e.mtm()).idle()
        && super::remote::idle()
        && crate::markdown::doc::figures(&st.doc)
            .iter()
            .all(|(_, f)| super::js::outcome(&super::update::render_key(f, dark)).is_some())
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
            let i = event.data1() as usize;
            let ctx = CTX.with(|c| c.borrow().clone()).unwrap();
            STEPS.with(|s| (s.borrow()[i].1)(&ctx));
            match STEPS.with(|s| s.borrow().len()) {
                n if i + 1 < n => wait_for(i + 1),
                _ => {
                    let snapshot = ctx.e.snapshot();
                    check(
                        "the patched text copy matches the source view",
                        snapshot.text == ctx.e.exact_text(),
                        &format!("{} vs {} bytes", snapshot.text.len(), ctx.e.exact_text().len()),
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

fn step(delay: f64, f: impl Fn(&Ctx) + 'static) -> Step {
    (Wait::Fixed(delay), Box::new(f))
}

/// A step that runs once `cond` holds.
fn when(cond: impl Fn(&Ctx) -> bool + 'static, f: impl Fn(&Ctx) + 'static) -> Step {
    (Wait::Until(Box::new(cond)), Box::new(f))
}

/// A step that runs once the editor has settled.
fn settle(f: impl Fn(&Ctx) + 'static) -> Step {
    when(|c| settled(&c.e), f)
}

/// A step that runs once the formula or diagram editor has closed and the editor has settled.
fn closed(f: impl Fn(&Ctx) + 'static) -> Step {
    when(|c| c.e.ivars().atoms.edit.borrow().is_none() && settled(&c.e), f)
}

fn run(mtm: MainThreadMarker, out: &Path) {
    let Some(e) = editor(mtm) else {
        check("open document", false, "no editor window");
        std::process::exit(1);
    };
    let window = e.window().unwrap();
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
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
    };
    if std::env::var_os("SPLAT_SELFTEST_SCENARIOS").is_some() {
        run_steps(ctx, scenarios::steps());
        return;
    }
    // The first step waits until formulas and diagrams are rendered, so the edits below do not race them.
    run_steps(
        ctx,
        vec![
            settle(|c| {
                let e = &c.e;
                e.window().unwrap().makeFirstResponder(Some(e.preview()));
                let heading_end = "Markdown-Kurzreferenz".encode_utf16().count();
                e.preview().setSelectedRange(NSRange::new(heading_end, 0));
                type_in_preview(e, " X");
                let s = source(e);
                check(
                    "typing in preview edits heading",
                    s.starts_with("# Markdown-Kurzreferenz X\n"),
                    &head(&s, 40),
                );
            }),
            step(0.15, |c| {
                super::dialogs::link(&c.e.window().unwrap(), "Beispieltext", |_, _| {})
            }),
            step(0.8, |c| {
                let window = c.e.window().unwrap();
                screen_image(&window, &c.out.join("14-link-sheet.png"));
                let Some(sheet) = window.attachedSheet() else {
                    check("link dialog fields span the sheet", false, "no sheet");
                    return;
                };
                let mut fields = Vec::new();
                if let Some(content) = sheet.contentView() {
                    editable_fields(&content, &mut fields);
                }
                let widths: Vec<f64> = fields.iter().map(|f| f.frame().size.width).collect();
                check(
                    "link dialog fields span the sheet",
                    widths.len() == 2 && widths.iter().all(|w| *w >= 300.0),
                    &format!("{widths:?}"),
                );
                window.endSheet_returnCode(&sheet, objc2_app_kit::NSAlertSecondButtonReturn);
                window.makeFirstResponder(Some(c.e.preview()));
            }),
            step(0.15, |c| unsafe {
                c.e.preview().doCommandBySelector(sel!(insertNewline:))
            }),
            step(0.15, |c| {
                type_in_preview(&c.e, "Neuer Absatz");
                let s = source(&c.e);
                check(
                    "return creates a paragraph",
                    s.starts_with("# Markdown-Kurzreferenz X\n\nNeuer Absatz"),
                    &head(&s, 60),
                );
            }),
            step(0.15, |c| {
                let e = &c.e;
                let p = storage(e.preview()).string().to_string();
                let start = p.encode_utf16().count() - p[p.find("Neuer Absatz").unwrap()..].encode_utf16().count();
                e.preview().setSelectedRange(NSRange::new(start, 5));
                let window = e.window().unwrap();
                let app = NSApplication::sharedApplication(e.mtm());
                // Without a key window (app started from a terminal) NSApp has no responder chain to walk, so this
                // walks the preview's own chain: text view, views, window, window controller.
                let _ = window.isKeyWindow() && unsafe { app.sendAction_to_from(sel!(mdBold:), None, None) }
                    || unsafe { e.preview().tryToPerform_with(sel!(mdBold:), None) };
                let s = source(e);
                check(
                    "bold from toolbar action in preview",
                    s.contains("\n\n**Neuer** Absatz"),
                    &format!("{:?}", head(&s, 70)),
                );
            }),
            step(0.15, |c| {
                if let Some(undo) = c.e.undo_manager() {
                    undo.undo();
                }
                let s = source(&c.e);
                check(
                    "undo removes bold only",
                    s.contains("X\n\nNeuer Absatz") && !s.contains("**Neuer**"),
                    &format!("{:?}", head(&s, 70)),
                );
            }),
            // Like selecting with the mouse: the other view's caret is already where the selection starts, so
            // nothing else redraws the spot.
            step(0.15, |c| {
                let e = &c.e;
                let (preview_at, _) = neuer_absatz(e);
                e.window().unwrap().makeFirstResponder(Some(e.preview()));
                e.preview().setSelectedRange(NSRange::new(preview_at, 0));
            }),
            step(0.3, |c| {
                let e = &c.e;
                let (preview_at, source_at) = neuer_absatz(e);
                e.preview().setSelectedRange(NSRange::new(preview_at, 5));
                check(
                    "a preview selection shows in the source in the selection colour",
                    e.ivars().marks.source.get() == Some(NSRange::new(source_at, 5))
                        && e.source().selectedRange().length == 0,
                    &format!(
                        "{:?}, real {:?}",
                        e.ivars().marks.source.get(),
                        e.source().selectedRange()
                    ),
                );
            }),
            step(0.4, |c| {
                let e = &c.e;
                let image = c.out.join("12-selection-from-preview.png");
                screen_image(&e.window().unwrap(), &image);
                let (preview_at, source_at) = neuer_absatz(e);
                same_selection_colour(
                    "the source shows the preview selection on screen",
                    &image,
                    (e.preview(), preview_at),
                    (e.source(), source_at),
                );
                e.window().unwrap().makeFirstResponder(Some(e.source()));
                e.source().setSelectedRange(NSRange::new(source_at, 0));
            }),
            step(0.3, |c| {
                let e = &c.e;
                let (preview_at, source_at) = neuer_absatz(e);
                e.source().setSelectedRange(NSRange::new(source_at, 5));
                check(
                    "a source selection shows in the preview",
                    e.ivars().marks.preview.get() == Some(NSRange::new(preview_at, 5))
                        && e.ivars().marks.source.get().is_none(),
                    &format!("{:?}", e.ivars().marks.preview.get()),
                );
            }),
            step(0.4, |c| {
                let e = &c.e;
                let image = c.out.join("12-selection-from-source.png");
                screen_image(&e.window().unwrap(), &image);
                let (preview_at, source_at) = neuer_absatz(e);
                same_selection_colour(
                    "the preview shows the source selection on screen",
                    &image,
                    (e.source(), source_at),
                    (e.preview(), preview_at),
                );
                e.window().unwrap().makeFirstResponder(Some(e.preview()));
                e.preview().setSelectedRange(NSRange::new(preview_at, 5));
            }),
            step(0.15, |c| {
                type_in_preview(&c.e, "*");
                let s = source(&c.e);
                check(
                    "typed markdown characters are escaped",
                    s.contains("\\*"),
                    &head(&s, 80),
                );
            }),
            step(0.15, |c| {
                let e = &c.e;
                let p = storage(e.preview()).string().to_string();
                let Some(cell) = p.find("Ergebnis").map(|b| p[..b].encode_utf16().count()) else {
                    check("tab moves to next table cell", false, "table not found");
                    return;
                };
                e.preview().setSelectedRange(NSRange::new(cell, 0));
                unsafe { e.preview().doCommandBySelector(sel!(insertTab:)) };
                let sel = e.preview().selectedRange();
                let text = storage(e.preview()).string().to_string();
                let selected: String = text
                    .encode_utf16()
                    .skip(sel.location)
                    .take(sel.length)
                    .map(|u| char::from_u32(u as u32).unwrap_or('?'))
                    .collect();
                check(
                    "tab moves to next table cell",
                    selected == "Schreibweise",
                    &format!("selected {selected:?}"),
                );
            }),
            step(0.15, |c| {
                let e = &c.e;
                let s = source(e);
                let task = s.find("- [ ] Offene").unwrap_or(0);
                let p = storage(e.preview()).string().to_string();
                let Some(b) = p.find("☐\tOffene") else {
                    check("checkbox click toggles task", false, "checkbox not rendered");
                    return;
                };
                let idx = p[..b].encode_utf16().count();
                let link: Retained<NSString> = ns("splat-task");
                e.link_clicked(e.preview(), &obj(link), idx);
                let s2 = source(e);
                check(
                    "checkbox click toggles task",
                    s2[task..].starts_with("- [x] Offene"),
                    &head(&s2[task..], 14),
                );
            }),
            settle(|c| {
                let e = &c.e;
                let p = storage(e.preview()).string().to_string();
                let unfold = |marker: &str, name: &str| {
                    let Some(at) = p.find(marker) else {
                        check(name, false, &format!("{marker} not rendered"));
                        return;
                    };
                    let link: Retained<NSString> = ns("splat-fold");
                    e.link_clicked(e.preview(), &obj(link), p[..at].encode_utf16().count());
                };
                unfold("▸", "a click on ▸ unfolds the callout");
                let s = source(e);
                check(
                    "a click on ▸ unfolds the callout",
                    s.contains("> [!tip]+") && !s.contains("> [!tip]-"),
                    "",
                );
            }),
            settle(|c| {
                let e = &c.e;
                let p = storage(e.preview()).string().to_string();
                let shown = p.contains("Ein Klick auf ▸ klappt die Box auf.");
                check("the unfolded callout shows its content", shown, "");
                let Some(at) = p.find("▸ HTML") else {
                    check("a click opens the HTML details", false, "details not rendered");
                    return;
                };
                let link: Retained<NSString> = ns("splat-fold");
                e.link_clicked(e.preview(), &obj(link), p[..at].encode_utf16().count());
                check(
                    "a click opens the HTML details",
                    source(e).contains("<details open>"),
                    "",
                );
            }),
            settle(|c| {
                let p = storage(c.e.preview()).string().to_string();
                check(
                    "opened details show their Markdown",
                    p.contains("HTML wie in GitHub-READMEs wird dargestellt"),
                    "",
                );
                check(
                    "extended syntax renders",
                    p.contains("Textmarker") && !p.contains("==Textmarker==") && p.contains("🚀") && !p.contains("%%"),
                    "",
                );
            }),
            step(0.15, |c| {
                let e = &c.e;
                let window = e.window().unwrap();
                snapshot(&window, &c.out.join("02-edited.png"));
                window.makeFirstResponder(Some(e.source()));
                e.set_zoom(1.0);
                snapshot(&window, &c.out.join("03-source-focus.png"));
                export_pdf(e, &c.out.join("export.pdf"));
            }),
            step(0.5, |c| {
                let e = &c.e;
                let p = storage(e.preview()).string().to_string();
                let attachments = p.matches('\u{FFFC}').count();
                check(
                    "formulas and diagram rendered as images",
                    attachments >= 3,
                    &format!("{attachments} attachments"),
                );
                if let Some(b) = p.find("Formeln und Diagramme") {
                    let idx = p[..b].encode_utf16().count();
                    e.preview().scrollRangeToVisible(NSRange::new(idx, 0));
                    let window = e.window().unwrap();
                    window.makeFirstResponder(Some(e.preview()));
                    snapshot(&window, &c.out.join("04-formulas.png"));
                }
            }),
            step(0.15, |c| {
                let e = &c.e;
                let Some(view) = open_formula_editor(e) else {
                    check("formula popover opens", false, "no popover");
                    return;
                };
                check(
                    "formula popover shows TeX",
                    view.string().to_string() == "E = mc^2",
                    &view.string().to_string(),
                );
                view.setString(&ns("E = mc^3"));
                e.atom_text_changed();
                check("popover edits apply live", source(e).contains("$E = mc^3$"), "");
                close_popover(e);
            }),
            closed(|c| check("closing the popover commits", source(&c.e).contains("$E = mc^3$"), "")),
            step(0.15, |c| {
                if let Some(undo) = c.e.undo_manager() {
                    undo.undo();
                }
                let s = source(&c.e);
                let at = s.find("$E = mc").unwrap_or(0);
                check(
                    "formula edit is one undo step",
                    s.contains("$E = mc^2$"),
                    &format!(
                        "{:?}",
                        &s[s.floor_char_boundary(at.saturating_sub(40))..s.ceil_char_boundary((at + 40).min(s.len()))]
                    ),
                );
            }),
            step(0.15, |c| {
                let e = &c.e;
                let Some(view) = open_formula_editor(e) else {
                    check("formula popover reopens", false, "no popover");
                    return;
                };
                view.setString(&ns("x"));
                e.atom_text_changed();
                e.atom_command(sel!(cancelOperation:));
            }),
            closed(|c| {
                check(
                    "escape discards the popover edit",
                    source(&c.e).contains("$E = mc^2$"),
                    "",
                )
            }),
            step(0.15, |c| {
                let e = &c.e;
                e.window().unwrap().makeFirstResponder(Some(e.preview()));
                let len = storage(e.preview()).length();
                e.preview().setSelectedRange(NSRange::new(len, 0));
                perform(e, sel!(mdInsertFormula:), 0);
                let open = e.ivars().atoms.edit.borrow().as_ref().map(|a| a.popover.isShown());
                check(
                    "inserting a formula opens its editor",
                    open == Some(true),
                    &format!("{open:?}"),
                );
                if let Some(view) = e.ivars().atoms.edit.borrow().as_ref().map(|a| a.view.clone()) {
                    view.setString(&ns("a^2 + b^2"));
                }
                e.atom_text_changed();
                close_popover(e);
            }),
            closed(|c| {
                let e = &c.e;
                check(
                    "the new formula is kept when its editor closes",
                    source(e).contains("$$\na^2 + b^2\n$$"),
                    &tail(&source(e), 60),
                );
                let before = source(e);
                perform(e, sel!(mdInsertFormula:), 0);
                check(
                    "an empty formula block is inserted",
                    source(e).len() > before.len(),
                    &tail(&source(e), 60),
                );
                // Like clicking into the source: it takes the focus and the editor closes.
                e.window().unwrap().makeFirstResponder(Some(e.source()));
                close_popover(e);
            }),
            closed(|c| {
                let e = &c.e;
                let s = source(e);
                let responder = e.window().unwrap().firstResponder();
                let responder: Option<&objc2::runtime::AnyObject> = responder.as_deref().map(|r| r.as_ref());
                check(
                    "a click into the source keeps the new formula and the focus",
                    s.trim_end().ends_with("$$\n\n$$") && e.is_source(responder),
                    &tail(&s, 60),
                );
                set_source_caret(e, s.len());
                perform(e, sel!(mdInsertFormula:), 0);
                e.cancel_atom_edit();
            }),
            closed(|c| {
                let e = &c.e;
                let s = source(e);
                check(
                    "escape removes a new empty formula again",
                    s.trim_end().ends_with("$$\n\n$$") && s.matches("$$\n\n$$").count() == 1,
                    &tail(&s, 60),
                );
                // Back to the document before the formulas: insert, edit, insert, insert, remove.
                if let Some(undo) = e.undo_manager() {
                    for _ in 0..5 {
                        undo.undo();
                    }
                }
            }),
            step(0.15, |c| check_context_menu(&c.e)),
            step(0.15, |c| {
                let e = &c.e;
                let src = source(e);
                let target = src.find("## Tabellen").unwrap();
                let map = crate::markdown::text::Utf16Map::new(&src);
                let y = super::scroll::char_rect(e.source(), map.to_u16(target)).origin.y;
                let clip = e.source().enclosingScrollView().unwrap().contentView();
                clip.scrollToPoint(objc2_foundation::NSPoint::new(0.0, y));
                e.source().enclosingScrollView().unwrap().reflectScrolledClipView(&clip);
            }),
            step(0.5, |c| {
                let e = &c.e;
                let p = storage(e.preview()).string().to_string();
                let top = e.preview().visibleRect().origin.y;
                let tables = p.find("Tabellen\n").map(|b| p[..b].encode_utf16().count()).unwrap_or(0);
                let ty = super::scroll::char_rect(e.preview(), tables).origin.y;
                let bottom = e.preview().bounds().size.height - e.preview().visibleRect().size.height;
                check(
                    "scrolling the source scrolls the preview",
                    (top - ty).abs() < 120.0 || (ty > top && top >= bottom - 2.0),
                    &format!("preview top {top:.0}, heading at {ty:.0}"),
                );
                check(
                    "source view stays on TextKit 2",
                    e.source().textLayoutManager().is_some(),
                    "",
                );
                check_link_dialog(e.mtm(), &c.out.join("13-link-dialog.png"));
                let app = NSApplication::sharedApplication(e.mtm());
                app.setAppearance(
                    objc2_app_kit::NSAppearance::appearanceNamed(unsafe { objc2_app_kit::NSAppearanceNameDarkAqua })
                        .as_deref(),
                );
            }),
            settle(|c| {
                let e = &c.e;
                snapshot(&e.window().unwrap(), &c.out.join("05-dark.png"));
                let p = storage(e.preview()).string().to_string();
                check(
                    "dark mode keeps rendered formulas",
                    p.matches('\u{FFFC}').count() >= 3,
                    "",
                );
                append(e, "\n\n![Badge](https://img.shields.io/badge/splat-ok-blue.svg)\n");
                check(
                    "images from the web wait for permission",
                    e.ivars().remote_banner.get().is_some_and(|b| !b.isHidden()),
                    "",
                );
                e.allow_remote_images();
            }),
            settle(|c| {
                let store = storage(c.e.preview());
                let p = store.string().to_string();
                let index = p
                    .encode_utf16()
                    .enumerate()
                    .filter(|(_, u)| *u == 0xFFFC)
                    .map(|(i, _)| i)
                    .last();
                let width = index
                    .and_then(|i| {
                        let value = unsafe {
                            store.attribute_atIndex_effectiveRange(
                                objc2_app_kit::NSAttachmentAttributeName,
                                i,
                                std::ptr::null_mut(),
                            )
                        };
                        value.and_then(|v| v.downcast::<objc2_app_kit::NSTextAttachment>().ok())
                    })
                    .and_then(|a| a.image())
                    .map(|img| img.size().width)
                    .unwrap_or(0.0);
                check("remote image loads", width > 40.0, &format!("attachment width {width}"));
                append(&c.e, "\n| a | b | c |\n|---|---|---|\n|   |   |   |");
            }),
            step(0.15, |c| {
                let store = storage(c.e.preview());
                let p = store.string().to_string();
                let cells = (0..store.length())
                    .filter(|&i| p.encode_utf16().nth(i) == Some(u16::from(b'\n')))
                    .filter(|&i| {
                        let style = unsafe {
                            store.attribute_atIndex_effectiveRange(
                                objc2_app_kit::NSParagraphStyleAttributeName,
                                i,
                                std::ptr::null_mut(),
                            )
                        };
                        style
                            .and_then(|v| v.downcast::<objc2_app_kit::NSParagraphStyle>().ok())
                            .is_some_and(|ps| {
                                ps.textBlocks().iter().any(|b| {
                                    b.downcast::<objc2_app_kit::NSTextTableBlock>()
                                        .is_ok_and(|t| t.table().numberOfColumns() == 3 && t.startingRow() == 1)
                                })
                            })
                    })
                    .count();
                let tail: String = p.chars().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect();
                check(
                    "empty last table row keeps all cells",
                    cells >= 3,
                    &format!("{cells} cells, tail {tail:?}"),
                );
                append(&c.e, "\n\nLeer Test\n\nAm Ende Wort\n");
            }),
            step(0.3, |c| {
                let e = &c.e;
                e.window().unwrap().makeFirstResponder(Some(e.preview()));
                let p = storage(e.preview()).string().to_string();
                let b = p.rfind("Leer Test").unwrap();
                let end = p[..b].encode_utf16().count() + "Leer Test".encode_utf16().count();
                e.preview().setSelectedRange(NSRange::new(end, 0));
                type_in_preview(e, " ");
            }),
            step(0.15, |c| {
                type_in_preview(&c.e, "z");
                let s = source(&c.e);
                check(
                    "a space typed at a paragraph end stays before the next letter (preview)",
                    s.contains("Leer Test z\n"),
                    &tail(&s, 40),
                );
            }),
            step(0.3, |c| {
                let e = &c.e;
                e.window().unwrap().makeFirstResponder(Some(e.preview()));
                let p = storage(e.preview()).string().to_string();
                let Some(b) = p.rfind("Am Ende Wort") else {
                    check(
                        "bold at a word end waits for typing (preview)",
                        false,
                        "paragraph not rendered",
                    );
                    return;
                };
                let end = p[..b].encode_utf16().count() + "Am Ende Wort".encode_utf16().count();
                e.preview().setSelectedRange(NSRange::new(end, 0));
                let _ = unsafe { e.preview().tryToPerform_with(sel!(mdBold:), None) };
                check(
                    "bold at a word end waits for typing (preview)",
                    source(e).contains("Am Ende Wort\n") && e.active().bold,
                    &tail(&source(e), 30),
                );
                type_in_preview(e, " ");
            }),
            step(0.15, |c| type_in_preview(&c.e, "n")),
            step(0.15, |c| {
                type_in_preview(&c.e, "eu");
                let s = source(&c.e);
                check(
                    "bold at a word end formats the next word (preview)",
                    s.contains("Am Ende Wort **neu**"),
                    &tail(&s, 30),
                );
            }),
            step(0.15, |c| {
                let _ = unsafe { c.e.preview().tryToPerform_with(sel!(mdBold:), None) };
                type_in_preview(&c.e, "!");
                let s = source(&c.e);
                check(
                    "bold again at the end of bold text stops it (preview)",
                    s.contains("Am Ende Wort **neu**!"),
                    &tail(&s, 30),
                );
                append(&c.e, "\nQuelle Wort\n");
            }),
            step(0.3, |c| {
                let e = &c.e;
                e.window().unwrap().makeFirstResponder(Some(e.source()));
                let s = source(e);
                let end = s.rfind("Quelle Wort").unwrap() + "Quelle Wort".len();
                let map = crate::markdown::text::Utf16Map::new(&s);
                e.source().setSelectedRange(NSRange::new(map.to_u16(end), 0));
                let _ = unsafe { e.source().tryToPerform_with(sel!(mdBold:), None) };
                type_in_source(e, " ");
            }),
            step(0.15, |c| type_in_source(&c.e, "x")),
            step(0.15, |c| {
                type_in_source(&c.e, "y");
                let s = source(&c.e);
                check(
                    "bold at a word end formats the next word (source)",
                    s.contains("Quelle Wort **xy**"),
                    &tail(&s, 30),
                );
            }),
            step(0.15, |c| {
                if let Some(undo) = c.e.undo_manager() {
                    undo.undo();
                }
            }),
            step(0.15, |c| {
                if let Some(undo) = c.e.undo_manager() {
                    undo.undo();
                }
                let s = source(&c.e);
                let line = s[s.rfind("Quelle Wort").unwrap()..]
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                check(
                    "undo takes back typed text together with its markers",
                    !line.contains('*') && !line.contains('x'),
                    &line,
                );
            }),
            step(0.15, |c| {
                let e = &c.e;
                let ids: Vec<String> = e
                    .window()
                    .and_then(|w| w.toolbar())
                    .map(|t| t.items().iter().map(|i| i.itemIdentifier().to_string()).collect())
                    .unwrap_or_default();
                check(
                    "toolbar offers new tab, share and more formats",
                    ["group.character", "group.paragraph", "group.insert", "share", "new"]
                        .iter()
                        .all(|id| ids.iter().any(|i| i == id)),
                    &format!("{ids:?}"),
                );
                window_image(&e.window().unwrap(), &c.out.join("06-toolbar.png"));
                let path = e.share_file(true).and_then(|u| u.path()).map(|p| p.to_string());
                let content = path.as_deref().and_then(|p| std::fs::read_to_string(p).ok());
                check(
                    "share hands over the current text as a .md file",
                    path.as_deref().is_some_and(|p| p.ends_with(".md")) && content == Some(source(e)),
                    &format!("{path:?}"),
                );
                let count = || e.window().and_then(|w| w.tabbedWindows()).map_or(1, |t| t.count());
                let before = count();
                let _ = unsafe { e.source().tryToPerform_with(sel!(mdNewTab:), None) };
                let after = count();
                check(
                    "plus opens a new document as a tab",
                    after == before + 1,
                    &format!("{before} -> {after} tabs"),
                );
            }),
            step(0.5, |c| {
                let window = c.e.window().unwrap();
                window_image(&window, &c.out.join("07-tabs.png"));
                if let Some(tabs) = window.tabbedWindows() {
                    for w in tabs.iter().filter(|w| !std::ptr::eq(&**w, &*window)) {
                        w.performClose(None);
                    }
                }
            }),
            step(0.5, |c| {
                let e = &c.e;
                e.window().unwrap().makeKeyAndOrderFront(None);
                append(e, "\n\n## Neue Formate\n\nEin wichtiges Wort hier.\n");
                select_in_source(e, "wichtiges");
                perform(e, sel!(mdHighlight:), 0);
                check(
                    "highlight wraps the selection in <mark>",
                    source(e).contains("Ein <mark>wichtiges</mark> Wort"),
                    &tail(&source(e), 60),
                );
                select_in_source(e, "Wort hier");
                perform(e, sel!(mdInsertAlert:), 3);
                check(
                    "alert box wraps the paragraph",
                    source(e).contains("> [!WARNING]\n> Ein <mark>"),
                    &tail(&source(e), 80),
                );
            }),
            step(0.3, |c| {
                let e = &c.e;
                let p = storage(e.preview()).string().to_string();
                check(
                    "alert box renders its German title",
                    p.contains("\u{FFFC} Warnung\n"),
                    &format!("{:?}", tail(&p, 80)),
                );
                let end = source(e).len();
                set_source_caret(e, end);
                perform(e, sel!(mdCodeBlock:), 13);
                check(
                    "code block with a language",
                    source(e).contains("```python\n"),
                    &tail(&source(e), 40),
                );
                type_in_source(e, "def f(x):  # Kommentar\n    return 42");
                let end = source(e).len();
                set_source_caret(e, end);
                perform(e, sel!(mdInsertToc:), 0);
                let s = source(e);
                check(
                    "table of contents links the headings",
                    s.contains("- [Neue Formate](#neue-formate)") && s.contains("](#tabellen)"),
                    &tail(&s, 120),
                );
                e.reveal_heading("tabellen");
                let at = Utf16Map::new(&s).to_byte(e.source().selectedRange().location);
                check(
                    "anchor link jumps to its heading",
                    s[at..].starts_with("## Tabellen"),
                    &head(&s[at..], 20),
                );
            }),
            step(0.3, |c| {
                let e = &c.e;
                set_source_caret(e, 0);
                let Some(at) = preview_link(e, "#tabellen") else {
                    check(
                        "⌘-click on a link to a heading jumps there",
                        false,
                        "no link to #tabellen in the preview",
                    );
                    return;
                };
                e.preview().scrollRangeToVisible(NSRange::new(at, 1));
                click_preview(e, at, objc2_app_kit::NSEventModifierFlags::Command);
            }),
            // AppKit reports a click on a link once a double click can no longer follow.
            step(1.0, |c| {
                let s = source(&c.e);
                let at = Utf16Map::new(&s).to_byte(c.e.source().selectedRange().location);
                check(
                    "⌘-click on a link to a heading jumps there",
                    s[at..].starts_with("## Tabellen"),
                    &head(&s[at..], 20),
                );
            }),
            step(0.5, |c| {
                let e = &c.e;
                let window = e.window().unwrap();
                window.makeFirstResponder(Some(e.preview()));
                if let Some(b) = storage(e.preview()).string().to_string().find("def f") {
                    let idx = storage(e.preview()).string().to_string()[..b].encode_utf16().count();
                    e.preview().scrollRangeToVisible(NSRange::new(idx, 0));
                }
                window_image(&window, &c.out.join("08-formats.png"));
                let Some(view) = open_formula_editor(e) else {
                    check("formula building blocks", false, "no popover");
                    return;
                };
                e.set_atom_visual(false);
                unsafe { view.selectAll(None) };
                e.atom_snippet(0);
                check(
                    "fraction button inserts a template with the selection",
                    view.string().to_string() == "\\frac{E = mc^2}{□}" && source(e).contains("$\\frac{E = mc^2}{□}$"),
                    &view.string().to_string(),
                );
                view.setSelectedRange(NSRange::new(0, 0));
                let jumped = e.atom_tab(false);
                let sel = view.selectedRange();
                check(
                    "tab selects the next placeholder",
                    jumped && sel.length == 1,
                    &format!("{sel:?}"),
                );
                e.set_atom_visual(true);
            }),
            when(
                |c| super::mathfield::field(c.e.mtm()).loaded.get(),
                |c| {
                    let e = &c.e;
                    let field = super::mathfield::field(e.mtm());
                    check("visual formula editor loads", field.loaded.get(), "");
                    field.run(
                        "mf.value = latex; mf.dispatchEvent(new Event('input', { bubbles: true }));",
                        "x^2+\\sqrt{y}",
                    );
                },
            ),
            step(0.8, |c| {
                let e = &c.e;
                if let Some(w) = super::mathfield::field(e.mtm()).web.window() {
                    screen_image(&w, &c.out.join("11-visual-formula.png"));
                }
                check(
                    "visual edits reach the document",
                    source(e).contains("$x^2+\\sqrt{y}$"),
                    &tail(&source(e), 40),
                );
            }),
            step(0.5, |c| c.e.cancel_atom_edit()),
            closed(|c| {
                let e = &c.e;
                check("escape restores the formula", source(e).contains("$E = mc^2$"), "");
                let s = source(e);
                let at = s.find("flowchart LR").unwrap();
                e.window().unwrap().makeFirstResponder(Some(e.source()));
                set_source_caret(e, at + 2);
                e.edit_atom_at_selection();
                let Some(view) = e.ivars().atoms.edit.borrow().as_ref().map(|a| a.view.clone()) else {
                    check("diagram building blocks", false, "no popover");
                    return;
                };
                let len = view.string().length();
                view.setSelectedRange(NSRange::new(len, 0));
                e.atom_snippet(super::atom::diagram_tag(crate::markdown::snippets::DiagramAction::Edge));
                check(
                    "connection button continues from the last node",
                    view.string().to_string().ends_with("C --> D[Text]"),
                    &view.string().to_string(),
                );
            }),
            step(0.3, |c| {
                let e = &c.e;
                if let Some(w) = e.ivars().atoms.edit.borrow().as_ref().and_then(|a| a.view.window()) {
                    window_image(&w, &c.out.join("10-diagram-popover.png"));
                }
                let view = e.ivars().atoms.edit.borrow().as_ref().map(|a| a.view.clone());
                if let Some(view) = view {
                    view.setString(&ns("flowchart LR\n    A --> "));
                    e.atom_text_changed();
                }
            }),
            settle(|c| {
                let e = &c.e;
                let status = e
                    .ivars()
                    .atoms
                    .edit
                    .borrow()
                    .as_ref()
                    .map(|a| a.status.stringValue().to_string())
                    .unwrap_or_default();
                check(
                    "diagram popover shows the Mermaid error",
                    status.starts_with("⚠︎"),
                    &status,
                );
                e.cancel_atom_edit();
            }),
            step(0.6, |c| {
                let e = &c.e;
                append(e, "\n\nLeerzeilen Absatz\n");
                e.window().unwrap().makeFirstResponder(Some(e.preview()));
                let p = storage(e.preview()).string().to_string();
                let b = p.rfind("Leerzeilen Absatz").unwrap();
                let end = p[..b].encode_utf16().count() + "Leerzeilen Absatz".encode_utf16().count();
                e.preview().setSelectedRange(NSRange::new(end, 0));
                unsafe { e.preview().doCommandBySelector(sel!(insertNewline:)) };
            }),
            step(0.15, |c| unsafe {
                c.e.preview().doCommandBySelector(sel!(insertNewline:))
            }),
            step(0.15, |c| {
                let e = &c.e;
                let s = source(e);
                let p = storage(e.preview()).string().to_string();
                let lines_after = p[p.rfind("Leerzeilen Absatz").unwrap()..].matches('\n').count();
                check(
                    "return twice in the preview leaves two empty lines",
                    s.ends_with("Leerzeilen Absatz\n\n\n\n\n") && lines_after >= 3,
                    &format!("{:?} / {lines_after} line ends", tail(&s, 30)),
                );
                unsafe { e.preview().doCommandBySelector(sel!(deleteBackward:)) };
                type_in_preview(e, "Danach");
                let s = source(e);
                check(
                    "backspace removes one empty line, typing fills the other",
                    s.ends_with("Leerzeilen Absatz\n\nDanach\n"),
                    &format!("{:?}", tail(&s, 40)),
                );
            }),
            step(0.3, |c| {
                let e = &c.e;
                e.window().unwrap().makeFirstResponder(Some(e.preview()));
                let p = storage(e.preview()).string().to_string();
                let at = p[..p.find("Inline-Formel: ").unwrap()].encode_utf16().count() + "Inline-Formel: ".len();
                e.preview().setSelectedRange(NSRange::new(at + 1, 0));
                check(
                    "edit command is available right behind a formula",
                    e.validate(Some(sel!(mdEditAtom:)), 0, None) && e.ivars().atoms.edit.borrow().is_none(),
                    "",
                );
                e.preview().setSelectedRange(NSRange::new(at, 0));
            }),
            step(0.3, |c| {
                let e = &c.e;
                check(
                    "caret on a formula opens its editor",
                    e.ivars().atoms.edit.borrow().is_some(),
                    "",
                );
                e.cancel_atom_edit();
            }),
            step(0.6, |c| {
                let e = &c.e;
                let at = e.preview().selectedRange().location;
                e.preview().setSelectedRange(NSRange::new(at, 0));
            }),
            step(0.3, |c| {
                let e = &c.e;
                check(
                    "a closed editor does not reopen while the caret stays",
                    e.ivars().atoms.edit.borrow().is_none(),
                    "",
                );
            }),
            step(0.3, |c| {
                if let Some(document) = c.e.document() {
                    unsafe { document.saveDocument(None) };
                }
            }),
            when(
                |c| c.e.document().is_some_and(|d| !d.isDocumentEdited()),
                |c| {
                    let e = &c.e;
                    let s = source(e);
                    set_source_caret(e, s.find("## Tabellen").unwrap_or(0));
                    let path = std::env::var("SPLAT_SELFTEST_FILE").unwrap_or_default();
                    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
                    // Written without file coordination, as Git or a shell would.
                    let _ = std::fs::write(&path, format!("{on_disk}\nVon außen geändert\n"));
                },
            ),
            when(
                |c| source(&c.e).contains("Von außen geändert"),
                |c| {
                    let e = &c.e;
                    let s = source(e);
                    let at = Utf16Map::new(&s).to_byte(e.source().selectedRange().location);
                    check(
                        "an unedited document reloads when another program changes its file",
                        s.ends_with("Von außen geändert\n"),
                        &tail(&s, 40),
                    );
                    check(
                        "reloading keeps the caret",
                        s[at..].starts_with("## Tabellen"),
                        &head(&s[at..], 20),
                    );
                    check(
                        "reloading leaves no undo steps into the old text",
                        e.document().and_then(|d| d.undoManager()).is_some_and(|u| !u.canUndo()),
                        "",
                    );
                    let Some(view) = open_formula_editor(e) else {
                        check("an open formula edit counts as unsaved", false, "no popover");
                        return;
                    };
                    view.setString(&ns("x"));
                    e.atom_text_changed();
                    check(
                        "an open formula edit counts as unsaved",
                        e.document().is_some_and(|d| d.isDocumentEdited()),
                        "",
                    );
                    e.atom_command(sel!(cancelOperation:));
                },
            ),
            closed(|c| {
                check(
                    "a discarded formula edit leaves the document unedited",
                    c.e.document().is_some_and(|d| !d.isDocumentEdited()),
                    "",
                );
                c.e.toggle_preview();
            }),
            step(0.8, |c| {
                let e = &c.e;
                set_source_caret(e, 0);
                type_in_source(e, "Äußerst übersichtlich: ");
            }),
            step(0.5, |c| c.e.toggle_preview()),
            settle(|c| {
                let e = &c.e;
                let preview = storage(e.preview()).string().to_string();
                check(
                    "a preview shown again has the text typed while it was hidden",
                    preview.starts_with("Äußerst übersichtlich: "),
                    &head(&preview, 40),
                );
                check(
                    "scroll positions follow the text typed while the preview was hidden",
                    e.scroll_spans().is_some_and(|spans| !spans.is_empty()),
                    "",
                );
                // Graphviz lays out this graph for minutes; the renderer must give up and render the next formula.
                let edges: String = (0..6000)
                    .map(|i| format!("n{} -> n{};", (i * 7919) % 600, (i * 104_729 + 13) % 600))
                    .collect();
                super::js::renderer(e.mtm()).request(vec![super::js::Key::Diagram {
                    source: format!("digraph {{ {edges} }}"),
                    kind: crate::markdown::doc::DiagramKind::Graphviz,
                    dark: false,
                }]);
            }),
            step(12.0, |c| {
                super::js::renderer(c.e.mtm()).request(vec![hang_check_formula()]);
            }),
            when(
                |_| super::js::outcome(&hang_check_formula()).is_some(),
                |_| {
                    let outcome = super::js::outcome(&hang_check_formula());
                    check(
                        "formulas render again after a diagram that never finishes",
                        outcome == Some(Ok(())),
                        &format!("{outcome:?}"),
                    );
                },
            ),
        ]
        .into_iter()
        .chain(scenarios::steps())
        .collect(),
    );
}

fn hang_check_formula() -> super::js::Key {
    super::js::Key::Math {
        tex: "a^2 + b^2 = c^2".into(),
        display: false,
        dark: false,
    }
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
        .setSelectedRange(NSRange::new(Utf16Map::new(&s).to_u16(at), 0));
}

fn select_in_source(e: &EditorController, needle: &str) {
    let s = source(e);
    let at = s.rfind(needle).unwrap();
    let map = Utf16Map::new(&s);
    e.window().unwrap().makeFirstResponder(Some(e.source()));
    e.source()
        .setSelectedRange(NSRange::new(map.to_u16(at), needle.encode_utf16().count()));
}

fn timed(label: &str, f: impl FnOnce()) {
    let t = std::time::Instant::now();
    f();
    eprintln!("BENCH {label}: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
}

/// Measures the hot paths on the opened benchmark file and quits.
fn bench(mtm: MainThreadMarker) {
    let Some(e) = editor(mtm) else {
        eprintln!("BENCH no editor");
        std::process::exit(1);
    };
    let text = source(&e);
    eprintln!("BENCH size: {} bytes, {} lines", text.len(), text.lines().count());
    eprintln!(
        "BENCH source on TextKit 2: {}",
        e.source().textLayoutManager().is_some()
    );
    let parsed = std::cell::RefCell::new(None);
    timed("parse + colors (background part 1)", || {
        *parsed.borrow_mut() = Some(crate::markdown::incremental::parse_and_color(&text))
    });
    let parsed = parsed.into_inner().unwrap();
    timed("preview layout (background part 2)", || {
        let _ = crate::preview::layout::layout(
            &text,
            &parsed.doc,
            &super::update::Images {
                dark: false,
                env: Default::default(),
            },
            None,
        );
    });
    let mut at = text.len() / 2;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let map = Utf16Map::new(&text);
    e.window().unwrap().makeFirstResponder(Some(e.source()));
    e.source().setSelectedRange(NSRange::new(map.to_u16(at), 0));
    timed("keystroke in the middle + synchronous update", || {
        type_in_source(&e, "x");
        e.update();
    });
    timed("keystroke in the source view alone", || type_in_source(&e, "y"));
    timed("handing the text to the background", || e.update_in_background());
    timed("15 toolbar validations after the keystroke", || {
        for _ in 0..15 {
            e.validate(Some(sel!(mdBold:)), 0, None);
        }
    });
    let clip = e.source().enclosingScrollView().unwrap().contentView();
    let y = e.source().bounds().size.height / 2.0;
    timed("scroll source to the middle + sync preview", || {
        clip.scrollToPoint(objc2_foundation::NSPoint::new(0.0, y));
        e.source().enclosingScrollView().unwrap().reflectScrolledClipView(&clip);
    });
    for fraction in [0.25, 0.75, 0.1, 0.9, 0.5] {
        let y = e.source().bounds().size.height * fraction;
        e.ivars().scrolling.set(true);
        timed(
            &format!("jump source to {:.0} % without sync, with redraw", fraction * 100.0),
            || {
                clip.scrollToPoint(objc2_foundation::NSPoint::new(0.0, y));
                e.source().enclosingScrollView().unwrap().reflectScrolledClipView(&clip);
                e.window().unwrap().displayIfNeeded();
            },
        );
        e.ivars().scrolling.set(false);
        let note = unsafe {
            objc2_foundation::NSNotification::notificationWithName_object(
                objc2_app_kit::NSViewBoundsDidChangeNotification,
                Some(&clip),
            )
        };
        timed("  sync of the preview to it", || e.sync_scroll(&note));
    }
    let pending = e
        .ivars()
        .state
        .borrow()
        .blocks
        .placeholders
        .iter()
        .filter(|p| **p)
        .count();
    let blocks = e.ivars().state.borrow().layout.blocks.len();
    eprintln!("BENCH preview blocks: {blocks}, still plain text: {pending}");
    for (n, fraction) in [0.3, 0.6, 0.85, 0.15].into_iter().enumerate() {
        after(3.0 + n as f64 * 0.6, move || {
            let e = editor(MainThreadMarker::new().unwrap()).unwrap();
            let clip = e.source().enclosingScrollView().unwrap().contentView();
            clip.scrollToPoint(objc2_foundation::NSPoint::new(
                0.0,
                e.source().bounds().size.height * fraction,
            ));
            e.source().enclosingScrollView().unwrap().reflectScrolledClipView(&clip);
        });
        after(3.4 + n as f64 * 0.6, move || {
            let e = editor(MainThreadMarker::new().unwrap()).unwrap();
            let tlm = e.source().textLayoutManager().unwrap();
            let content = tlm.textContentManager().unwrap();
            let Some(viewport) = tlm.textViewportLayoutController().viewportRange() else {
                eprintln!("BENCH sync check: no viewport");
                return;
            };
            use objc2_app_kit::NSTextElementProvider;
            let first =
                content.offsetFromLocation_toLocation(&content.documentRange().location(), &viewport.location());
            let text = source(&e);
            let byte = crate::markdown::text::byte_at_u16(&text, 0, first.max(0) as usize);
            let st = e.ivars().state.borrow();
            let source_block = st
                .layout
                .blocks
                .partition_point(|b| b.src.start <= byte)
                .saturating_sub(1);
            drop(st);
            let preview_block = e.visible_preview_block();
            eprintln!(
                "BENCH sync check at {:.0} %: source shows block {source_block}, preview block {preview_block}",
                fraction * 100.0
            );
        });
    }
    for (n, fraction) in [0.35, 0.65, 0.05, 0.95].into_iter().enumerate() {
        after(2.0 + n as f64 * 0.25, move || {
            let e = editor(MainThreadMarker::new().unwrap()).unwrap();
            let clip = e.preview().enclosingScrollView().unwrap().contentView();
            e.ivars().scrolling.set(true);
            timed(
                &format!("jump preview to {:.0} % without sync, with redraw", fraction * 100.0),
                || {
                    clip.scrollToPoint(objc2_foundation::NSPoint::new(
                        0.0,
                        e.preview().bounds().size.height * fraction,
                    ));
                    e.preview()
                        .enclosingScrollView()
                        .unwrap()
                        .reflectScrolledClipView(&clip);
                    e.window().unwrap().displayIfNeeded();
                },
            );
            e.ivars().scrolling.set(false);
        });
    }
    for (n, fraction) in [0.4, 0.7, 0.2].into_iter().enumerate() {
        after(5.5 + n as f64 * 0.6, move || {
            let e = editor(MainThreadMarker::new().unwrap()).unwrap();
            let clip = e.preview().enclosingScrollView().unwrap().contentView();
            let t = std::time::Instant::now();
            clip.scrollToPoint(objc2_foundation::NSPoint::new(
                0.0,
                e.preview().bounds().size.height * fraction,
            ));
            e.preview()
                .enclosingScrollView()
                .unwrap()
                .reflectScrolledClipView(&clip);
            eprintln!(
                "BENCH jump preview to {:.0} % + sync source: {:.1} ms",
                fraction * 100.0,
                t.elapsed().as_secs_f64() * 1000.0
            );
        });
        after(5.9 + n as f64 * 0.6, move || {
            let e = editor(MainThreadMarker::new().unwrap()).unwrap();
            let tlm = e.source().textLayoutManager().unwrap();
            let content = tlm.textContentManager().unwrap();
            use objc2_app_kit::NSTextElementProvider;
            let top = e.source().visibleRect().origin.y - e.source().textContainerOrigin().y;
            let Some(fragment) = tlm.textLayoutFragmentForPosition(objc2_foundation::NSPoint::new(0.0, top.max(0.0)))
            else {
                eprintln!("BENCH sync check: no fragment at the source top");
                return;
            };
            let first = content.offsetFromLocation_toLocation(
                &content.documentRange().location(),
                &fragment.rangeInElement().location(),
            );
            let text = source(&e);
            let byte = crate::markdown::text::byte_at_u16(&text, 0, first.max(0) as usize);
            let st = e.ivars().state.borrow();
            let source_block = st
                .layout
                .blocks
                .partition_point(|b| b.src.start <= byte)
                .saturating_sub(1);
            drop(st);
            let preview_block = e.visible_preview_block();
            eprintln!(
                "BENCH sync check preview {:.0} %: preview block {preview_block}, source shows block {source_block}",
                fraction * 100.0
            );
        });
    }
    after(8.0, move || {
        let e = editor(MainThreadMarker::new().unwrap()).unwrap();
        eprintln!("BENCH -- second keystroke after the images settled");
        e.source().scrollRangeToVisible(e.source().selectedRange());
        e.window().unwrap().displayIfNeeded();
        timed("keystroke in the source view alone", || type_in_source(&e, "z"));
        timed("reading the text from the source view", || {
            e.snapshot();
        });
        timed("handing the text to the background", || e.update_in_background());
    });
    after(9.0, move || {
        let e = editor(MainThreadMarker::new().unwrap()).unwrap();
        eprintln!("BENCH -- scrolling the preview down in screen steps");
        let clip = e.preview().enclosingScrollView().unwrap().contentView();
        let step = clip.bounds().size.height;
        for n in 0..40 {
            let y = clip.bounds().origin.y + step;
            clip.scrollToPoint(objc2_foundation::NSPoint::new(0.0, y));
            e.preview()
                .enclosingScrollView()
                .unwrap()
                .reflectScrolledClipView(&clip);
            let t = std::time::Instant::now();
            e.render_pending_blocks();
            if n % 10 == 9 {
                eprintln!(
                    "BENCH   scroll step {}: rendering what came into view {:.1} ms",
                    n + 1,
                    t.elapsed().as_secs_f64() * 1000.0
                );
            }
        }
    });
    after(11.0, move || {
        let e = editor(MainThreadMarker::new().unwrap()).unwrap();
        let clip = e.preview().enclosingScrollView().unwrap().contentView();
        let (mut worst, mut unformatted) = (0.0f64, 0usize);
        for k in 0..30 {
            let y = e.preview().bounds().size.height * (k as f64 * 0.033);
            let t = std::time::Instant::now();
            clip.scrollToPoint(objc2_foundation::NSPoint::new(0.0, y));
            e.preview()
                .enclosingScrollView()
                .unwrap()
                .reflectScrolledClipView(&clip);
            worst = worst.max(t.elapsed().as_secs_f64() * 1000.0);
            let st = e.ivars().state.borrow();
            let visible = e.preview().visibleRect();
            let block_at = |y: f64| {
                let offset = super::scroll::offset_at(e.preview(), y).unwrap_or(0);
                st.index
                    .block_starts
                    .partition_point(|&s| s <= offset)
                    .saturating_sub(1)
            };
            let (top, bottom) = (
                block_at(visible.origin.y),
                block_at(visible.origin.y + visible.size.height),
            );
            unformatted += st.blocks.placeholders[top..=bottom.min(st.blocks.placeholders.len() - 1)]
                .iter()
                .filter(|p| **p)
                .count();
        }
        eprintln!(
            "BENCH fast scrolling over the preview: slowest step {worst:.1} ms, unformatted blocks in view {unformatted}"
        );
    });
    after(14.0, move || {
        let e = editor(MainThreadMarker::new().unwrap()).unwrap();
        let pending = e
            .ivars()
            .state
            .borrow()
            .blocks
            .placeholders
            .iter()
            .filter(|p| **p)
            .count();
        eprintln!("BENCH still plain text after 14 s (only rendered near the visible part): {pending}");
        eprintln!(
            "BENCH source still on TextKit 2: {}",
            e.source().textLayoutManager().is_some()
        );
        if std::env::var_os("SPLAT_BENCH_TYPING").is_some() {
            eprintln!("BENCH TYPING START");
            keep_typing(30);
            return;
        }
        eprintln!("BENCH done");
        std::process::exit(0);
    });
}

/// Types one character per second, as fast as a 10 MB document finishes a background update.
fn keep_typing(left: usize) {
    if left == 0 {
        eprintln!("BENCH TYPING END");
        after(4.0, || std::process::exit(0));
        return;
    }
    after(1.0, move || {
        let e = editor(MainThreadMarker::new().unwrap()).unwrap();
        type_in_source(&e, "w");
        keep_typing(left - 1);
    });
}

fn type_in_source(e: &EditorController, text: &str) {
    unsafe {
        e.source()
            .insertText_replacementRange(&ns(text), NSRange::new(NSNotFound as usize, 0))
    };
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
        range: end..end,
        replacement: text.to_string(),
        selection: end..end,
    };
    e.apply_edit(edit, None, false);
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
    e.source().setSelectedRange(NSRange::new(map.to_u16(formula), 0));
    e.edit_atom_at_selection();
    e.ivars().atoms.edit.borrow().as_ref().map(|a| a.view.clone())
}

fn close_popover(e: &EditorController) {
    let popover = e.ivars().atoms.edit.borrow().as_ref().map(|a| a.popover.clone());
    if let Some(p) = popover {
        p.close();
    }
}

fn check_context_menu(e: &EditorController) {
    let menu: Option<Retained<objc2_app_kit::NSMenu>> =
        unsafe { objc2::msg_send![<objc2_app_kit::NSTextView as objc2::ClassType>::class(), defaultMenu] };
    let menu = menu.map(|m| m.copy()).unwrap();
    let p = storage(e.preview()).string().to_string();
    let formula = p[..p.find("Inline-Formel: ").unwrap()].encode_utf16().count() + "Inline-Formel: ".len();
    let adjusted = e.adjust_context_menu(&menu, e.preview(), formula);
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
