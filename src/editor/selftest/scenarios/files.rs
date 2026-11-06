//! Image files, the memory their pictures take, and a text in an old encoding.

use block2::RcBlock;
use objc2::MainThreadOnly;
use objc2_app_kit::NSDocumentController;
use objc2_foundation::NSURL;

use super::super::{Ctx, Step, append, applied, check, settle, when};
use super::{doc_dir, documents, remember, undone};
use crate::util::ns;

/// A PNG of `side` × `side` pixels.
fn png_of_side(side: u32) -> Vec<u8> {
    let mut png = Vec::new();
    let _ = image::RgbaImage::from_pixel(side, side, image::Rgba([30, 160, 60, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    png
}

/// The width of the image in `file`, once the preview has it.
fn shown_width(c: &Ctx, file: &str) -> Option<f64> {
    let path = doc_dir(c).join(file);
    c.e.pictures().store().borrow().shown_size(&path).map(|s| s.width)
}

pub(super) fn image_file_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            let _ = std::fs::write(doc_dir(c).join("bild.png"), png_of_side(7));
        }),
        when(
            "the replaced image to show",
            |c| shown_width(c, "bild.png") == Some(7.0),
            |c| {
                let width = shown_width(c, "bild.png");
                check(
                    "an image replaced on disk shows without reopening",
                    width == Some(7.0),
                    &format!("{width:?}"),
                );
                // Paper does not wait for a load under way.
                let _ = std::fs::write(doc_dir(c).join("druck.png"), png_of_side(3));
                let dir = doc_dir(c);
                let mut images = c.e.pictures().store().borrow_mut();
                let loading = images.file("druck.png", Some(&dir), false, Some(&c.e));
                let printed = images.file("druck.png", Some(&dir), false, None);
                check(
                    "printing does not leave out an image that is still loading",
                    loading.is_none() && printed.is_some(),
                    &format!("preview {}, paper {}", loading.is_some(), printed.is_some()),
                );
            },
        ),
        applied(|c| {
            remember(c);
            append(&c.e, "\n\n![spät](spaeter.png)\n");
        }),
        settle(|c| {
            check(
                "a missing image counts as failed",
                c.e.pictures().store().borrow().shown(&doc_dir(c).join("spaeter.png")) == Some(false),
                "",
            );
            let _ = std::fs::write(doc_dir(c).join("spaeter.png"), png_of_side(5));
        }),
        when(
            "the late image to show",
            |c| shown_width(c, "spaeter.png") == Some(5.0),
            |c| {
                let width = shown_width(c, "spaeter.png");
                check(
                    "an image that appears later shows",
                    width == Some(5.0),
                    &format!("{width:?}"),
                );
                let _ = std::fs::write(doc_dir(c).join("bild.png"), png_of_side(2));
            },
        ),
        undone("the image line is undone in one step".into()),
        when(
            "the image of the next checks to show",
            |c| shown_width(c, "bild.png") == Some(2.0),
            |_| {},
        ),
    ]
}

pub(super) fn memory_steps() -> Vec<Step> {
    vec![applied(|c| {
        crate::editor::images::memory_pressure_changed(true);
        check(
            "while memory is short, images decode smaller",
            crate::editor::picture::allowed_side() == 640,
            "",
        );
        let small = crate::editor::decoder::decode(&png_of_side(700), crate::editor::picture::allowed_side());
        check(
            "the helpers start again after they were ended for memory",
            matches!(small, Some(crate::editor::safe_image::Decoded::Rgba { width: 640, .. })),
            "",
        );
        crate::editor::images::memory_pressure_changed(false);
        check(
            "once memory is back, images decode at full size again",
            crate::editor::picture::allowed_side() == crate::editor::safe_image::SHOWN_SIDE,
            "",
        );
        check("the shown image survives", shown_width(c, "bild.png").is_some(), "");
    })]
}

pub(super) fn old_encoding_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            let path = doc_dir(c).join("alt.md");
            let mut utf16 = vec![0xFF, 0xFE];
            utf16.extend("# Grüße\n".encode_utf16().flat_map(u16::to_le_bytes));
            let _ = std::fs::write(&path, utf16);
            let done = RcBlock::new(
                |_: *mut objc2_app_kit::NSDocument, _: objc2::runtime::Bool, _: *mut objc2_foundation::NSError| {},
            );
            NSDocumentController::sharedDocumentController(c.e.mtm())
                .openDocumentWithContentsOfURL_display_completionHandler(
                    &NSURL::fileURLWithPath(&ns(path.to_str().unwrap_or_default())),
                    true,
                    &done,
                );
        }),
        when(
            "the file in UTF-16 to open",
            |c| {
                documents(c.e.mtm())
                    .iter()
                    .any(|d| d.displayName().to_string().starts_with("alt"))
            },
            |c| {
                let other = documents(c.e.mtm())
                    .into_iter()
                    .find(|d| d.displayName().to_string().starts_with("alt"));
                check(
                    "a file in UTF-16 opens with its umlauts",
                    other.as_ref().is_some_and(|d| d.text() == "# Grüße\n"),
                    "",
                );
                if let Some(d) = other {
                    d.close();
                }
                if let Some(window) = c.e.window() {
                    window.makeKeyAndOrderFront(None);
                }
            },
        ),
    ]
}
