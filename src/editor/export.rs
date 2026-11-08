//! Exporting the document as HTML page or Word document, with its images embedded.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSBitmapImageFileType, NSBitmapImageRep,
    NSDeviceRGBColorSpace, NSGraphicsContext, NSImage, NSPrintInfo, NSPrintOperation, NSPrintingPaginationMode,
    NSSavePanel, NSSharingService,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSMutableAttributedString, NSNumber, NSPoint, NSRect, NSSize, NSURL,
};
use objc2_uniform_type_identifiers::UTType;

use super::attributes::{Attribute, Attributes, attributed};
use super::export_options::{ExportOptions, TitleFacts};
use super::images::{native_only, resolve_path};
use super::layers::{ForSharing, Message};
use super::picture::Sizing;
use super::render::{Live, RenderCtx};
use super::safe_image::{Decoded, EXPORT_SIDE, MAX_FILE_BYTES};
use super::update::{Images, render_key};
use super::{decoder, js, remote, views};
use crate::i18n::t;
use crate::markdown::doc::Figure;
use crate::markdown::docx::{Picture, TitlePage, TooLarge, WordExport, to_docx};
use crate::markdown::html::{Export, data_url, escape, to_html, web_image_type};
use crate::markdown::reader::parse;
use crate::preview::layout::{ParaKind, layout};
use crate::util::{constant, contained, ns, on_main, storage};

/// Where a title stands on its page, from the top of the page.
const TITLE_HEIGHT: f64 = 0.28;
/// A template larger than this holds more than a look.
const MAX_TEMPLATE_BYTES: u64 = 256 << 20;

/// Paper shows the preview the way browsers print a page: a screen point, and so an image pixel, becomes 0.75 pt.
const PRINT_SCALE: f64 = 0.75;

#[derive(Clone, Copy)]
pub(super) enum ExportKind {
    Html,
    Word,
}

impl ExportKind {
    fn extension(self) -> &'static str {
        match self {
            ExportKind::Html => "html",
            ExportKind::Word => "docx",
        }
    }
}

#[derive(Default)]
pub(super) struct ShareState {
    path: RefCell<Option<PathBuf>>,
}

impl Drop for ShareState {
    fn drop(&mut self) {
        if let Some(file) = self.path.get_mut() {
            super::shadow::discard_share_file_later(file);
        }
    }
}

/// The share file to use again: the folder is only worth keeping while the document keeps its name.
fn reusable(previous: Option<PathBuf>, name: &str) -> Option<PathBuf> {
    previous.filter(|p| p.file_name().is_some_and(|n| *n == *name))
}

pub(super) struct Sharing<'a> {
    host: ForSharing<'a>,
    state: &'a ShareState,
}

impl<'a> Sharing<'a> {
    pub(super) fn new(host: ForSharing<'a>, state: &'a ShareState) -> Self {
        Sharing { host, state }
    }
}

