//! Exporting the document as HTML page or Word document, with its images embedded.

use std::path::Path;

use objc2::rc::Retained;
use objc2::{AnyThread, DefinedClass, MainThreadOnly, Message};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSDeviceRGBColorSpace, NSGraphicsContext, NSImage, NSSavePanel,
};
use objc2_foundation::{NSArray, NSDictionary, NSMutableAttributedString, NSPoint, NSRect, NSSize};
use objc2_uniform_type_identifiers::UTType;

use super::images::{Sizing, native_only, resolve_path};
use super::render::RenderCtx;
use super::safe_image::{Decoded, MAX_FILE_BYTES};
use super::update::{Images, render_key};
use super::{EditorController, decoder, js, links, remote};
use crate::i18n::t;
use crate::markdown::doc::{Figure, parse};
use crate::markdown::docx::{Picture, WordExport, to_docx};
use crate::markdown::html::{Export, data_url, escape, to_html, web_image_type};
use crate::preview::layout::layout;
use crate::util::ns;

/// Paper shows the preview the way browsers print a page: a screen point, and so an image pixel, becomes 0.75 pt.
const PRINT_SCALE: f64 = 0.75;

#[derive(Clone, Copy)]
pub(super) enum Format {
    Html,
    Word,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Format::Html => "html",
            Format::Word => "docx",
        }
    }
}

impl EditorController {
    /// The document as printed: light appearance, smaller type, images no wider than `width`.
    pub(super) fn printable(&self, width: f64) -> Retained<NSMutableAttributedString> {
        let snap = self.snapshot();
        let doc = parse(&snap.text);
        let env = super::env::Env::new(self.doc_dir(), &snap.text);
        let print_layout = layout(&snap.text, &doc, &Images { dark: false, env }, None);
        let doc_dir = self.doc_dir();
        let ctx = RenderCtx {
            mtm: self.mtm(),
            zoom: PRINT_SCALE,
            doc_dir: doc_dir.as_deref(),
            images: &self.ivars().images,
            max_image_width: width - 20.0,
            dark: false,
            controller: None,
            remote_images: self.ivars().remote_allowed.get(),
        };
        let out = NSMutableAttributedString::new();
        for block in &print_layout.blocks {
            out.appendAttributedString(&ctx.render_block(block));
        }
        out
    }

    pub(super) fn export(&self, format: Format) {
        let (Some(window), Some(doc)) = (self.window(), self.document()) else {
            return;
        };
        let panel = NSSavePanel::savePanel(self.mtm());
        if let Some(kind) = UTType::typeWithFilenameExtension(&ns(format.extension())) {
            panel.setAllowedContentTypes(&NSArray::from_retained_slice(&[kind]));
        }
        let stem = doc.displayName().to_string();
        let stem = stem.rsplit_once('.').map_or(stem.as_str(), |(s, _)| s).to_string();
        panel.setNameFieldStringValue(&ns(&format!("{stem}.{}", format.extension())));
        let this = self.retain();
        let chosen = panel.clone();
        let handler = block2::RcBlock::new(move |response: objc2_app_kit::NSModalResponse| {
            if response != objc2_app_kit::NSModalResponseOK {
                return;
            }
            let Some(path) = chosen.URL().and_then(|u| u.path()) else {
                return;
            };
            let path = std::path::PathBuf::from(path.to_string());
            if let Err(reason) = this.write_export(format, &stem, &path) {
                chosen.orderOut(None);
                this.report_export_failure(&reason);
            }
        });
        panel.beginSheetModalForWindow_completionHandler(&window, &handler);
    }

    /// Writes the document in `format` to `path`; the reason when that failed.
    pub(super) fn write_export(&self, format: Format, title: &str, path: &Path) -> Result<(), String> {
        match format {
            Format::Html => std::fs::write(path, self.html(title, path)).map_err(|e| e.to_string()),
            Format::Word => self
                .word()
                .and_then(|data| std::fs::write(path, data).map_err(|e| e.to_string())),
        }
    }

