use std::cell::OnceCell;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2::runtime::Bool;
use objc2::{AnyThread, MainThreadOnly, Message};
use objc2_app_kit::{NSCompositingOperation, NSImage};
use objc2_foundation::{NSData, NSRect, NSSize};

use super::EditorController;
use super::safe_image::{Decoded, MAX_FILE_BYTES};

#[derive(Clone, Copy)]
pub enum Sizing {
    /// Fixed size in points; `descent` is how far the image reaches below the baseline.
    Points { size: NSSize, descent: f64 },
    /// MathJax metrics in ex of the surrounding font, so formulas scale with the text.
    Ex { width: f64, height: f64, valign: f64 },
}

#[derive(Clone)]
pub struct Rendered {
    pub image: Retained<NSImage>,
    pub sizing: Sizing,
}

enum Slot {
    Loading,
    Done(Option<Rendered>),
}

#[derive(Default)]
pub struct ImageStore {
    files: HashMap<PathBuf, Slot>,
    dimmed: HashMap<String, Rendered>,
}

/// An image file read and decoded off the main thread.
pub enum Loaded {
    Decoded(Decoded),
    /// HEIC, TIFF, AVIF or PDF, which only the system can decode; the content was checked to be that format.
    Native(Vec<u8>),
}

/// Draws the image slightly transparent, so bright pictures do not glare on a dark background.
fn dimmed(image: &NSImage) -> Retained<NSImage> {
    let source = image.retain();
    let block = RcBlock::new(move |rect: NSRect| -> Bool {
        source.drawInRect_fromRect_operation_fraction(rect, NSRect::ZERO, NSCompositingOperation::SourceOver, 0.82);
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(image.size(), false, &block)
}

pub fn resolve_path(url: &str, doc_dir: Option<&Path>) -> Option<PathBuf> {
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("data:") {
        return None;
    }
    let raw = url.strip_prefix("file://").unwrap_or(url);
    let decoded = super::links::percent_decode(raw);
    let path = PathBuf::from(&decoded);
    if path.is_absolute() {
        return Some(path);
    }
    if let Some(stripped) = decoded.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return Some(PathBuf::from(home).join(stripped));
    }
    doc_dir.map(|d| d.join(path))
}

impl ImageStore {
    /// The image a document refers to. With a `requester`, an image not yet loaded loads in the background and the
    /// requester hears when it arrives; without one, it loads right away, as printing needs.
    pub fn file(
        &mut self,
        url: &str,
        doc_dir: Option<&Path>,
        dark: bool,
        requester: Option<&EditorController>,
    ) -> Option<Rendered> {
        let original = self.original(url, doc_dir, requester)?;
        let key = resolve_path(url, doc_dir)?.to_string_lossy().into_owned();
        Some(self.for_appearance(&key, original, dark))
    }

    /// The image as shown on the current appearance: dimmed variants are cached by `key`.
    pub fn for_appearance(&mut self, key: &str, original: Rendered, dark: bool) -> Rendered {
        if !dark {
            return original;
        }
        self.dimmed
            .entry(key.to_string())
            .or_insert_with(|| Rendered {
                image: dimmed(&original.image),
                sizing: original.sizing,
            })
            .clone()
    }

    fn original(
        &mut self,
        url: &str,
        doc_dir: Option<&Path>,
        requester: Option<&EditorController>,
    ) -> Option<Rendered> {
        let path = resolve_path(url, doc_dir)?;
        match self.files.get(&path) {
            Some(Slot::Done(found)) => return found.clone(),
            Some(Slot::Loading) => return None,
            None => {}
        }
        let Some(requester) = requester else {
            let found = load(&path, doc_dir).and_then(rendered);
            self.files.insert(path, Slot::Done(found.clone()));
            return found;
        };
        self.files.insert(path.clone(), Slot::Loading);
        let me = dispatch2::MainThreadBound::new(Weak::from(requester), requester.mtm());
        let doc_dir = doc_dir.map(Path::to_path_buf);
        load_in_background(move || {
            let loaded = load(&path, doc_dir.as_deref());
            dispatch2::DispatchQueue::main().exec_async(move || {
                let Some(mtm) = objc2::MainThreadMarker::new() else {
                    return;
                };
                if let Some(c) = me.get(mtm).load() {
                    c.image_loaded(path, loaded);
                }
            });
        });
        None
    }

    /// Whether the image at `path` loaded and could be shown; `None` while it is not done.
    #[cfg(feature = "selftest")]
    pub fn shown(&self, path: &Path) -> Option<bool> {
        match self.files.get(path)? {
            Slot::Done(found) => Some(found.is_some()),
            Slot::Loading => None,
        }
    }

    pub fn finish(&mut self, path: PathBuf, loaded: Option<Loaded>) {
        self.files.insert(path, Slot::Done(loaded.and_then(rendered)));
    }
}

type Job = Box<dyn FnOnce() + Send>;

thread_local! {
    static LOADERS: OnceCell<mpsc::Sender<Job>> = const { OnceCell::new() };
}

/// Image files are read and decoded by as many threads as there are decoder helpers, so a document with hundreds of
/// pictures holds only a few of them in memory at a time.
fn load_in_background(job: impl FnOnce() + Send + 'static) {
    LOADERS.with(|loaders| {
        let sender = loaders.get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            let rx = Arc::new(Mutex::new(rx));
            for n in 0..super::decoder::HELPER_COUNT {
                let rx = rx.clone();
                let _ = std::thread::Builder::new()
                    .name(format!("splat-image-{n}"))
                    .spawn(move || {
                        loop {
                            let Ok(Ok(job)) = rx.lock().map(|rx| rx.recv()) else {
                                return;
                            };
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                        }
                    });
            }
            tx
        });
        let _ = sender.send(Box::new(job));
    });
}