impl Sharing<'_> {
    /// The picker opens: the file gets the current text, and the exports stand behind what the system proposes.
    pub(super) fn services_with(&self, proposed: &NSArray<NSSharingService>) -> Retained<NSArray<NSSharingService>> {
        self.share_file(true);
        let mut services = proposed.to_vec();
        services.extend(self.export_sharing_services());
        NSArray::from_retained_slice(&services)
    }

    pub(super) fn service_chosen(&self, service: Option<&NSSharingService>) {
        if service.is_some() {
            self.share_file(true);
        }
    }

    pub(super) fn items(&self) -> Retained<NSArray> {
        match self.share_file(false) {
            Some(url) => NSArray::from_retained_slice(&[crate::util::obj(url)]),
            None => NSArray::new(),
        }
    }

    /// The document as printed: light appearance, smaller type, images no wider than `width`.
    pub(super) fn printable(&self, width: f64) -> Retained<NSMutableAttributedString> {
        self.printable_from(self.host.doc_dir(), width)
    }

    /// `printable` for a document that lies in `doc_dir`.
    pub(super) fn printable_from(&self, doc_dir: Option<PathBuf>, width: f64) -> Retained<NSMutableAttributedString> {
        let dir = doc_dir.clone();
        let macros = self.host.model().read(|st| st.macros());
        let for_layout = macros.clone();
        let print_layout = self.host.on_source_text(|text| {
            // Printing runs on the main thread, and what the disk thread knows does not wait for a volume.
            let env = super::env::Env::known(dir, text).sendable();
            let images = Images {
                dark: false,
                env,
                macros: for_layout,
            };
            contained(|| layout(&parse(text), &images, None))
        });
        let Some(print_layout) = print_layout else {
            return NSMutableAttributedString::from_nsstring(&ns(&self.host.source_text()));
        };
        let live = Live { requester: None };
        let ctx = RenderCtx {
            mtm: self.host.mtm(),
            zoom: PRINT_SCALE,
            doc_dir: doc_dir.as_deref(),
            images: self.host.pictures().store(),
            max_image_width: width - 20.0,
            dark: false,
            controller: None,
            remote_images: self.host.pictures().remote_allowed(),
            leaves_the_mac: true,
            macros,
            drawn: &live,
        };
        let out = NSMutableAttributedString::new();
        // The summary of the metadata is there to open its form, and neither the HTML nor the Word export has it.
        let on_paper = print_layout.blocks.iter().filter(|block| {
            !block
                .paras
                .iter()
                .all(|para| matches!(para.style.kind, ParaKind::FrontMatter))
        });
        for block in on_paper {
            out.appendAttributedString(&ctx.render_block(block));
        }
        out
    }

    pub(super) fn export(&self, format: ExportKind) {
        let (Some(window), Some(doc)) = (self.host.window(), self.host.document()) else {
            return;
        };
        let panel = NSSavePanel::savePanel(self.host.mtm());
        if let Some(kind) = UTType::typeWithFilenameExtension(&ns(format.extension())) {
            panel.setAllowedContentTypes(&NSArray::from_retained_slice(&[kind]));
        }
        let stem = doc.displayName().to_string();
        let stem = stem.rsplit_once('.').map_or(stem.as_str(), |(s, _)| s).to_string();
        panel.setNameFieldStringValue(&ns(&format!("{stem}.{}", format.extension())));
        let (messenger, mtm) = (self.host.object().messenger(), self.host.mtm());
        let chosen = panel.clone();
        let handler = block2::RcBlock::new(move |response: objc2_app_kit::NSModalResponse| {
            crate::util::guarded((), || {
                if response != objc2_app_kit::NSModalResponseOK {
                    return;
                }
                let Some(path) = chosen.URL().and_then(|u| u.path()) else {
                    return;
                };
                chosen.orderOut(None);
                let (path, title) = (PathBuf::from(path.to_string()), stem.clone());
                messenger.send(mtm, Message::ExportTo { format, path, title });
            });
        });
        panel.beginSheetModalForWindow_completionHandler(&window, &handler);
    }

    /// Writes `source` as an export to `path`; a failure comes back as a message.
    pub(super) fn export_to(&self, source: ExportSource, format: ExportKind, path: PathBuf, title: String) {
        let messenger = self.host.object().messenger();
        // Each export has a thread of its own: it reads from one volume and writes to another, and either may hang.
        std::thread::spawn(move || {
            if let Err(failure) = source.write(format, &title, &path) {
                messenger.send_soon(Message::ExportFailed(failure));
            }
        });
    }

    pub(super) fn export_source(&self) -> ExportSource {
        ExportSource {
            text: self.host.source_text(),
            doc_dir: self.host.doc_dir(),
            macros: self.host.model().read(|st| st.macros()),
            remote: self.host.pictures().remote_allowed(),
            options: ExportOptions::load(),
        }
    }

    /// The sheet for what exports add to the text; it holds for all documents from then on.
    pub(super) fn ask_for_options(&self) {
        if let Some(window) = self.host.window() {
            super::dialogs::export_options(&window, ExportOptions::load(), |options| options.save());
        }
    }

    pub(super) fn report_export_failure(&self, failure: &ExportFailure) {
        let Some(window) = self.host.window() else { return };
        let alert = objc2_app_kit::NSAlert::new(self.host.mtm());
        alert.setMessageText(&ns(t("Export fehlgeschlagen")));
        alert.setInformativeText(&ns(&failure.reason()));
        alert.beginSheetModalForWindow_completionHandler(&window, None);
    }

    /// The exports as entries of the share menu: PDF through the document's own export, HTML and Word like their
    /// menu commands.
    pub(super) fn export_sharing_services(&self) -> Vec<Retained<NSSharingService>> {
        let Some(image) = objc2_app_kit::NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &ns("arrow.down.document"),
            None,
        ) else {
            return Vec::new();
        };
        let service = |title: &str, handler: block2::RcBlock<dyn Fn()>| {
            NSSharingService::initWithTitle_image_alternateImage_handler(
                <NSSharingService as objc2::AnyThread>::alloc(),
                &ns(title),
                &image,
                None,
                &handler,
            )
        };
        let document = self.host.weak_document();
        let mut services = vec![service(
            t("Als PDF exportieren …"),
            block2::RcBlock::new(move || {
                crate::util::guarded((), || {
                    if let Some(doc) = document.load() {
                        // SAFETY: The export action accepts a missing sender.
                        unsafe { doc.saveDocumentToPDF(None) };
                    }
                });
            }),
        )];
        for action in [super::Action::ExportHtml, super::Action::ExportWord] {
            let (messenger, mtm) = (self.host.object().messenger(), self.host.mtm());
            services.push(service(
                action.spec().title.map_or("", t),
                block2::RcBlock::new(move || {
                    crate::util::guarded((), || messenger.send(mtm, Message::Perform(action)));
                }),
            ));
        }
        services
    }

    /// Temporary Markdown file the share menu hands out, so unsaved changes go along; `write_text` puts the current
    /// text into it. Until then the file stays empty, since AppKit offers no service for a file that does not exist.
    pub(super) fn share_file(&self, write_text: bool) -> Option<Retained<NSURL>> {
        if !self.host.panes().is_built() {
            return None;
        }
        let doc = self.host.document()?;
        let name = crate::util::file_name_of(&match doc.fileURL().and_then(|u| u.lastPathComponent()) {
            Some(name) => name.to_string(),
            None => format!("{}.md", doc.displayName()),
        });
        let previous = self.state.path.borrow().clone();
        let path = match reusable(previous, &name) {
            Some(path) => path,
            None => super::shadow::new_share_folder()?.join(name),
        };
        if write_text {
            self.host.on_source_text(|text| std::fs::write(&path, text)).ok()?;
        } else if !path.exists() {
            std::fs::File::create(&path).ok()?;
        }
        let url = NSURL::fileURLWithPath(&ns(path.to_str()?));
        *self.state.path.borrow_mut() = Some::<PathBuf>(path);
        Some(url)
    }

    pub(super) fn print_operation(&self, info: &NSPrintInfo) -> Retained<NSPrintOperation> {
        let mtm = self.host.mtm();
        let margin = 56.0;
        info.setLeftMargin(margin);
        info.setRightMargin(margin);
        info.setTopMargin(margin);
        info.setBottomMargin(margin);
        info.setHorizontalPagination(NSPrintingPaginationMode::Clip);
        info.setVerticalPagination(NSPrintingPaginationMode::Automatic);
        info.setVerticallyCentered(false);
        info.setHorizontallyCentered(false);
        let width = info.paperSize().width - 2.0 * margin;
        let view = views::print_view(mtm, width);
        if let Some(aqua) = NSAppearance::appearanceNamed(constant!(NSAppearanceNameAqua)) {
            view.setAppearance(Some(&aqua));
        }
        let out = self.printable(width);
        let options = ExportOptions::load();
        let name = self.document_name();
        let facts = self
            .host
            .model()
            .read(|st| contained(|| TitleFacts::of(st.doc(), &name)).unwrap_or_default());
        let title_length = options.title_page.then(|| {
            let page = title_page_text(&facts, info.paperSize().height);
            let length = page.length();
            page.appendAttributedString(&out);
            storage(&view).setAttributedString(&page);
            length
        });
        if title_length.is_none() {
            storage(&view).setAttributedString(&out);
        }
        views::fit_height(&view);
        let title_end = title_length.map(|length| views::line_bottom(&view, length.saturating_sub(1)));
        let key = constant!(objc2_app_kit::NSPrintHeaderAndFooter);
        // SAFETY: The key belongs to NSPrintInfo's dictionary and takes a number holding a boolean.
        unsafe {
            info.dictionary().setObject_forKey(
                &*crate::util::obj(NSNumber::new_bool(options.decorates_pages())),
                objc2::runtime::ProtocolObject::from_ref(key),
            );
        }
        view.set_pages(views::PageLook {
            options,
            facts,
            title_end,
        });
        NSPrintOperation::printOperationWithView_printInfo(&view, info)
    }

    /// The name the document shows, without its extension.
    fn document_name(&self) -> String {
        let name = self
            .host
            .document()
            .map(|doc| doc.displayName().to_string())
            .unwrap_or_default();
        name.rsplit_once('.')
            .map_or(name.as_str(), |(stem, _)| stem)
            .to_string()
    }
}

