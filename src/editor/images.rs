use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use block2::RcBlock;
use dispatch2::{DispatchObject, DispatchRetained, DispatchSource};
use objc2::rc::{Retained, Weak};
use objc2::runtime::Bool;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSCompositingOperation, NSImage};
use objc2_foundation::{NSRect, NSSize};

use super::safe_image::{Decoded, MAX_FILE_BYTES, SHOWN_SIDE};
use super::{EditorController, OPEN_EDITORS};
use crate::util::{BusyVolumes, Lane, Stamp, answer_from, lane_of_volume, stamp, volume_of};

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
    Done(Done),
}

struct Done {
    found: Option<Rendered>,
    /// The file the image came from, which a search may have found below the document, and how it looked then.
    file: PathBuf,
    seen: Stamp,
    /// Decoded smaller than `SHOWN_SIDE` because memory was short.
    reduced: bool,
    bytes: usize,
}

#[derive(Default)]
pub struct ImageStore {
    files: HashMap<PathBuf, Slot>,
    dimmed: HashMap<String, Rendered>,
    /// Which failed image gets the next try.
    next_try: usize,
}

impl Drop for ImageStore {
    fn drop(&mut self) {
        for slot in self.files.values() {
            if let Slot::Done(done) = slot {
                count_decoded(-crate::util::to_isize(done.bytes));
            }
        }
    }
}

/// What became of reading an image file.
pub struct ImageFile {
    loaded: Option<Decoded>,
    file: PathBuf,
    seen: Stamp,
    side: u32,
}

/// Decoded pixels held by all open documents; attachments in the previews keep them alive as well.
static DECODED: AtomicUsize = AtomicUsize::new(0);
static LOW_MEMORY: AtomicBool = AtomicBool::new(false);

pub(super) fn count_decoded(bytes: isize) {
    if bytes >= 0 {
        DECODED.fetch_add(crate::util::to_usize(bytes), Ordering::Relaxed);
    } else {
        DECODED.fetch_sub(bytes.unsigned_abs(), Ordering::Relaxed);
    }
}

/// The longest side an image decodes to: smaller as pixels pile up, so a document with hundreds of photos stays
/// usable instead of driving the Mac into swapping.
fn side_for(decoded: usize, budget: usize, low_memory: bool) -> u32 {
    if low_memory || decoded >= budget {
        640
    } else if decoded >= budget / 2 {
        1280
    } else {
        SHOWN_SIDE
    }
}

pub(super) fn allowed_side() -> u32 {
    static BUDGET: OnceLock<usize> = OnceLock::new();
    let budget = *BUDGET.get_or_init(|| (objc2_foundation::NSProcessInfo::processInfo().physicalMemory() / 8) as usize);
    side_for(
        DECODED.load(Ordering::Relaxed),
        budget,
        LOW_MEMORY.load(Ordering::Relaxed),
    )
}

thread_local! {
    static MEMORY_WATCH: Cell<Option<DispatchRetained<DispatchSource>>> = const { Cell::new(None) };
}

/// Follows the system's reports of memory pressure for the life of the app.
pub fn watch_memory(_mtm: MainThreadMarker) {
    const NORMAL: usize = 1;
    const WARN: usize = 2;
    const CRITICAL: usize = 4;
    extern "C" fn changed(_: *mut std::ffi::c_void) {
        MEMORY_WATCH.with(|watch| {
            if let Some(source) = watch.take() {
                memory_pressure_changed(source.data() & NORMAL == 0);
                watch.set(Some(source));
            }
        });
    }
    // SAFETY: The type is the system's constant, which takes no handle and these mask bits.
    let source = unsafe {
        DispatchSource::new(
            &raw const dispatch2::_dispatch_source_type_memorypressure as *mut _,
            0,
            NORMAL | WARN | CRITICAL,
            Some(dispatch2::DispatchQueue::main()),
        )
    };
    source.set_event_handler_f(changed);
    source.activate();
    MEMORY_WATCH.with(|watch| watch.set(Some(source)));
}

/// New images decode smaller while memory is short; caches and helpers that are not needed right now go.
pub fn memory_pressure_changed(low: bool) {
    LOW_MEMORY.store(low, Ordering::Relaxed);
    if low {
        super::decoder::release_idle();
        super::js::trim();
        super::remote::forget_unused();
    }
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
    let raw = url
        .strip_prefix("file://")
        .map_or(url, |r| r.strip_prefix("localhost").unwrap_or(r));
    let without_suffix = super::links::strip_suffixes(raw);
    let found = local_path(raw, doc_dir);
    // The main thread asks this during layout, and only the start-up disk is sure to answer.
    if without_suffix.len() == raw.len() || found.as_ref().is_some_and(|p| volume_of(p) == "/" && p.exists()) {
        return found;
    }
    // `![](a.png?raw=true)` as GitHub reads it, unless a file is really called that.
    local_path(without_suffix, doc_dir)
}

