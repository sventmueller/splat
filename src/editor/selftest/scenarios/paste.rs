//! Pasting and dropping text and pictures.

use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSAlertSecondButtonReturn, NSDocumentController, NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeString,
    NSPasteboardTypeTIFF, NSWindow,
};
use objc2_foundation::{NSArray, NSData, NSURL};

use super::super::{Ctx, Step, applied, at_least, check, select_in_source, source, when};
use super::{doc_dir, documents, picture, remember, sizes, undone};
use crate::document::MdDocument;
use crate::util::{constant, ns};

/// The alert on the window of the unsaved document.
fn untitled_alert(c: &Ctx) -> Option<Retained<NSWindow>> {
    documents(c.e.mtm())
        .into_iter()
        .find(|d| d.fileURL().is_none())
        .and_then(|d| d.editor())
        .and_then(|e| e.window())
        .and_then(|w| w.attachedSheet())
}

#[expect(clippy::too_many_lines, reason = "a list of steps, each of which stands for itself")]
pub(super) fn paste_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            select_in_source(&c.e, "drei.");
            remember(c);
            let board = NSPasteboard::pasteboardWithUniqueName();
            board.clearContents();
            board.setData_forType(Some(&NSData::with_bytes(&picture(c))), constant!(NSPasteboardTypePNG));
            check(
                "a pasted picture is taken without waiting for it",
                c.e.paste_image(&board),
                "",
            );
        }),
        when(
            "the pasted picture to arrive",
            |c| source(&c.e).contains("![Bild "),
            |c| {
                let pictures = std::fs::read_dir(doc_dir(c))
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|f| f.file_name().to_string_lossy().starts_with("Bild "))
                    .count();
                check(
                    "a pasted picture becomes a file beside the document and an image link",
                    pictures == 1,
                    &format!("{pictures} files"),
                );
            },
        ),
        undone("pasting a picture is undone in one step".into()),
        applied(|c| {
            let board = NSPasteboard::pasteboardWithUniqueName();
            board.clearContents();
            let tiff = std::fs::read(doc_dir(c).join("scan.tiff")).unwrap_or_default();
            board.setData_forType(Some(&NSData::with_bytes(&tiff)), constant!(NSPasteboardTypeTIFF));
            remember(c);
            check(
                "a pasted TIFF picture is taken without waiting for it",
                c.e.paste_image(&board),
                "",
            );
        }),
        when(
            "the pasted TIFF picture to arrive",
            |c| source(&c.e).matches("![Bild ").count() == 1,
            |c| {
                let (address, stored) = picture_in_text(c);
                check(
                    "a pasted TIFF picture is stored as PNG",
                    stored
                        .as_deref()
                        .is_some_and(|bytes| bytes.starts_with(b"\x89PNG\r\n\x1a\n")),
                    &format!(
                        "{address}: {} bytes, {}",
                        stored.map_or(0, |bytes| bytes.len()),
                        sizes(c)
                    ),
                );
            },
        ),
        undone("pasting a TIFF picture is undone in one step".into()),
        applied(|c| {
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
        when(
            "the unsaved document to ask for a save",
            |c| untitled_alert(c).is_some(),
            |c| {
                let untitled = documents(c.e.mtm()).into_iter().find(|d| d.fileURL().is_none());
                let window = untitled.as_ref().and_then(|d| d.editor()).and_then(|e| e.window());
                let alert = untitled_alert(c);
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
            },
        ),
        at_least(
            0.5,
            "the unsaved document to close",
            |c| !documents(c.e.mtm()).iter().any(|d| d.fileURL().is_none()),
            |c| {
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
                let before = source(&c.e);
                let dropped = c.e.handle_source_drop(&board, at);
                let s = source(&c.e);
                let more = |mark: &str| s.matches(mark).count() > before.matches(mark).count();
                check(
                    "dropped files and addresses become images and links",
                    dropped && more("(bild.png)") && more("(andere.md)") && more("https://example.org/seite"),
                    &format!("dropped {dropped}"),
                );
            },
        ),
        undone("a drop is undone in one step".into()),
    ]
}

/// The address of the picture the text refers to first, and its bytes.
fn picture_in_text(c: &Ctx) -> (String, Option<Vec<u8>>) {
    let text = source(&c.e);
    let address = text.find("![Bild ").and_then(|start| {
        let open = start + text[start..].find("](")? + 2;
        let close = open + text[open..].find(')')?;
        Some(text[open..close].trim_matches(['<', '>']).to_string())
    });
    let Some(address) = address else {
        return (String::new(), None);
    };
    let bytes =
        crate::editor::images::resolve_path(&address, c.e.doc_dir().as_deref()).and_then(|p| std::fs::read(p).ok());
    (address, bytes)
}