fn rendered(loaded: Loaded) -> Option<Rendered> {
    let image = match loaded {
        Loaded::Decoded(decoded) => super::decoder::image(decoded)?,
        Loaded::Native(bytes) => NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(&bytes))?,
    };
    let size = image.size();
    Some(Rendered {
        image,
        sizing: Sizing::Points { size, descent: 0.0 },
    })
}

/// The file an image link names; a picture Obsidian embeds as `![[name.png]]` may sit in any folder below the
/// document's. Only there is it searched for, so a document cannot fish for a file name across the disk.
pub fn locate(path: &Path, doc_dir: Option<&Path>) -> PathBuf {
    let below_document = doc_dir
        .and_then(|dir| path.strip_prefix(dir).ok())
        .is_some_and(|rest| rest.components().all(|c| matches!(c, Component::Normal(_))));
    if path.exists() || !below_document {
        return path.to_path_buf();
    }
    path.parent()
        .zip(path.file_name().and_then(|n| n.to_str()))
        .and_then(|(dir, name)| super::links::find_note(dir, name))
        .unwrap_or_else(|| path.to_path_buf())
}

/// Reads and decodes an image file in a decoder helper; only formats Rust cannot decode go to the system.
fn load(path: &Path, doc_dir: Option<&Path>) -> Option<Loaded> {
    let path = &locate(path, doc_dir);
    let size = std::fs::metadata(path).ok()?.len();
    if size > MAX_FILE_BYTES as u64 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if let Some(decoded) = super::decoder::decode(&bytes) {
        return Some(Loaded::Decoded(decoded));
    }
    native_only(path, &bytes).then_some(Loaded::Native(bytes))
}

/// The system decoder recognises formats by their content, not by the file name, so a file only goes there when
/// its content is one of the formats Rust has no decoder for.
pub(super) fn native_only(path: &Path, bytes: &[u8]) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let brand = (bytes.get(4..8) == Some(b"ftyp")).then(|| bytes.get(8..12)).flatten();
    match ext.as_str() {
        "tif" | "tiff" => bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*"),
        "heic" | "heif" => brand.is_some_and(|b| {
            [b"heic", b"heix", b"hevc", b"hevx", b"mif1", b"msf1"].contains(&b.try_into().unwrap_or(&[0; 4]))
        }),
        "avif" => brand.is_some_and(|b| b == b"avif" || b == b"avis"),
        "pdf" => bytes.starts_with(b"%PDF"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_formats_without_a_rust_decoder_reach_the_system() {
        let tiff = b"II*\0rest";
        let heic = b"\0\0\0\x18ftypheic\0\0";
        assert!(native_only(Path::new("a.TIFF"), tiff));
        assert!(native_only(Path::new("a.heic"), heic));
        assert!(native_only(Path::new("a.pdf"), b"%PDF-1.7"));
        // A PNG renamed to HEIC would reach the system's PNG decoder.
        assert!(!native_only(Path::new("a.heic"), b"\x89PNG\r\n\x1a\n"));
        assert!(!native_only(Path::new("a.png"), b"\x89PNG\r\n\x1a\n"));
        assert!(!native_only(Path::new("a.tif"), heic));
    }
}