fn local_path(raw: &str, doc_dir: Option<&Path>) -> Option<PathBuf> {
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
            // Paper gets the full size, also of an image the preview shows smaller.
            Some(Slot::Done(done)) if requester.is_some() || !done.reduced => return done.found.clone(),
            // Paper does not wait for a load under way: it prints once.
            Some(Slot::Loading) if requester.is_some() => return None,
            _ => {}
        }
        let Some(requester) = requester else {
            let (file, dir) = (path.clone(), doc_dir.map(Path::to_path_buf));
            let file = answer_from(&path, PAPER_WAIT, move || load(&file, dir.as_deref(), SHOWN_SIDE))?;
            self.finish(path.clone(), file);
            return match self.files.get(&path) {
                Some(Slot::Done(done)) => done.found.clone(),
                _ => None,
            };
        };
        self.files.insert(path.clone(), Slot::Loading);
        read_again(path, doc_dir, requester);
        None
    }

    /// The finished images that `look_at_images` compares with their files.
    fn watched(&self) -> Vec<(PathBuf, PathBuf, Stamp)> {
        self.files
            .iter()
            .filter_map(|(path, slot)| match slot {
                Slot::Done(done) => Some((path.clone(), done.file.clone(), done.seen)),
                Slot::Loading => None,
            })
            .collect()
    }

    /// One missing image, another one at every call: it may appear in a folder below the document, where only a search
    /// finds it, and one search at a time stays cheap.
    fn missing_in_turn(&mut self) -> Option<PathBuf> {
        let mut failed: Vec<&PathBuf> = self
            .files
            .iter()
            .filter(|(_, slot)| matches!(slot, Slot::Done(done) if done.seen.is_none()))
            .map(|(path, _)| path)
            .collect();
        failed.sort();
        let turn = self.next_try;
        self.next_try = turn.wrapping_add(1);
        (!failed.is_empty()).then(|| failed[turn % failed.len()].clone())
    }

    /// Remembers how the file looks now, so one change starts one reading however long that takes.
    fn note_seen(&mut self, path: &Path, seen: Stamp) {
        if let Some(Slot::Done(done)) = self.files.get_mut(path) {
            done.seen = seen;
        }
    }

    /// Whether the image at `path` loaded and could be shown; `None` while it is not done.
    #[cfg(feature = "selftest")]
    pub fn shown(&self, path: &Path) -> Option<bool> {
        match self.files.get(path)? {
            Slot::Done(done) => Some(done.found.is_some()),
            Slot::Loading => None,
        }
    }

    /// The size of the image at `path`, once it is shown.
    #[cfg(feature = "selftest")]
    pub fn shown_size(&self, path: &Path) -> Option<NSSize> {
        match self.files.get(path)? {
            Slot::Done(done) => done.found.as_ref().map(|r| r.image.size()),
            Slot::Loading => None,
        }
    }

    pub fn finish(&mut self, path: PathBuf, file: ImageFile) {
        let bytes = match &file.loaded {
            Some(Decoded::Rgba { pixels, .. }) => pixels.len(),
            _ => 0,
        };
        count_decoded(crate::util::to_isize(bytes));
        let done = Done {
            found: file.loaded.and_then(rendered),
            file: file.file,
            seen: file.seen,
            reduced: file.side < SHOWN_SIDE,
            bytes,
        };
        self.dimmed.remove(path.to_string_lossy().as_ref());
        if let Some(Slot::Done(old)) = self.files.insert(path, Slot::Done(done)) {
            count_decoded(-crate::util::to_isize(old.bytes));
        }
    }
}

/// Reads the image in the background; whoever shows it hears when it arrives, and until then it stays as it is.
fn read_again(path: PathBuf, doc_dir: Option<&Path>, requester: &EditorController) {
    let me = dispatch2::MainThreadBound::new(Weak::from(requester), requester.mtm());
    let doc_dir = doc_dir.map(Path::to_path_buf);
    let volume = volume_of(&path);
    let job = move || {
        let file = load(&path, doc_dir.as_deref(), allowed_side());
        dispatch2::DispatchQueue::main().exec_async(move || {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            if let Some(c) = me.get(mtm).load() {
                c.image_loaded(path, file);
            }
        });
    };
    // An image on a volume that may stop answering must not take up a place that other images wait for.
    if volume == "/" {
        load_in_background(job);
    } else {
        lane_of_volume(&volume).run(job);
    }
}

/// Every this many looks at the images, one that is missing is searched for again.
const LOOKS_PER_TRY: u32 = 15;
static LOOKING: BusyVolumes = BusyVolumes::new();

thread_local! {
    static LOOKS: Cell<u32> = const { Cell::new(0) };
}

