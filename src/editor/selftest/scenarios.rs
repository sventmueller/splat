//! Checks that run after the main click-through, on a section of their own appended to the document: every
//! formatting and table command, the dialogs, link clicks, pasting and dropping, exports, a large change, the
//! toolbar groups, the recovery after a crash and the quick reference.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSApplication, NSDocumentController, NSEventModifierFlags,
    NSMenu, NSModalResponse, NSModalResponseCancel, NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeString,
    NSPasteboardTypeTIFF, NSSavePanel, NSSegmentedControl, NSTextFinderAction, NSTextFinderBarContainer,
    NSUserInterfaceItemIdentification, NSWindow,
};
use objc2_foundation::{NSArray, NSData, NSRange, NSRunLoop, NSRunLoopCommonModes, NSTimer, NSURL, NSUserDefaults};

use super::super::actions::Action;
use super::{
    Ctx, Step, append, check, click_preview, editable_fields, perform, preview_link, select_in_source, settle, source,
    step, type_in_preview, type_in_source, when,
};
use crate::document::MdDocument;
use crate::util::{constant, ns, storage};

const SECTION: &str = "\n\n## Prüfstand\n\nWort **fett** eins zwei drei.\n\n\
    Links: [Netz](https://example.com/pruefstand) #pruefstand [Andere](andere.md) [Fehlt](fehlt.md) \
    [Einstellungen](x-apple.systempreferences:com.apple.Settings) mit Fußnote[^pruef].\n\n\
    ![png](bild.png) ![jpg](foto.jpg) ![svg](zeichnung.svg) ![pdf](seite.pdf) ![tif](scan.tiff)\n\n\
    | A | B |\n|---|---|\n| 1 | 2 |\n\n[^pruef]: Die Prüffußnote.\n";

const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"10\"><rect width=\"20\" \
                   height=\"10\" fill=\"#3a7\"/></svg>";

thread_local! {
    /// The source before the step under test, for the step that checks or undoes it.
    static BEFORE: RefCell<String> = const { RefCell::new(String::new()) };
    /// Whether an alert ran modally while `offer_recovered` was called.
    static MODAL_SEEN: Cell<bool> = const { Cell::new(false) };
}

fn remember(c: &Ctx) {
    BEFORE.with(|b| *b.borrow_mut() = source(&c.e));
}

fn before() -> String {
    BEFORE.with(|b| b.borrow().clone())
}

fn sizes(c: &Ctx) -> String {
    format!("{} bytes, before {}", source(&c.e).len(), before().len())
}

fn undo(c: &Ctx) {
    if let Some(u) = c.e.document().and_then(|d| d.undoManager()) {
        u.undo();
    }
}

/// A step that undoes the last one and checks the text is as before it.
fn undone(name: String) -> Step {
    step(0.1, move |c| {
        undo(c);
        check(&name, source(&c.e) == before(), &sizes(c));
    })
}

fn doc_dir(c: &Ctx) -> PathBuf {
    c.e.doc_dir().unwrap_or_else(|| c.out.clone())
}

fn sheet(c: &Ctx) -> Option<Retained<NSWindow>> {
    c.e.window().and_then(|w| w.attachedSheet())
}

/// Ends the sheet on the editor's window as if its button `response` was clicked.
fn end_sheet(c: &Ctx, response: NSModalResponse) {
    if let (Some(window), Some(sheet)) = (c.e.window(), sheet(c)) {
        window.endSheet_returnCode(&sheet, response);
    }
}

/// Runs `action` with `tag` on the text `needle` of the test section, checks that it changed the text, and undoes it.
fn command(name: &'static str, needle: &'static str, action: Action, tag: isize) -> [Step; 2] {
    [
        step(0.1, move |c| {
            select_in_source(&c.e, needle);
            remember(c);
            perform(&c.e, action.spec().selector, tag);
            check(&format!("{name} changes the text"), source(&c.e) != before(), "");
        }),
        undone(format!("{name} is undone in one step")),
    ]
}

