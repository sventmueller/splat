//! Turning a Word document into a Markdown file with its images beside it, and opening that.

use std::path::{Path, PathBuf};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSApplication, NSDocument, NSDocumentController, NSModalResponse, NSModalResponseOK, NSOpenPanel,
    NSSavePanel,
};
use objc2_foundation::{NSArray, NSError, NSURL};
use objc2_uniform_type_identifiers::UTType;
use splat_import::docx::{ImportError, import};

use crate::i18n::t;
use crate::util::{contained, guarded, main_soon, ns, read_regular_file};

/// A Word file beyond this is no text document anybody would write in Markdown, and it stays in memory while it is read.
const MOST_BYTES: u64 = 1 << 30;

/// Why a Word document did not become a Markdown file.
#[derive(Debug)]
pub(crate) enum ImportFailure {
    NotRead,
    Refused(ImportError),
    /// A bug in the conversion, which the panic hook has reported.
    NotConverted,
    NotWritten(std::io::Error),
}

impl ImportFailure {
    fn reason(&self) -> String {
        match self {
            ImportFailure::NotRead => t("Die Datei ließ sich nicht lesen.").to_string(),
            ImportFailure::Refused(ImportError::NotAWordFile) => {
                t("Das ist kein Word-Dokument im Format .docx.").to_string()
            }
            ImportFailure::Refused(ImportError::Encrypted) => {
                t("Das Dokument ist mit einem Kennwort geschützt.").to_string()
            }
            ImportFailure::Refused(ImportError::TooLarge) => t("Das Dokument ist zu groß.").to_string(),
            ImportFailure::Refused(ImportError::Damaged) => t("Das Dokument ist beschädigt.").to_string(),
            ImportFailure::NotConverted => t("Das Dokument ließ sich nicht umwandeln.").to_string(),
            ImportFailure::NotWritten(error) => error.to_string(),
        }
    }
}

/// The folder for the images of the Markdown file at `target`: its name with `-images`, numbered if that is taken.
fn images_folder(target: &Path) -> String {
    let stem = target
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let free = |name: &str| !target.with_file_name(name).exists();
    let plain = format!("{stem}-images");
    if free(&plain) {
        return plain;
    }
    (2..)
        .map(|n| format!("{stem}-images-{n}"))
        .find(|name| free(name))
        .unwrap_or(plain)
}

/// Writes the Markdown file at `target` for the Word document at `word`, with its images in a folder beside it.
pub(crate) fn import_file(word: &Path, target: &Path) -> Result<(), ImportFailure> {
    let bytes = read_regular_file(word, MOST_BYTES).ok_or(ImportFailure::NotRead)?;
    let folder = images_folder(target);
    let imported = contained(|| import(&bytes, &folder))
        .ok_or(ImportFailure::NotConverted)?
        .map_err(ImportFailure::Refused)?;
    if !imported.missing.is_empty() {
        let reasons: Vec<String> = imported
            .missing
            .iter()
            .map(|missing| missing.reason.to_string())
            .collect();
        crate::diag::report("a Word import left pictures out", reasons.join(", "));
    }
    if !imported.media.is_empty() {
        let dir = target.with_file_name(&folder);
        std::fs::create_dir_all(&dir).map_err(ImportFailure::NotWritten)?;
        for media in &imported.media {
            std::fs::write(dir.join(&media.file_name), &media.bytes).map_err(ImportFailure::NotWritten)?;
        }
    }
    std::fs::write(target, imported.markdown).map_err(ImportFailure::NotWritten)
}

fn word_types() -> Retained<NSArray<UTType>> {
    let types: Vec<Retained<UTType>> = ["docx", "dotx"]
        .iter()
        .filter_map(|extension| UTType::typeWithFilenameExtension(&ns(extension)))
        .collect();
    NSArray::from_retained_slice(&types)
}

/// Asks for a Word document and then for where its Markdown goes, and opens the result. The panel it shows comes back.
pub(crate) fn ask(mtm: MainThreadMarker) -> Retained<NSOpenPanel> {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(false);
    panel.setAllowsMultipleSelection(false);
    panel.setAllowedContentTypes(&word_types());
    panel.setPrompt(Some(&ns(t("Importieren"))));
    let chosen = panel.clone();
    let handler = RcBlock::new(move |response: NSModalResponse| {
        guarded((), || {
            if response != NSModalResponseOK {
                return;
            }
            if let Some(path) = chosen.URL().and_then(|u| u.path()) {
                ask_where(chosen.mtm(), PathBuf::from(path.to_string()));
            }
        });
    });
    panel.beginWithCompletionHandler(&handler);
    panel
}

/// Asks where the Markdown of the Word document at `word` goes. The panel it shows comes back.
pub(crate) fn ask_where(mtm: MainThreadMarker, word: PathBuf) -> Retained<NSSavePanel> {
    let panel = NSSavePanel::savePanel(mtm);
    if let Some(markdown) = UTType::typeWithIdentifier(&ns("net.daringfireball.markdown")) {
        panel.setAllowedContentTypes(&NSArray::from_retained_slice(&[markdown]));
    }
    let stem = word
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    panel.setNameFieldStringValue(&ns(&format!("{stem}.md")));
    if let Some(folder) = word.parent().and_then(Path::to_str) {
        panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&ns(folder))));
    }
    let chosen = panel.clone();
    let handler = RcBlock::new(move |response: NSModalResponse| {
        guarded((), || {
            if response != NSModalResponseOK {
                return;
            }
            let Some(target) = chosen.URL().and_then(|u| u.path()) else {
                return;
            };
            import_and_open(word.clone(), PathBuf::from(target.to_string()));
        });
    });
    panel.beginWithCompletionHandler(&handler);
    panel
}