/// Why an export did not reach its file.
#[derive(Debug)]
pub(super) enum ExportFailure {
    /// A bug in making the export, which the panic hook has reported.
    NotConverted,
    TooLargeForWord,
    TemplateNotRead,
    TemplateNotUsable,
    NotWritten(std::io::Error),
}

impl ExportFailure {
    fn reason(&self) -> String {
        match self {
            ExportFailure::NotConverted => t("Das Dokument ließ sich nicht umwandeln.").to_string(),
            ExportFailure::TooLargeForWord => t("Das Dokument ist zu groß für eine Word-Datei.").to_string(),
            ExportFailure::TemplateNotRead => t("Die Word-Vorlage ließ sich nicht lesen.").to_string(),
            ExportFailure::TemplateNotUsable => t("Die Word-Vorlage ist kein gültiges Word-Dokument.").to_string(),
            ExportFailure::NotWritten(error) => described(error),
        }
    }
}

/// What an export takes from the window, so that the rest can wait for the volumes on another thread.
pub(super) struct ExportSource {
    text: String,
    doc_dir: Option<PathBuf>,
    remote: bool,
    macros: Arc<str>,
    options: ExportOptions,
}

impl ExportSource {
    /// As if the document lay in `dir`, for a check without a file.
    #[cfg(feature = "selftest")]
    pub(super) fn beside(mut self, dir: PathBuf) -> ExportSource {
        self.doc_dir = Some(dir);
        self
    }