/// Writes the files the test section links to, in every image format the exports handle differently.
fn write_files(dir: &Path) {
    let mut png = Vec::new();
    let _ = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 30, 30, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    let mut jpeg = Vec::new();
    let _ = image::RgbImage::from_pixel(2, 2, image::Rgb([30, 30, 200]))
        .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg);
    let tiff = objc2_app_kit::NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(&png))
        .and_then(|rep| rep.TIFFRepresentation())
        .map(|d| d.to_vec())
        .unwrap_or_default();
    let pdf = super::super::decoder::svg_to_pdf(SVG).unwrap_or_default();
    for (name, bytes) in [
        ("bild.png", png.as_slice()),
        ("foto.jpg", &jpeg),
        ("zeichnung.svg", SVG.as_bytes()),
        ("seite.pdf", &pdf),
        ("scan.tiff", &tiff),
        ("andere.md", b"# Andere\n"),
    ] {
        let _ = std::fs::write(dir.join(name), bytes);
    }
}

/// The menu items of the main menu whose action is `selector`.
fn menu_items(mtm: MainThreadMarker, selector: Sel) -> Vec<Retained<objc2_app_kit::NSMenuItem>> {
    fn walk(menu: &NSMenu, selector: Sel, out: &mut Vec<Retained<objc2_app_kit::NSMenuItem>>) {
        for item in menu.itemArray().iter() {
            if item.action() == Some(selector) {
                out.push(item.clone());
            }
            if let Some(sub) = item.submenu() {
                walk(&sub, selector, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(menu) = NSApplication::sharedApplication(mtm).mainMenu() {
        walk(&menu, selector, &mut out);
    }
    out
}

fn documents(mtm: MainThreadMarker) -> Vec<Retained<MdDocument>> {
    NSDocumentController::sharedDocumentController(mtm)
        .documents()
        .iter()
        .filter_map(|d| d.downcast::<MdDocument>().ok())
        .collect()
}

/// Clicks the preview link to `target`, scrolled into view first; whether there was one.
fn click_link(c: &Ctx, target: &str, flags: NSEventModifierFlags) -> bool {
    let Some(at) = preview_link(&c.e, target) else {
        return false;
    };
    c.e.preview().scrollRangeToVisible(NSRange::new(at, 1));
    click_preview(&c.e, at, flags);
    true
}

/// A plain click on the preview link to `target`, handed to the controller as AppKit hands it over: on a window in the
/// background, a real plain click would only bring it to the front.
fn plain_click(c: &Ctx, target: &str) -> Option<usize> {
    let at = preview_link(&c.e, target)?;
    c.e.preview().scrollRangeToVisible(NSRange::new(at, 1));
    let link: &AnyObject = &ns(target);
    c.e.link_clicked(c.e.preview(), link, at);
    Some(at)
}

fn cmd_click(c: &Ctx, target: &str) -> bool {
    click_link(c, target, NSEventModifierFlags::Command)
}

/// The save panel of an export, which proposes `name`; it runs in a process of its own and is cancelled here.
fn proposes(c: &Ctx, name: &str) -> bool {
    let proposed = sheet(c)
        .and_then(|s| s.downcast::<NSSavePanel>().ok())
        .map(|p| p.nameFieldStringValue().to_string());
    end_sheet(c, NSModalResponseCancel);
    proposed.is_some_and(|p| p == name)
}

fn export_steps() -> Vec<Step> {
    use super::super::export::Format;
    vec![
        step(0.1, |c| perform(&c.e, Action::ExportHtml.spec().selector, 0)),
        step(0.8, |c| {
            check(
                "HTML export proposes the document's name",
                proposes(c, "QuickReference.html"),
                "",
            );
            let page = doc_dir(c).join("Export.html");
            let written = c.e.write_export(Format::Html, "Export", &page);
            let page = std::fs::read_to_string(page).unwrap_or_default();
            check(
                "HTML export embeds PNG, JPEG, SVG, PDF and TIFF images",
                written.is_ok()
                    && page.contains("data:image/jpeg;base64,")
                    && page.contains("data:image/svg+xml;base64,")
                    && page.matches("data:image/png;base64,").count() >= 3,
                &format!("{written:?}, {} bytes", page.len()),
            );
            perform(&c.e, Action::ExportWord.spec().selector, 0);
        }),
        step(0.8, |c| {
            check(
                "Word export proposes the document's name",
                proposes(c, "QuickReference.docx"),
                "",
            );
            let path = doc_dir(c).join("Export.docx");
            let written = c.e.write_export(Format::Word, "Export", &path);
            let docx = std::fs::read(path).unwrap_or_default();
            check(
                "Word export carries the images",
                written.is_ok() && String::from_utf8_lossy(&docx).matches("word/media/").count() >= 5,
                &format!("{written:?}, {} bytes", docx.len()),
            );
            let failed =
                c.e.write_export(Format::Html, "Export", Path::new("/System/Export.html"));
            if let Err(reason) = &failed {
                c.e.report_export_failure(reason);
            }
            check("an export to a place without write access fails", failed.is_err(), "");
        }),
        step(0.8, |c| {
            check("a failed export says so", sheet(c).is_some(), "no alert");
            end_sheet(c, NSAlertFirstButtonReturn);
        }),
    ]
}

fn dialog_steps() -> Vec<Step> {
    vec![
        step(0.1, |c| {
            select_in_source(&c.e, "drei.");
            remember(c);
            perform(&c.e, Action::Table.spec().selector, 0);
        }),
        step(0.6, |c| {
            check("the table dialog opens", sheet(c).is_some(), "no sheet");
            end_sheet(c, NSAlertFirstButtonReturn);
        }),
        step(0.3, |c| {
            let rows = |s: &str| s.lines().filter(|l| l.starts_with('|')).count();
            check(
                "the table dialog inserts a table of three by three",
                rows(&source(&c.e)) == rows(&before()) + 4,
                &sizes(c),
            );
        }),
        undone("inserting a table is undone in one step".into()),
        step(0.1, |c| {
            select_in_source(&c.e, "zwei");
            remember(c);
            perform(&c.e, Action::Link.spec().selector, 0);
        }),
        step(0.6, |c| {
            let mut fields = Vec::new();
            if let Some(view) = sheet(c).and_then(|s| s.contentView()) {
                editable_fields(&view, &mut fields);
            }
            check("the link dialog asks for address and text", fields.len() == 2, "");
            if let Some(address) = fields.first() {
                address.setStringValue(&ns("https://splat.example"));
            }
            end_sheet(c, NSAlertFirstButtonReturn);
        }),
        step(0.3, |c| {
            check(
                "the link dialog links the selection",
                source(&c.e).contains("[zwei](https://splat.example)"),
                &sizes(c),
            );
        }),
        undone("inserting a link is undone in one step".into()),
        step(0.1, |c| {
            remember(c);
            perform(&c.e, Action::Image.spec().selector, 0);
        }),
        step(1.0, |c| {
            check("the image dialog opens", sheet(c).is_some(), "no open panel");
            end_sheet(c, NSModalResponseCancel);
        }),
        step(0.5, |c| {
            check(
                "a cancelled image dialog leaves the text",
                source(&c.e) == before(),
                &sizes(c),
            );
        }),
    ]
}

fn link_steps() -> Vec<Step> {
    vec![
        step(0.1, |c| {
            let clicked = plain_click(c, "splat-footnote:pruef");
            check("the footnote reference is a link", clicked.is_some(), "");
        }),
        step(0.4, |c| {
            let preview = c.e.preview();
            let text = storage(preview).string().to_string();
            let visible = text.find("Die Prüffußnote").is_some_and(|at| {
                let rect = super::super::scroll::char_rect(preview, text[..at].encode_utf16().count());
                let shown = preview.visibleRect();
                rect.origin.y >= shown.origin.y && rect.origin.y <= shown.origin.y + shown.size.height
            });
            check("a click on a footnote reference shows the footnote", visible, "");
            plain_click(c, "https://example.com/pruefstand");
        }),
        step(0.3, |c| {
            let at = preview_link(&c.e, "https://example.com/pruefstand");
            let caret = c.e.preview().selectedRange().location;
            check(
                "a plain click on a web link only places the caret",
                at == Some(caret),
                &format!("{at:?} vs {caret}"),
            );
            check("tags are links", cmd_click(c, "splat-tag:#pruefstand"), "");
        }),
        step(0.4, |c| {
            // SAFETY: Reading AppKit's constant for the shared find pasteboard.
            let board = NSPasteboard::pasteboardWithName(unsafe { objc2_app_kit::NSPasteboardNameFind });
            let searched = board
                .stringForType(constant!(NSPasteboardTypeString))
                .map(|s| s.to_string());
            let bar = c.e.source().enclosingScrollView().is_some_and(|s| s.isFindBarVisible());
            check(
                "⌘-click on a tag searches for it in the source",
                bar && searched.as_deref() == Some("#pruefstand"),
                &format!("{searched:?}, find bar {bar}"),
            );
            let item = objc2_app_kit::NSMenuItem::new(c.e.mtm());
            item.setTag(NSTextFinderAction::HideFindInterface.0);
            // SAFETY: The sender is a menu item whose tag names the action, as the method expects.
            unsafe { c.e.source().performTextFinderAction(Some(&item)) };
            let count = documents(c.e.mtm()).len();
            let opened = cmd_click(c, "andere.md");
            check("links to other documents are links", opened && count > 0, "");
        }),
        when(
            |c| {
                documents(c.e.mtm())
                    .iter()
                    .any(|d| d.displayName().to_string().starts_with("andere"))
            },
            |c| {
                let other = documents(c.e.mtm())
                    .into_iter()
                    .find(|d| d.displayName().to_string().starts_with("andere"));
                check(
                    "⌘-click on a link to a Markdown file opens it in Splat",
                    other.as_ref().is_some_and(|d| d.text() == "# Andere\n"),
                    "",
                );
                if let Some(d) = other {
                    d.close();
                }
            },
        ),
        step(0.5, |c| {
            if let Some(window) = c.e.window() {
                window.makeKeyAndOrderFront(None);
            }
            let count = documents(c.e.mtm()).len();
            cmd_click(c, "fehlt.md");
            check(
                "⌘-click on a missing file opens nothing",
                documents(c.e.mtm()).len() == count && sheet(c).is_none(),
                "",
            );
            check(
                "links to other apps are links",
                cmd_click(c, "x-apple.systempreferences:com.apple.Settings"),
                "",
            );
        }),
        step(0.6, |c| {
            check("a link to another app asks first", sheet(c).is_some(), "no alert");
            end_sheet(c, NSAlertFirstButtonReturn);
        }),
    ]
}

fn picture(c: &Ctx) -> Vec<u8> {
    std::fs::read(doc_dir(c).join("bild.png")).unwrap_or_default()
}

fn paste_steps() -> Vec<Step> {
    vec![
        step(0.1, |c| {
            select_in_source(&c.e, "drei.");
            remember(c);
            let board = NSPasteboard::pasteboardWithUniqueName();
            board.clearContents();
            board.setData_forType(Some(&NSData::with_bytes(&picture(c))), constant!(NSPasteboardTypePNG));
            let pasted = c.e.paste_image(&board);
            let pictures = std::fs::read_dir(doc_dir(c))
                .into_iter()
                .flatten()
                .flatten()
                .filter(|f| f.file_name().to_string_lossy().starts_with("Bild "))
                .count();
            check(
                "a pasted picture becomes a file beside the document and an image link",
                pasted && pictures == 1 && source(&c.e).contains("![Bild "),
                &format!("pasted {pasted}, {pictures} files"),
            );
        }),
        undone("pasting a picture is undone in one step".into()),
        step(0.1, |c| {
            let board = NSPasteboard::pasteboardWithUniqueName();
            board.clearContents();
            let tiff = std::fs::read(doc_dir(c).join("scan.tiff")).unwrap_or_default();
            board.setData_forType(Some(&NSData::with_bytes(&tiff)), constant!(NSPasteboardTypeTIFF));
            remember(c);
            let pasted = c.e.paste_image(&board);
            check(
                "a pasted TIFF picture is stored as PNG",
                pasted && source(&c.e).matches("![Bild ").count() == 1,
                &format!("pasted {pasted}"),
            );
        }),
        undone("pasting a TIFF picture is undone in one step".into()),
        step(0.1, |c| {
            let board = NSPasteboard::pasteboardWithUniqueName();
            board.clearContents();
            board.setString_forType(&ns("Text"), constant!(NSPasteboardTypeString));
            board.setData_forType(Some(&NSData::with_bytes(b"x")), constant!(NSPasteboardTypePNG));
            check(
                "text on the pasteboard wins over its picture",
                !c.e.paste_image(&board),
                "",
            );
            let untitled = NSDocumentController::sharedDocumentController(c.e.mtm())
                .openUntitledDocumentAndDisplay_error(true)
                .ok()
                .and_then(|d| d.downcast::<MdDocument>().ok());
            let board = NSPasteboard::pasteboardWithUniqueName();
            board.clearContents();
            board.setData_forType(Some(&NSData::with_bytes(&picture(c))), constant!(NSPasteboardTypePNG));
            let asked = untitled
                .as_ref()
                .and_then(|d| d.editor())
                .map(|e| e.paste_image(&board));
            check(
                "a picture pasted into an unsaved document is taken",
                asked == Some(true),
                "",
            );
        }),
        step(0.6, |c| {
            let untitled = documents(c.e.mtm()).into_iter().find(|d| d.fileURL().is_none());
            let window = untitled.as_ref().and_then(|d| d.editor()).and_then(|e| e.window());
            let alert = window.as_ref().and_then(|w| w.attachedSheet());
            check(
                "an unsaved document asks to be saved before a picture is pasted",
                alert.is_some(),
                "",
            );
            if let (Some(w), Some(a)) = (&window, &alert) {
                w.endSheet_returnCode(a, NSAlertSecondButtonReturn);
            }
            if let Some(d) = untitled {
                d.close();
            }
        }),
        step(0.5, |c| {
            if let Some(window) = c.e.window() {
                window.makeKeyAndOrderFront(None);
            }
            select_in_source(&c.e, "drei.");
            remember(c);
            let dir = doc_dir(c);
            let urls: Vec<Retained<ProtocolObject<dyn objc2_app_kit::NSPasteboardWriting>>> = [
                NSURL::fileURLWithPath(&ns(dir.join("bild.png").to_str().unwrap_or_default())),
                NSURL::fileURLWithPath(&ns(dir.join("andere.md").to_str().unwrap_or_default())),
            ]
            .into_iter()
            .chain(NSURL::URLWithString(&ns("https://example.org/seite")))
            .map(ProtocolObject::from_retained)
            .collect();
            let board = NSPasteboard::pasteboardWithUniqueName();
            board.clearContents();
            board.writeObjects(&NSArray::from_retained_slice(&urls));
            let at = c.e.source().selectedRange().location;
            let dropped = c.e.handle_source_drop(&board, at);
            let s = source(&c.e);
            check(
                "dropped files and addresses become images and links",
                dropped
                    && s.contains("(bild.png)")
                    && s.contains("(andere.md)")
                    && s.contains("https://example.org/seite"),
                &format!("dropped {dropped}"),
            );
        }),
        undone("a drop is undone in one step".into()),
    ]
}

fn table_steps() -> Vec<Step> {
    let mut steps = Vec::new();
    for (name, action, tag) in [
        ("row above", Action::RowAbove, 0),
        ("row below", Action::RowBelow, 0),
        ("column left", Action::ColumnLeft, 0),
        ("column right", Action::ColumnRight, 0),
        ("deleting a row", Action::DeleteRow, 0),
        ("deleting a column", Action::DeleteColumn, 0),
        ("left alignment", Action::TableAlign, 1),
        ("centred alignment", Action::TableAlign, 2),
        ("right alignment", Action::TableAlign, 3),
        ("aligning the table", Action::TableFormat, 0),
    ] {
        steps.extend(command(name, "1 | 2", action, tag));
    }
    for (name, backwards) in [("tab", false), ("backtab", true)] {
        steps.push(step(0.1, move |c| {
            select_in_source(&c.e, "1 | 2");
            c.e.source()
                .setSelectedRange(NSRange::new(c.e.source().selectedRange().location, 0));
            remember(c);
            let command = if backwards {
                sel!(insertBacktab:)
            } else {
                sel!(insertTab:)
            };
            check(
                &format!("{name} in a table moves between its cells"),
                c.e.source_command(command),
                "",
            );
        }));
        steps.push(step(0.1, |c| {
            if source(&c.e) != before() {
                undo(c);
            }
        }));
    }
    steps
}

fn view_steps() -> Vec<Step> {
    vec![
        step(0.1, |c| {
            select_in_source(&c.e, "fett");
            // Validated by hand: AppKit asks the controller only while the app is in front.
            let validate = |selector: Sel| {
                let items = menu_items(c.e.mtm(), selector);
                for item in &items {
                    c.e.validate(item.action(), item.tag(), Some(item));
                }
                items
            };
            let bold = validate(Action::Bold.spec().selector);
            check(
                "the menu checks bold where the caret is in bold text",
                !bold.is_empty() && bold.iter().all(|i| i.state() == objc2_app_kit::NSControlStateValueOn),
                &format!("{} items", bold.len()),
            );
            let preview = validate(Action::TogglePreview.spec().selector);
            check(
                "the menu offers to hide the shown preview",
                preview.iter().any(|i| i.title().to_string() == "Vorschau ausblenden"),
                "",
            );
            perform(&c.e, Action::ToggleOutline.spec().selector, 0);
        }),
        step(0.4, |c| {
            check("the outline opens", c.e.outline_visible(), "");
            perform(&c.e, Action::ToggleOutline.spec().selector, 0);
        }),
        step(0.4, |c| {
            check("the outline closes", !c.e.outline_visible(), "");
            let on = super::super::env::typography_on();
            perform(&c.e, Action::Typography.spec().selector, 0);
            check(
                "typographic punctuation switches",
                super::super::env::typography_on() != on,
                "",
            );
            perform(&c.e, Action::Typography.spec().selector, 0);
            perform(&c.e, Action::ZoomIn.spec().selector, 0);
            let zoom = c.e.ivars().state.borrow().zoom;
            check("zooming in enlarges", zoom > 1.0, &format!("{zoom}"));
            perform(&c.e, Action::ZoomOut.spec().selector, 0);
            perform(&c.e, Action::ZoomOut.spec().selector, 0);
            let zoom = c.e.ivars().state.borrow().zoom;
            check("zooming out shrinks", zoom < 1.0, &format!("{zoom}"));
            perform(&c.e, Action::ZoomReset.spec().selector, 0);
            let zoom = c.e.ivars().state.borrow().zoom;
            check(
                "the actual size comes back",
                (zoom - 1.0).abs() < 1e-9,
                &format!("{zoom}"),
            );
        }),
        settle(|c| {
            select_in_source(&c.e, "drei.");
            remember(c);
            // Typing in the source schedules a parse; typing in the preview before it lands cannot be mapped.
            type_in_source(&c.e, "Q");
            c.e.window().map(|w| w.makeFirstResponder(Some(c.e.preview())));
            type_in_preview(&c.e, "Zzz");
        }),
        settle(|c| {
            let preview = storage(c.e.preview()).string().to_string();
            check(
                "typing in the preview while the source is parsed shows the source again",
                source(&c.e).contains("zwei Q") && !preview.contains("Zzz") && !source(&c.e).contains("Zzz"),
                "",
            );
        }),
        step(0.1, |c| {
            undo(c);
            check("the typing is undone", source(&c.e) == before(), &sizes(c));
        }),
    ]
}

fn toolbar_steps() -> Vec<Step> {
    vec![
        step(0.1, |c| {
            let mtm = c.e.mtm();
            let share = ProtocolObject::from_ref(&*c.e);
            let allowed = super::super::toolbar::allowed_identifiers();
            let missing: Vec<String> = allowed
                .iter()
                .map(|id| id.to_string())
                .filter(|id| !id.starts_with("NSToolbar"))
                .filter(|id| super::super::toolbar::make(mtm, id, share, &c.e).is_none())
                .collect();
            check(
                "every item the toolbar offers can be made",
                missing.is_empty(),
                &format!("{missing:?}"),
            );
            select_in_source(&c.e, "eins");
            remember(c);
            let bold = Action::Bold.spec().selector;
            let control = c.e.window().and_then(|w| w.toolbar()).and_then(|toolbar| {
                toolbar.items().iter().find_map(|item| {
                    let control = item.view()?.downcast::<NSSegmentedControl>().ok()?;
                    let group = NSUserInterfaceItemIdentification::identifier(&*control)?.to_string();
                    let segment = (0..control.segmentCount()).find(|&s| {
                        super::super::toolbar::segment_item(mtm, &group, s as usize, share, &c.e)
                            .is_some_and(|i| i.action() == Some(bold))
                    })?;
                    Some((control, segment))
                })
            });
            check("the bold button sits in a toolbar group", control.is_some(), "");
            // Its segments are momentary and forget a selection made outside a click.
            if let Some((control, segment)) = control {
                let mode = control.trackingMode();
                control.setTrackingMode(objc2_app_kit::NSSegmentSwitchTracking::SelectOne);
                control.setSelectedSegment(segment);
                c.e.toolbar_segment(Some(&control));
                control.setSelectedSegment(-1);
                control.setTrackingMode(mode);
            }
            check(
                "a toolbar group runs the command of its segment",
                source(&c.e).contains("**eins**"),
                "",
            );
        }),
        undone("a toolbar command is undone in one step".into()),
    ]
}

fn large_change_steps() -> Vec<Step> {
    vec![
        step(0.1, |c| {
            remember(c);
            let text: String = (1..=1000)
                .map(|n| format!("\n\nAbsatz {n} des großen Nachtrags."))
                .collect();
            append(&c.e, &text);
        }),
        settle(|c| {
            let pending = c.e.ivars().state.borrow().blocks.pending();
            check("a large change renders the part around the view first", pending, "");
            let end = storage(c.e.preview()).length();
            c.e.preview().scrollRangeToVisible(NSRange::new(end, 0));
        }),
        step(0.8, |c| {
            let last = c.e.ivars().state.borrow().blocks.placeholders.last().copied();
            check(
                "blocks scrolled into view are rendered",
                last == Some(false),
                &format!("{last:?}"),
            );
        }),
        undone("the large change is undone in one step".into()),
    ]
}

fn app_steps() -> Vec<Step> {
    vec![
        step(0.1, |c| {
            let files = super::super::emergency_save();
            let copy = files.iter().find_map(|f| {
                let text = std::fs::read_to_string(f).ok()?;
                (text == source(&c.e)).then_some(())
            });
            check(
                "a crash leaves a copy of every open document",
                copy.is_some(),
                &format!("{files:?}"),
            );
            let mtm = c.e.mtm();
            MODAL_SEEN.with(|m| m.set(false));
            let stop = RcBlock::new(move |_: std::ptr::NonNull<NSTimer>| {
                let app = NSApplication::sharedApplication(mtm);
                MODAL_SEEN.with(|m| m.set(app.modalWindow().is_some()));
                app.stopModalWithCode(NSAlertSecondButtonReturn);
            });
            let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(0.5, false, &stop) };
            unsafe { NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
            super::super::offer_recovered(mtm);
            let stamp = NSUserDefaults::standardUserDefaults().doubleForKey(&ns("RecoveredTextsShownUntil"));
            check(
                "the next start says once where the copies are",
                MODAL_SEEN.with(Cell::get) && stamp > 0.0,
                &format!("stamp {stamp}"),
            );
            let count = documents(mtm).len();
            let _ = unsafe {
                NSApplication::sharedApplication(mtm).sendAction_to_from(sel!(mdQuickReference:), None, None)
            };
            check(
                "the quick reference opens as a document",
                documents(mtm).len() == count + 1,
                "",
            );
        }),
        step(0.5, |c| {
            let reference = documents(c.e.mtm())
                .into_iter()
                .find(|d| d.displayName().to_string() == "Markdown-Kurzreferenz");
            check(
                "the quick reference is in the interface language",
                reference
                    .as_ref()
                    .is_some_and(|d| d.text().starts_with("# Markdown-Kurzreferenz")),
                "",
            );
            if let Some(d) = reference {
                d.close();
            }
            if let Some(window) = c.e.window() {
                window.makeKeyAndOrderFront(None);
            }
        }),
    ]
}

pub(super) fn steps() -> Vec<Step> {
    let mut steps = vec![
        step(0.1, |c| {
            write_files(&doc_dir(c));
            if let Some(window) = c.e.window() {
                window.makeKeyAndOrderFront(None);
            }
            append(&c.e, SECTION);
        }),
        settle(|_| {}),
    ];
    for (name, needle, action, tag) in [
        ("bold", "eins", Action::Bold, 0),
        ("italic", "eins", Action::Italic, 0),
        ("underline", "eins", Action::Underline, 0),
        ("strikethrough", "eins", Action::Strikethrough, 0),
        ("highlight", "eins", Action::Highlight, 0),
        ("inline code", "eins", Action::InlineCode, 0),
        ("superscript", "eins", Action::Superscript, 0),
        ("subscript", "eins", Action::Subscript, 0),
        ("a heading", "eins", Action::Heading, 2),
        ("a bulleted list", "eins", Action::BulletList, 0),
        ("a numbered list", "eins", Action::NumberedList, 0),
        ("a task list", "eins", Action::TaskList, 0),
        ("a quote", "eins", Action::Quote, 0),
        ("a code block", "eins", Action::CodeBlock, 1),
        ("an alert box", "eins", Action::Alert, 1),
        ("removing formatting", "**fett**", Action::RemoveFormatting, 0),
        ("a flowchart", "drei.", Action::Diagram, 0),
        ("a sequence diagram", "drei.", Action::Diagram, 1),
        ("a Gantt chart", "drei.", Action::Diagram, 2),
        ("a table of contents", "drei.", Action::Toc, 0),
        ("a footnote", "drei.", Action::Footnote, 0),
        ("a rule", "drei.", Action::Rule, 0),
    ] {
        steps.extend(command(name, needle, action, tag));
    }
    steps.extend(table_steps());
    steps.extend(view_steps());
    steps.extend(dialog_steps());
    steps.extend(link_steps());
    steps.extend(paste_steps());
    steps.extend(export_steps());
    steps.extend(toolbar_steps());
    steps.extend(large_change_steps());
    steps.extend(app_steps());
    steps
}