    pub(super) fn html(&self, title: &str, target: &Path) -> String {
        let snap = self.snapshot();
        let doc_dir = self.doc_dir();
        let export_dir = target.parent().map(Path::to_path_buf);
        let figure = |f: &Figure| figure_image(f);
        let embed = |url: &str| embedded(url, doc_dir.as_deref());
        let rebase = |link: &str| rebased(link, doc_dir.as_deref(), export_dir.as_deref());
        let env = super::env::Env::new(doc_dir.clone(), &snap.text);
        let reference = |r: &str| env.repository.as_ref()?.url(r);
        let citation = |k: &str| env.citations.as_ref()?.get(k).cloned();
        to_html(
            &snap.text,
            &Export {
                title,
                figure: &figure,
                embed: &embed,
                rebase: &rebase,
                quotes: env.quotes,
                reference: &reference,
                citation: &citation,
            },
        )
    }

    pub(super) fn word(&self) -> Result<Vec<u8>, String> {
        let snap = self.snapshot();
        let doc_dir = self.doc_dir();
        let remote = self.ivars().remote_allowed.get();
        let figure = |f: &Figure| {
            let rendered = js::cached(&render_key(f, false))?;
            // One ex is about half the 11 pt body text.
            let size = match rendered.sizing {
                Sizing::Ex { width, height, .. } => NSSize::new(width * 5.5, height * 5.5),
                Sizing::Points { size, .. } => size,
            };
            picture_of(&rendered.image, size)
        };
        let image = |url: &str| {
            let picture = if remote::is_remote(url) {
                let rendered = remote.then(|| remote::get(url, None)).flatten().flatten()?;
                picture_of(&rendered.image, rendered.image.size())?
            } else {
                local_picture(&resolve_path(url, doc_dir.as_deref())?, doc_dir.as_deref())?
            };
            Some(Picture {
                width: picture.width * PRINT_SCALE,
                height: picture.height * PRINT_SCALE,
                ..picture
            })
        };
        let env = super::env::Env::new(doc_dir.clone(), &snap.text);
        let reference = |r: &str| env.repository.as_ref()?.url(r);
        let citation = |k: &str| env.citations.as_ref()?.get(k).cloned();
        Ok(to_docx(
            &parse(&snap.text),
            &WordExport {
                source: &snap.text,
                figure: &figure,
                image: &image,
                quotes: env.quotes,
                reference: &reference,
                citation: &citation,
            },
        ))
    }

    pub(super) fn report_export_failure(&self, reason: &str) {
        let Some(window) = self.window() else { return };
        let alert = objc2_app_kit::NSAlert::new(self.mtm());
        alert.setMessageText(&ns(t("Export fehlgeschlagen")));
        alert.setInformativeText(&ns(reason));
        alert.beginSheetModalForWindow_completionHandler(&window, None);
    }
}

/// A rendered formula or diagram as embedded PNG, at twice its size for sharp display.
fn figure_image(figure: &Figure) -> Option<String> {
    let rendered = js::cached(&render_key(figure, false))?;
    // One ex is about half the 16 px body text of the page.
    let (size, attributes) = match rendered.sizing {
        Sizing::Ex { width, height, valign } => (
            NSSize::new(width * 8.0, height * 8.0),
            format!(" class=\"math\" style=\"height: {height:.3}ex; vertical-align: {valign:.3}ex\""),
        ),
        Sizing::Points { size, .. } => (size, format!(" width=\"{:.0}\"", size.width)),
    };
    let png = png_of(&rendered.image, size, 2.0)?;
    let alt = match figure {
        Figure::Math { tex, .. } => tex.to_string(),
        Figure::Diagram { .. } => t("Diagramm").to_string(),
    };
    Some(format!(
        "<img src=\"{}\" alt=\"{}\"{attributes}>",
        data_url("image/png", &png),
        escape(&alt)
    ))
}

fn picture_of(image: &NSImage, size: NSSize) -> Option<Picture> {
    Some(Picture {
        data: png_of(image, size, 2.0)?,
        jpeg: false,
        width: size.width,
        height: size.height,
    })
}