    /// Writes the document in `format` to `path`.
    pub(super) fn write(&self, format: ExportKind, title: &str, path: &Path) -> Result<(), ExportFailure> {
        let data = match format {
            ExportKind::Html => contained(|| self.html(title, path))
                .ok_or(ExportFailure::NotConverted)?
                .into_bytes(),
            ExportKind::Word => contained(|| self.word_file(title)).ok_or(ExportFailure::NotConverted)??,
        };
        std::fs::write(path, data).map_err(ExportFailure::NotWritten)
    }

    pub(super) fn html(&self, title: &str, target: &Path) -> String {
        let doc_dir = self.doc_dir.as_deref();
        let export_dir = target.parent().map(Path::to_path_buf);
        let figure = |f: &Figure| on_main(|| figure_image(f, &self.macros)).flatten();
        let embed = |url: &str| embedded(url, doc_dir);
        let rebase = |link: &str| rebased(link, doc_dir, export_dir.as_deref());
        let env = super::env::Env::now(self.doc_dir.clone(), &self.text).sendable();
        let reference = |r: &str| env.repository.as_ref()?.url(r);
        let citation = |k: &str| env.citations.as_ref()?.get(k).cloned();
        to_html(
            &parse(&self.text),
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

    /// The Word file with the options applied, in the look of the template if one is set.
    pub(super) fn word_file(&self, title: &str) -> Result<Vec<u8>, ExportFailure> {
        let generated = self.word(title).map_err(|TooLarge| ExportFailure::TooLargeForWord)?;
        let Some(template) = &self.options.word_template else {
            return Ok(generated);
        };
        let template =
            crate::util::read_regular_file(template, MAX_TEMPLATE_BYTES).ok_or(ExportFailure::TemplateNotRead)?;
        splat_import::template::apply_template(&generated, &template).map_err(|error| match error {
            splat_import::WordError::TooLarge => ExportFailure::TooLargeForWord,
            _ => ExportFailure::TemplateNotUsable,
        })
    }

    pub(super) fn word(&self, title: &str) -> Result<Vec<u8>, TooLarge> {
        let doc_dir = self.doc_dir.as_deref();
        let figure = |f: &Figure| {
            on_main(|| {
                let rendered = js::cached(&render_key(f, false, &self.macros))?;
                // One ex is about half the 11 pt body text.
                let size = match rendered.sizing {
                    Sizing::Ex { width, height, .. } => NSSize::new(width * 5.5, height * 5.5),
                    Sizing::Points { size, .. } => size,
                };
                picture_of(&rendered.image, size)
            })
            .flatten()
        };
        let image = |url: &str| {
            let picture = if remote::is_remote(url) {
                on_main(|| {
                    let rendered = self.remote.then(|| remote::get(url, None)).flatten().flatten()?;
                    picture_of(&rendered.image, rendered.image.size())
                })
                .flatten()?
            } else {
                local_picture(&resolve_path(url, doc_dir)?, doc_dir)?
            };
            Some(Picture {
                width: picture.width * PRINT_SCALE,
                height: picture.height * PRINT_SCALE,
                ..picture
            })
        };
        let env = super::env::Env::now(self.doc_dir.clone(), &self.text).sendable();
        let reference = |r: &str| env.repository.as_ref()?.url(r);
        let citation = |k: &str| env.citations.as_ref()?.get(k).cloned();
        let doc = parse(&self.text);
        let page = self.options.title_page.then(|| {
            let facts = TitleFacts::of(&doc, title);
            TitlePage {
                title: facts.title,
                subtitle: facts.subtitle,
                author: facts.author,
                date: facts.date,
            }
        });
        to_docx(
            &doc,
            &WordExport {
                title: page.as_ref(),
                figure: &figure,
                image: &image,
                quotes: env.quotes,
                reference: &reference,
                citation: &citation,
            },
        )
    }
}

/// The title page for paper of `height` points, which ends with the first page: what follows starts the next one.
fn title_page_text(facts: &TitleFacts, height: f64) -> Retained<NSMutableAttributedString> {
    let page = NSMutableAttributedString::new();
    let centered = |top: f64| {
        let style = objc2_app_kit::NSMutableParagraphStyle::new();
        style.setAlignment(objc2_app_kit::NSTextAlignment::Center);
        style.setParagraphSpacingBefore(top);
        style.setParagraphSpacing(8.0);
        style
    };
    let line = |text: &str, size: f64, bold: bool, top: f64| {
        let font = if bold {
            objc2_app_kit::NSFont::boldSystemFontOfSize(size)
        } else {
            objc2_app_kit::NSFont::systemFontOfSize(size)
        };
        let attributes = Attributes::new(&[Attribute::font(&font), Attribute::paragraph(&centered(top))]);
        attributed(&format!("{text}\n"), Some(&attributes))
    };
    // A first paragraph's spacing before counts for nothing at the top of a page, so an empty line holds the title down.
    let spacer = objc2_app_kit::NSMutableParagraphStyle::new();
    spacer.setMinimumLineHeight(height * TITLE_HEIGHT);
    page.appendAttributedString(&attributed(
        "\n",
        Some(&Attributes::new(&[Attribute::paragraph(&spacer)])),
    ));
    page.appendAttributedString(&line(&facts.title, 28.0, true, 0.0));
    for (text, size) in [(&facts.subtitle, 16.0), (&facts.author, 13.0), (&facts.date, 12.0)] {
        if !text.is_empty() {
            page.appendAttributedString(&line(text, size, false, 6.0));
        }
    }
    page
}

/// The system's wording of a failure, in the language of the app.
fn described(e: &std::io::Error) -> String {
    let Some(code) = e.raw_os_error().and_then(|code| isize::try_from(code).ok()) else {
        return e.to_string();
    };
    NSError::new(code, constant!(objc2_foundation::NSPOSIXErrorDomain))
        .localizedDescription()
        .to_string()
}

/// A rendered formula or diagram as embedded PNG, at twice its size for sharp display.
fn figure_image(figure: &Figure, macros: &Arc<str>) -> Option<String> {
    let rendered = js::cached(&render_key(figure, false, macros))?;
    // One ex is about half the 16 px body text of the page.
    let (size, attributes) = match rendered.sizing {
        Sizing::Ex { width, height, valign } => (
            NSSize::new(width * 8.0, height * 8.0),
            format!(" class=\"math\" style=\"height: {height:.3}ex; vertical-align: {valign:.3}ex\""),
        ),
        Sizing::Points { size, .. } => (size, format!(" width=\"{:.0}\"", size.width)),
    };
    let png = png_of_image(&rendered.image, size, 2.0)?;
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
        data: png_of_image(image, size, 2.0)?,
        jpeg: false,
        width: size.width,
        height: size.height,
    })
}