/// Makes the Markdown file on a thread of its own, as it reads from one volume and writes to another and either may hang.
pub(crate) fn import_and_open(word: PathBuf, target: PathBuf) {
    std::thread::spawn(move || {
        let result = import_file(&word, &target);
        main_soon(move || {
            let Some(mtm) = MainThreadMarker::new() else { return };
            match result {
                Ok(()) => open(mtm, &target),
                Err(failure) => report(mtm, &failure),
            }
        });
    });
}

fn open(mtm: MainThreadMarker, target: &Path) {
    let Some(address) = target.to_str().map(|p| NSURL::fileURLWithPath(&ns(p))) else {
        return;
    };
    let done = RcBlock::new(|_: *mut NSDocument, _: objc2::runtime::Bool, error: *mut NSError| {
        guarded((), || {
            // SAFETY: The completion handler's error is nil or a valid NSError for the duration of the call.
            let Some(error) = (unsafe { Retained::retain(error) }) else {
                return;
            };
            if let Some(mtm) = MainThreadMarker::new() {
                NSApplication::sharedApplication(mtm).presentError(&error);
            }
        });
    });
    NSDocumentController::sharedDocumentController(mtm)
        .openDocumentWithContentsOfURL_display_completionHandler(&address, true, &done);
}

fn report(mtm: MainThreadMarker, failure: &ImportFailure) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&ns(t("Import fehlgeschlagen")));
    alert.setInformativeText(&ns(&failure.reason()));
    // The panel that led here may still count among the app's windows, so not just any window will do.
    let window = NSApplication::sharedApplication(mtm).mainWindow().or_else(|| {
        NSDocumentController::sharedDocumentController(mtm)
            .documents()
            .firstObject()
            .and_then(|document| document.windowForSheet())
    });
    match window {
        Some(window) => alert.beginSheetModalForWindow_completionHandler(&window, None),
        None => {
            alert.runModal();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_images_of_a_document_get_a_folder_nobody_uses() {
        let dir = std::env::temp_dir().join(format!("splat-import-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Bericht-images")).unwrap();
        assert_eq!(images_folder(&dir.join("Neu.md")), "Neu-images");
        assert_eq!(images_folder(&dir.join("Bericht.md")), "Bericht-images-2");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn what_is_no_word_file_says_so_and_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("splat-import-refused-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (word, target) = (dir.join("a.docx"), dir.join("a.md"));
        std::fs::write(&word, "kein Word").unwrap();
        let failure = import_file(&word, &target).unwrap_err();
        assert!(matches!(failure, ImportFailure::Refused(ImportError::NotAWordFile)));
        assert!(!failure.reason().is_empty());
        assert!(!target.exists());
        assert!(matches!(
            import_file(&dir.join("fehlt.docx"), &target),
            Err(ImportFailure::NotRead)
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A Word file of `markdown` whose pictures are a red square, without the parts `leave_out` names.
    fn word_file(markdown: &str, leave_out: impl Fn(&str) -> bool) -> Vec<u8> {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 30, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let picture = |_: &str| {
            Some(crate::markdown::docx::Picture {
                data: png.clone(),
                jpeg: false,
                width: 2.0,
                height: 2.0,
            })
        };
        let word = crate::markdown::docx::to_docx(
            &crate::markdown::reader::parse(markdown),
            &crate::markdown::docx::WordExport {
                title: None,
                figure: &|_| None,
                image: &picture,
                quotes: None,
                reference: &|_| None,
                citation: &|_| None,
            },
        )
        .unwrap();
        let archive = splat_import::zip::Archive::read(&word).unwrap();
        let mut zip = crate::markdown::docx::zip::Zip::default();
        for name in archive.names().filter(|name| !leave_out(name)) {
            zip.add(name, &archive.get(name).unwrap().unwrap()).unwrap();
        }
        zip.finish()
    }

    #[test]
    fn a_picture_goes_into_the_folder_beside_the_markdown_and_a_missing_one_leaves_its_text() {
        let dir = std::env::temp_dir().join(format!("splat-import-pictures-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (word, target) = (dir.join("Bericht.docx"), dir.join("Bericht.md"));
        std::fs::write(&word, word_file("![Ein Bild](bild.png)\n", |_| false)).unwrap();
        import_file(&word, &target).unwrap();
        let markdown = std::fs::read_to_string(&target).unwrap();
        let pictures: Vec<_> = std::fs::read_dir(dir.join("Bericht-images"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(pictures.len(), 1, "{markdown}");
        assert!(markdown.contains("Bericht-images/"), "{markdown}");
        let (word, target) = (dir.join("Ohne.docx"), dir.join("Ohne.md"));
        std::fs::write(
            &word,
            word_file("![Ein Bild](bild.png)\n", |name| name.starts_with("word/media/")),
        )
        .unwrap();
        import_file(&word, &target).unwrap();
        let markdown = std::fs::read_to_string(&target).unwrap();
        assert!(markdown.contains("Ein Bild") && !markdown.contains("!["), "{markdown}");
        assert!(!dir.join("Ohne-images").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_failure_says_in_its_own_words_what_went_wrong() {
        let failures = [
            ImportFailure::NotRead,
            ImportFailure::Refused(ImportError::NotAWordFile),
            ImportFailure::Refused(ImportError::Encrypted),
            ImportFailure::Refused(ImportError::TooLarge),
            ImportFailure::Refused(ImportError::Damaged),
            ImportFailure::NotConverted,
            ImportFailure::NotWritten(std::io::Error::other("voll")),
        ];
        let reasons: std::collections::HashSet<String> = failures.iter().map(ImportFailure::reason).collect();
        assert_eq!(reasons.len(), failures.len(), "{reasons:?}");
        assert!(reasons.iter().all(|r| !r.is_empty()));
    }
}