/// Compares the images of open previews with their files, so one that was replaced, or one that appeared only after
/// the document named it, shows without opening the document again.
pub fn look_at_images(mtm: MainThreadMarker) {
    super::remote::try_failed_again();
    let look = LOOKS.with(|n| n.replace(n.get().wrapping_add(1)));
    let editors: Vec<Retained<EditorController>> =
        OPEN_EDITORS.with(|e| e.borrow().iter().filter_map(Weak::load).collect());
    let mut by_volume: BTreeMap<String, Vec<LookAtEditor>> = BTreeMap::new();
    for e in editors.iter().filter(|e| e.preview_visible()) {
        let Ok(mut store) = e.ivars().images.try_borrow_mut() else {
            continue;
        };
        let retry = if look % LOOKS_PER_TRY == LOOKS_PER_TRY - 1 {
            store.missing_in_turn()
        } else {
            None
        };
        let mut watched_on: BTreeMap<String, Vec<(PathBuf, PathBuf, Stamp)>> = BTreeMap::new();
        for watched in store.watched() {
            watched_on.entry(volume_of(&watched.1)).or_default().push(watched);
        }
        for (volume, watched) in watched_on {
            by_volume.entry(volume).or_default().push(LookAtEditor {
                editor: dispatch2::MainThreadBound::new(Weak::from(&**e), mtm),
                watched,
                retry: retry.clone(),
            });
        }
    }
    for (volume, requests) in by_volume {
        let Some(on_volume) = LOOKING.enter(&volume) else {
            continue;
        };
        lane_of_volume(&volume).run(move || {
            let changed: Vec<_> = requests
                .into_iter()
                .map(|request| {
                    let changed: Vec<(PathBuf, Stamp)> = request
                        .watched
                        .into_iter()
                        .filter_map(|(path, file, seen)| {
                            let now = stamp(&file);
                            (now != seen || Some(&path) == request.retry.as_ref()).then_some((path, now))
                        })
                        .collect();
                    (request.editor, changed)
                })
                .collect();
            dispatch2::DispatchQueue::main().exec_async(move || {
                drop(on_volume);
                let Some(mtm) = MainThreadMarker::new() else {
                    return;
                };
                for (editor, changed) in changed {
                    if let Some(editor) = editor.get(mtm).load() {
                        editor.read_images_again(changed);
                    }
                }
            });
        });
    }
}

/// The images of one editor on one volume.
struct LookAtEditor {
    editor: dispatch2::MainThreadBound<Weak<EditorController>>,
    watched: Vec<(PathBuf, PathBuf, Stamp)>,
    retry: Option<PathBuf>,
}

impl EditorController {
    fn read_images_again(&self, changed: Vec<(PathBuf, Stamp)>) {
        let doc_dir = self.doc_dir();
        for (path, now) in changed {
            if let Ok(mut store) = self.ivars().images.try_borrow_mut() {
                store.note_seen(&path, now);
            }
            read_again(path, doc_dir.as_deref(), self);
        }
    }
}

/// Image files and downloads are decoded by as many threads as there are decoder helpers, so a document with hundreds
/// of pictures holds only a few of the files in memory at a time.
static LOADERS: Lane = Lane::new("splat-image").with_workers(super::decoder::HELPER_COUNT);

pub(super) fn load_in_background(job: impl FnOnce() + Send + 'static) {
    LOADERS.run(job);
}

fn rendered(loaded: Decoded) -> Option<Rendered> {
    let image = super::decoder::image(loaded)?;
    let size = image.size();
    Some(Rendered {
        image,
        sizing: Sizing::Points { size, descent: 0.0 },
    })
}

/// How long the main thread waits for a picture to print; printing goes on without one from a volume that hangs.
const PAPER_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

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
fn load(path: &Path, doc_dir: Option<&Path>, side: u32) -> ImageFile {
    let file = locate(path, doc_dir);
    let seen = stamp(&file);
    let loaded = seen.and_then(|_| {
        let bytes = crate::util::read_regular_file(&file, MAX_FILE_BYTES as u64)?;
        super::decoder::decode(&bytes, side).or_else(|| {
            native_only(&file, &bytes)
                .then(|| super::decoder::decode_native(&bytes, side))
                .flatten()
        })
    });
    ImageFile {
        loaded,
        file,
        seen,
        side,
    }
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
    fn a_picture_url_may_carry_what_github_adds_to_it() {
        let dir = Path::new("/docs");
        for url in ["a.png?raw=true", "a.png#gh-dark-mode-only", "a.png"] {
            assert_eq!(resolve_path(url, Some(dir)), Some(dir.join("a.png")), "{url}");
        }
        assert_eq!(
            resolve_path("file://localhost/docs/a.png", None),
            Some(PathBuf::from("/docs/a.png"))
        );
        assert_eq!(
            resolve_path("file:///docs/a.png", None),
            Some(PathBuf::from("/docs/a.png"))
        );
    }

    #[test]
    fn a_file_with_a_question_mark_in_its_name_stays_that_file() {
        let dir = std::env::temp_dir().join(format!("splat-mark-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("was?.png"), "").unwrap();
        assert_eq!(resolve_path("was?.png", Some(&dir)), Some(dir.join("was?.png")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn images_decode_smaller_as_pixels_pile_up() {
        let budget = 1 << 30;
        assert_eq!(side_for(0, budget, false), SHOWN_SIDE);
        assert_eq!(side_for(budget / 2, budget, false), 1280);
        assert_eq!(side_for(budget, budget, false), 640);
        assert_eq!(side_for(0, budget, true), 640);
    }

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