/// Pixels from a helper, or from the other one for what only the system reads.
fn decoded_file(path: &Path, bytes: &[u8]) -> Option<Decoded> {
    decoder::decode(bytes, EXPORT_SIDE).or_else(|| {
        native_only(path, bytes)
            .then(|| decoder::decode_native(bytes, EXPORT_SIDE))
            .flatten()
    })
}

fn png_of_pixels(width: u32, height: u32, pixels: Vec<u8>) -> Option<Vec<u8>> {
    let image = image::RgbaImage::from_raw(width, height, pixels)?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// A local image for Word: PNG and JPEG as they are, other formats converted to PNG.
fn local_picture(path: &Path, doc_dir: Option<&Path>) -> Option<Picture> {
    let path = &super::links::locate(path, doc_dir);
    if !super::env::may_carry_along(path, doc_dir) {
        return None;
    }
    let bytes = crate::util::read_regular_file(path, MAX_FILE_BYTES as u64)?;
    let Decoded::Rgba {
        width,
        height,
        pixels,
        natural,
    } = decoded_file(path, &bytes)?
    else {
        return None;
    };
    if let Some(mime @ ("image/png" | "image/jpeg")) = web_image_type(&bytes) {
        return Some(Picture {
            data: bytes,
            jpeg: mime == "image/jpeg",
            width: f64::from(width),
            height: f64::from(height),
        });
    }
    // A vector picture has more pixels than points, and it is the points that say how large it is.
    let vector = natural.0 < width && natural.1 < height;
    let (w, h) = if vector { natural } else { (width, height) };
    Some(Picture {
        data: png_of_pixels(width, height, pixels)?,
        jpeg: false,
        width: f64::from(w),
        height: f64::from(h),
    })
}

/// A local image as `data:` URL: web formats as they are, others converted to PNG. Images from the web stay links.
fn embedded(url: &str, doc_dir: Option<&Path>) -> Option<String> {
    if url.contains("://") && crate::markdown::url::behind_file_scheme(url).is_none() {
        return None;
    }
    let path = super::links::locate(&resolve_path(url, doc_dir)?, doc_dir);
    if !super::env::may_carry_along(&path, doc_dir) {
        return None;
    }
    let bytes = crate::util::read_regular_file(&path, MAX_FILE_BYTES as u64)?;
    if let Some(mime) = web_image_type(&bytes) {
        return Some(data_url(mime, &bytes));
    }
    // Scripts in an SVG do not run when a page shows it as image.
    if std::str::from_utf8(&bytes).is_ok_and(|t| t.contains("<svg")) {
        return Some(data_url("image/svg+xml", &bytes));
    }
    let Decoded::Rgba {
        width, height, pixels, ..
    } = decoded_file(&path, &bytes)?
    else {
        return None;
    };
    Some(data_url("image/png", &png_of_pixels(width, height, pixels)?))
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
    let target = doc_dir.join(crate::markdown::url::percent_decode(path));
    let relative = super::links::relative_path(Some(export_dir), &target);
    // Without a folder in common the only way there is an absolute path, which would show the user's folders to the reader.
    if Path::new(&relative).is_absolute() {
        return link.to_string();
    }
    format!("{}{suffix}", in_an_address(&relative))
}

/// A path as a link can name it: the characters that would end the path or start an escape are escaped themselves.
fn in_an_address(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            '%' => out.push_str("%25"),
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            c => out.push(c),
        }
    }
    out
}