/// A local image for Word: PNG and JPEG as they are, other formats converted to PNG.
fn local_picture(path: &Path, doc_dir: Option<&Path>) -> Option<Picture> {
    let path = &super::images::locate(path, doc_dir);
    if std::fs::metadata(path).ok()?.len() > MAX_FILE_BYTES as u64 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    match decoder::decode(&bytes) {
        Some(Decoded::Rgba { width, height, pixels }) => {
            let (w, h) = (f64::from(width), f64::from(height));
            let data = match web_image_type(&bytes) {
                Some(mime @ ("image/png" | "image/jpeg")) => {
                    return Some(Picture {
                        data: bytes,
                        jpeg: mime == "image/jpeg",
                        width: w,
                        height: h,
                    });
                }
                _ => {
                    let image = image::RgbaImage::from_raw(width, height, pixels)?;
                    let mut png = Vec::new();
                    image
                        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                        .ok()?;
                    png
                }
            };
            Some(Picture {
                data,
                jpeg: false,
                width: w,
                height: h,
            })
        }
        Some(pdf @ Decoded::Pdf(_)) => {
            let image = decoder::image(pdf)?;
            picture_of(&image, image.size())
        }
        None if native_only(path, &bytes) => {
            let image = NSImage::initWithData(NSImage::alloc(), &objc2_foundation::NSData::with_bytes(&bytes))?;
            picture_of(&image, image.size())
        }
        None => None,
    }
}

/// A local image as `data:` URL: web formats as they are, others converted to PNG. Images from the web stay links.
fn embedded(url: &str, doc_dir: Option<&Path>) -> Option<String> {
    if url.contains("://") && !url.starts_with("file://") {
        return None;
    }
    let path = super::images::locate(&resolve_path(url, doc_dir)?, doc_dir);
    if std::fs::metadata(&path).ok()?.len() > MAX_FILE_BYTES as u64 {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    if let Some(mime) = web_image_type(&bytes) {
        return Some(data_url(mime, &bytes));
    }
    // Scripts in an SVG do not run when a page shows it as image.
    if std::str::from_utf8(&bytes).is_ok_and(|t| t.contains("<svg")) {
        return Some(data_url("image/svg+xml", &bytes));
    }
    let png = match decoder::decode(&bytes) {
        Some(Decoded::Rgba { width, height, pixels }) => {
            let image = image::RgbaImage::from_raw(width, height, pixels)?;
            let mut png = Vec::new();
            image
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .ok()?;
            png
        }
        _ if native_only(&path, &bytes) => {
            let image = NSImage::initWithData(NSImage::alloc(), &objc2_foundation::NSData::with_bytes(&bytes))?;
            png_of(&image, image.size(), 1.0)?
        }
        _ => return None,
    };
    Some(data_url("image/png", &png))
}

/// A relative link of the document as seen from the folder the page goes to.
fn rebased(link: &str, doc_dir: Option<&Path>, export_dir: Option<&Path>) -> String {
    let (Some(doc_dir), Some(export_dir)) = (doc_dir, export_dir) else {
        return link.to_string();
    };
    if link.starts_with('#') || link.is_empty() {
        return link.to_string();
    }
    let split = link.find(['#', '?']).unwrap_or(link.len());
    let (path, suffix) = link.split_at(split);
    let target = doc_dir.join(links::percent_decode(path));
    let relative = super::editing::relative_path(Some(export_dir), &target);
    format!("{}{suffix}", relative.replace('%', "%25").replace(' ', "%20"))
}

/// Draws an image into a bitmap of `size` points at `scale` and encodes it as PNG.
fn png_of(image: &NSImage, size: NSSize, scale: f64) -> Option<Vec<u8>> {
    let (w, h) = (
        (size.width * scale).ceil() as isize,
        (size.height * scale).ceil() as isize,
    );
    if w <= 0 || h <= 0 || w * h > 64_000_000 {
        return None;
    }
    // SAFETY: without plane pointers the image rep allocates its own buffer of the given size.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            w,
            h,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    rep.setSize(size);
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    image.drawInRect(NSRect::new(NSPoint::new(0.0, 0.0), size));
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    // SAFETY: an empty property dictionary, which every file type accepts.
    let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
    Some(png.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_point_to_the_same_files_from_the_export_folder() {
        let docs = Path::new("/Users/a/Notizen");
        assert_eq!(
            rebased("Teil 2.md#ende", Some(docs), Some(Path::new("/Users/a/Notizen"))),
            "Teil%202.md#ende"
        );
        assert_eq!(
            rebased("bilder/a.png", Some(docs), Some(Path::new("/Users/a/Export"))),
            "../Notizen/bilder/a.png"
        );
        assert_eq!(rebased("#kopf", Some(docs), Some(Path::new("/tmp"))), "#kopf");
        assert_eq!(rebased("a.md", None, Some(Path::new("/tmp"))), "a.md");
    }
}