/// Draws an image into a bitmap of `size` points at `scale` and encodes it as PNG.
fn png_of_image(image: &NSImage, size: NSSize, scale: f64) -> Option<Vec<u8>> {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "pixels of a picture, and checked just below"
    )]
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
    fn the_share_file_is_used_again_only_under_the_documents_name() {
        let previous = || Some(PathBuf::from("/tmp/share-1/Bericht.md"));
        assert_eq!(reusable(previous(), "Bericht.md"), previous());
        assert_eq!(
            reusable(previous(), "Neu.md"),
            None,
            "a renamed document gets a new file"
        );
        assert_eq!(reusable(None, "Bericht.md"), None);
    }

    #[test]
    fn a_failed_write_reads_like_the_system_says_it() {
        let said = described(&std::io::Error::from_raw_os_error(13));
        assert!(!said.is_empty() && !said.contains("os error"), "{said}");
        assert_eq!(described(&std::io::Error::other("kaputt")), "kaputt");
    }

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

    #[test]
    fn a_rebased_link_leads_to_the_same_file() {
        let docs = Some(Path::new("/Users/a/Notizen"));
        let export = Some(Path::new("/Users/a/Export"));
        assert_eq!(rebased("a%23b.md", docs, export), "../Notizen/a%23b.md");
        assert_eq!(rebased("a%3Fb.md#ende", docs, export), "../Notizen/a%3Fb.md#ende");
        assert_eq!(rebased("100%25.md", docs, export), "../Notizen/100%25.md");
    }

    #[test]
    fn a_link_without_a_folder_in_common_shows_no_folders() {
        assert_eq!(
            rebased("a.md", Some(Path::new("/Users/a/Notizen")), Some(Path::new("/tmp"))),
            "a.md"
        );
    }
}
