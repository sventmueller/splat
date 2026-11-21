//! The image files documents show: read on a thread of their volume, decoded in the decoding service, held per
//! document and looked at again when their files change; and what memory pressure takes from all pictures.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use block2::RcBlock;
use dispatch2::{DispatchObject, DispatchRetained, DispatchSource};
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{MainThreadMarker, Message as _};
use objc2_app_kit::{NSCompositingOperation, NSImage};
use objc2_foundation::NSRect;
use splat_decoder::{Decoded, MAX_FILE_BYTES};
use splat_editor_state::links::{locate, strip_suffixes};
use splat_file_state::Stamp;
use splat_markdown::url;

use crate::appkit::to_isize;
use crate::files::{read_regular_file, stamp};
use crate::lanes::{guarded, main_soon};
pub use crate::picture::ImageFile;
use crate::picture::{
    Decode, Fit, Rendered, Wanted, allowed_side, count_decoded, loaders_of, rendered, room_for_more, set_low_memory,
};
use crate::viewer::{Messenger, Viewer};
use crate::volumes::{BusyVolumes, answer_from, lane_of_volume, volume_of};

enum ImageLoad {
    /// Read for the first time, for as many pixels as the views that asked for it need.
    Loading(Arc<Wanted>),
    Done(Done),
}

struct Done {
    found: Option<Rendered>,
    /// The file the image came from, which a search may have found below the document, and how it looked then.
    file: PathBuf,
    seen: Stamp,
    /// What the pixels shown were decoded for.
    decode: Option<Decode>,
    /// The longest side the load was allowed, which for one that failed says whether paper may try again.
    side: u32,
    bytes: usize,
    wanted: Arc<Wanted>,
    /// A decode with more pixels is under way.
    growing: bool,
}

#[derive(Default)]
pub struct ImageStore {
    files: HashMap<PathBuf, ImageLoad>,
    /// By the key of the picture they dim, with the picture they were made of.
    dimmed: HashMap<String, (Retained<NSImage>, Rendered)>,
    /// Which failed image gets the next try.
    next_try: usize,
}

impl Drop for ImageStore {
    fn drop(&mut self) {
        for slot in self.files.values() {
            if let ImageLoad::Done(done) = slot {
                count_decoded(-to_isize(done.bytes));
            }
        }
    }
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
        guarded((), || {
            MEMORY_WATCH.with(|watch| {
                if let Some(source) = watch.take() {
                    memory_pressure_changed(source.data() & NORMAL == 0);
                    watch.set(Some(source));
                }
            });
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
    set_low_memory(low);
    if low {
        splat_decoder::release_idle();
        crate::js::trim();
        crate::remote::forget_unused();
    }
}

/// Draws the image slightly transparent, so bright pictures do not glare on a dark background.
fn dimmed(image: &NSImage) -> Retained<NSImage> {
    let source = image.retain();
    let block = RcBlock::new(move |rect: NSRect| -> Bool {
        guarded(Bool::NO, || {
            source.drawInRect_fromRect_operation_fraction(rect, NSRect::ZERO, NSCompositingOperation::SourceOver, 0.82);
            Bool::YES
        })
    });
    NSImage::imageWithSize_flipped_drawingHandler(image.size(), false, &block)
}

pub fn resolve_path(url: &str, doc_dir: Option<&Path>) -> Option<PathBuf> {
    if url::is_web_or_data(url) {
        return None;
    }
    let raw = url::behind_file_scheme(url).map_or(url, |r| r.strip_prefix("localhost").unwrap_or(r));
    let without_suffix = strip_suffixes(raw);
    let found = local_path(raw, doc_dir).map(|path| without_detours(&path));
    // The main thread asks this during layout, and only the start-up disk is sure to answer.
    if without_suffix.len() == raw.len() || found.as_ref().is_some_and(|p| volume_of(p) == "/" && p.exists()) {
        return found;
    }
    // `![](a.png?raw=true)` as GitHub reads it, unless a file is really called that.
    local_path(without_suffix, doc_dir).map(|path| without_detours(&path))
}

/// The path without what leads back into itself, so that each way of writing it does not decode a picture of its own.
fn without_detours(path: &Path) -> PathBuf {
    let mut clean = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir if clean.file_name().is_some() => {
                clean.pop();
            }
            Component::ParentDir if clean.has_root() => {}
            Component::CurDir => {}
            part => clean.push(part),
        }
    }
    clean
}

fn local_path(raw: &str, doc_dir: Option<&Path>) -> Option<PathBuf> {
    let decoded = url::percent_decode(raw);
    let path = PathBuf::from(&decoded);
    if path.is_absolute() {
        return Some(path);
    }
    if let Some(stripped) = decoded.strip_prefix("~/")
        && let Some(home) = url::home()
    {
        return Some(home.join(stripped));
    }
    doc_dir.map(|d| d.join(path))
}

impl ImageStore {
    /// The image a document refers to, with the pixels `need` asks for. With a `requester`, an image not yet loaded, or
    /// shown with fewer pixels, loads in the background and the requester hears when it arrives; until then it shows as
    /// it is. Without one, it loads right away, as printing needs, and stays out of the store.
    pub fn file(
        &mut self,
        url: &str,
        doc_dir: Option<&Path>,
        dark: bool,
        requester: Option<&Rc<dyn Viewer>>,
        need: Fit,
    ) -> Option<Rendered> {
        let original = self.original(url, doc_dir, requester, need)?;
        let key = resolve_path(url, doc_dir)?.to_string_lossy().into_owned();
        Some(self.for_appearance(&key, original, dark))
    }

    /// The image as shown on the current appearance: dimmed variants are cached by `key`, as long as the picture under
    /// that key stays the same.
    pub fn for_appearance(&mut self, key: &str, original: Rendered, dark: bool) -> Rendered {
        if !dark {
            return original;
        }
        if let Some((of, dimmed)) = self.dimmed.get(key)
            && std::ptr::eq::<NSImage>(&**of, &*original.image)
        {
            return dimmed.clone();
        }
        let made = Rendered {
            image: dimmed(&original.image),
            sizing: original.sizing,
        };
        self.dimmed.insert(key.to_string(), (original.image, made.clone()));
        made
    }

    fn original(
        &mut self,
        url: &str,
        doc_dir: Option<&Path>,
        requester: Option<&Rc<dyn Viewer>>,
        need: Fit,
    ) -> Option<Rendered> {
        let path = resolve_path(url, doc_dir)?;
        let Some(requester) = requester else {
            return self.on_paper(path, doc_dir, need);
        };
        match self.files.get_mut(&path) {
            Some(ImageLoad::Done(done)) => {
                if done.found.is_some() && done.decode.is_some_and(|decode| !decode.serves(need)) {
                    done.wanted.raise(need);
                    // While memory is short a larger decode would only be refused, and the refusal shown again.
                    if !done.growing && room_for_more() {
                        done.growing = true;
                        read_in_background(path, doc_dir, requester, done.wanted.clone(), true);
                    }
                }
                return done.found.clone();
            }
            Some(ImageLoad::Loading(wanted)) => {
                wanted.raise(need);
                return None;
            }
            None => {}
        }
        let wanted = Wanted::of(need);
        self.files.insert(path.clone(), ImageLoad::Loading(wanted.clone()));
        read_in_background(path, doc_dir, requester, wanted, false);
        None
    }

    /// The image for paper: the preview's if it has the pixels `need` asks for, else read for paper alone, which does not
    /// wait for a load under way either, as it prints once.
    fn on_paper(&self, path: PathBuf, doc_dir: Option<&Path>, need: Fit) -> Option<Rendered> {
        if let Some(ImageLoad::Done(done)) = self.files.get(&path) {
            match done.decode {
                Some(decode) if decode.serves(need) => return done.found.clone(),
                None if done.side >= need.side => return None,
                _ => {}
            }
        }
        let (file, dir) = (path.clone(), doc_dir.map(Path::to_path_buf));
        let file = answer_from(&path, PAPER_WAIT, move || load(&file, dir.as_deref(), || need))?;
        file.loaded.and_then(rendered)
    }

    /// Reads the image at `path` again, a file that changed, for the pixels its views asked for.
    pub fn read_again(&self, path: PathBuf, doc_dir: Option<&Path>, requester: &Rc<dyn Viewer>) {
        let wanted = match self.files.get(&path) {
            Some(ImageLoad::Done(done)) => done.wanted.clone(),
            Some(ImageLoad::Loading(wanted)) => wanted.clone(),
            None => Wanted::of(Fit::within(allowed_side())),
        };
        read_in_background(path, doc_dir, requester, wanted, false);
    }

    /// Lets go of what a document stopped showing, as an image taken out of the text: its pixels would stay for as long as
    /// the window is open. One still loading stays, as it reports to this store.
    pub fn keep_only(&mut self, shown: &std::collections::HashSet<PathBuf>) {
        self.files.retain(|path, slot| match slot {
            ImageLoad::Loading(_) => true,
            ImageLoad::Done(done) => {
                let keep = shown.contains(path);
                if !keep {
                    count_decoded(-to_isize(done.bytes));
                }
                keep
            }
        });
        self.dimmed.retain(|path, _| shown.contains(Path::new(path)));
    }

    /// The finished images that `look_at_images` compares with their files.
    fn watched(&self) -> Vec<(PathBuf, PathBuf, Stamp)> {
        self.files
            .iter()
            .filter_map(|(path, slot)| match slot {
                ImageLoad::Done(done) => Some((path.clone(), done.file.clone(), done.seen)),
                ImageLoad::Loading(_) => None,
            })
            .collect()
    }

    /// One missing image, another one at every call: it may appear in a folder below the document, where only a search
    /// finds it, and one search at a time stays cheap.
    fn missing_in_turn(&mut self) -> Option<PathBuf> {
        let mut failed: Vec<&PathBuf> = self
            .files
            .iter()
            .filter(|(_, slot)| matches!(slot, ImageLoad::Done(done) if !done.seen.is_reached()))
            .map(|(path, _)| path)
            .collect();
        failed.sort();
        let turn = self.next_try;
        self.next_try = turn.wrapping_add(1);
        (!failed.is_empty()).then(|| failed[turn % failed.len()].clone())
    }

    /// Remembers how the file looks now, so one change starts one reading however long that takes.
    pub fn note_seen(&mut self, path: &Path, seen: Stamp) {
        if let Some(ImageLoad::Done(done)) = self.files.get_mut(path) {
            done.seen = seen;
        }
    }

    /// How many images are shown, and whether one is still being read.
    #[cfg(feature = "selftest")]
    pub fn shown_and_loading(&self) -> (usize, bool) {
        let shown = self
            .files
            .values()
            .filter(|slot| matches!(slot, ImageLoad::Done(done) if done.found.is_some()))
            .count();
        (
            shown,
            self.files.values().any(|slot| matches!(slot, ImageLoad::Loading(_))),
        )
    }

    /// Whether the image at `path` loaded and could be shown; `None` while it is not done.
    #[cfg(feature = "selftest")]
    pub fn shown(&self, path: &Path) -> Option<bool> {
        match self.files.get(path)? {
            ImageLoad::Done(done) => Some(done.found.is_some()),
            ImageLoad::Loading(_) => None,
        }
    }

    /// The size of the image at `path`, once it is shown.
    #[cfg(feature = "selftest")]
    pub fn shown_size(&self, path: &Path) -> Option<objc2_foundation::NSSize> {
        match self.files.get(path)? {
            ImageLoad::Done(done) => done.found.as_ref().map(|r| r.image.size()),
            ImageLoad::Loading(_) => None,
        }
    }

    /// The pixels the image at `path` was decoded to, once it is shown, and whether more are on their way.
    #[cfg(feature = "selftest")]
    pub fn decoded(&self, path: &Path) -> Option<((u32, u32), bool)> {
        match self.files.get(path)? {
            ImageLoad::Done(done) => Some((done.decode?.pixels, done.growing)),
            ImageLoad::Loading(_) => None,
        }
    }

    /// Whether it holds no image at all, loaded or loading.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Takes what a load brought; whether the picture shown changed.
    pub fn finish(&mut self, path: PathBuf, file: ImageFile) -> bool {
        let (wanted, growing) = match self.files.get_mut(&path) {
            Some(ImageLoad::Done(done)) if file.grows && file.loaded.is_none() => {
                done.growing = false;
                return false;
            }
            Some(ImageLoad::Done(done)) => (done.wanted.clone(), done.growing && !file.grows),
            Some(ImageLoad::Loading(wanted)) => (wanted.clone(), false),
            // What stopped showing while it grew has no use for more pixels.
            None if file.grows => return false,
            None => (Wanted::of(file.fit), false),
        };
        let bytes = match &file.loaded {
            Some(Decoded::Rgba { pixels, .. }) => pixels.len(),
            _ => 0,
        };
        count_decoded(to_isize(bytes));
        let decode = file.loaded.as_ref().and_then(|loaded| Decode::of(loaded, file.fit));
        let done = Done {
            found: file.loaded.and_then(rendered),
            file: file.file,
            seen: file.seen,
            decode,
            side: file.fit.side,
            bytes,
            wanted,
            growing,
        };
        self.dimmed.remove(path.to_string_lossy().as_ref());
        if let Some(ImageLoad::Done(old)) = self.files.insert(path, ImageLoad::Done(done)) {
            count_decoded(-to_isize(old.bytes));
        }
        true
    }
}

/// Reads the image in the background, for as many pixels as `wanted` holds when its decode begins; whoever shows it hears
/// when it arrives, and until then it stays as it is.
fn read_in_background(
    path: PathBuf,
    doc_dir: Option<&Path>,
    requester: &Rc<dyn Viewer>,
    wanted: Arc<Wanted>,
    grows: bool,
) {
    let requester = Messenger::of(requester);
    let doc_dir = doc_dir.map(Path::to_path_buf);
    let asked = path.clone();
    loaders_of(&volume_of(&path)).ask(
        move || {
            let file = if room_for_more() {
                load(&asked, doc_dir.as_deref(), || wanted.fit(allowed_side()))
            } else {
                ImageFile::failed(asked)
            };
            ImageFile { grows, ..file }
        },
        move |file| {
            let file = file.unwrap_or_else(|| ImageFile {
                grows,
                ..ImageFile::failed(path.clone())
            });
            requester.send_soon(move |viewer| viewer.image_loaded(path, file));
        },
    );
}

/// Every this many looks at the images, one that is missing is searched for again.
const LOOKS_PER_TRY: u32 = 15;
static LOOKING: BusyVolumes = BusyVolumes::new();

thread_local! {
    static LOOKS: Cell<u32> = const { Cell::new(0) };
}

/// Compares the images of the open previews, each a viewer with its store, with their files, so one that was replaced,
/// or one that appeared only after the document named it, shows without opening the document again.
pub fn look_at_images<'a>(open: impl IntoIterator<Item = (Rc<dyn Viewer>, &'a RefCell<ImageStore>)>) {
    crate::remote::try_failed_again();
    let look = LOOKS.with(|n| n.replace(n.get().wrapping_add(1)));
    let mut by_volume: BTreeMap<String, Vec<LookAtEditor>> = BTreeMap::new();
    for (e, store) in open {
        let mut store = store.borrow_mut();
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
                editor: Messenger::of(&e),
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
            main_soon(move || drop(on_volume));
            for (editor, changed) in changed {
                editor.send_soon(move |viewer| viewer.images_changed(changed));
            }
        });
    }
}

/// The images of one editor on one volume.
struct LookAtEditor {
    editor: Messenger,
    watched: Vec<(PathBuf, PathBuf, Stamp)>,
    retry: Option<PathBuf>,
}

/// How long the main thread waits for a picture to print; printing goes on without one from a volume that hangs.
const PAPER_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Reads and decodes an image file in a decoder helper, for the pixels `fit` gives once the file is read; only formats
/// Rust cannot decode go to the system.
fn load(path: &Path, doc_dir: Option<&Path>, fit: impl FnOnce() -> Fit) -> ImageFile {
    let file = locate(path, doc_dir);
    let seen = stamp(&file);
    let bytes = seen
        .is_reached()
        .then(|| read_regular_file(&file, MAX_FILE_BYTES as u64))
        .flatten();
    let fit = fit();
    let loaded = bytes.and_then(|bytes| {
        splat_decoder::decode_to(&bytes, fit).or_else(|| {
            native_only(&file, &bytes)
                .then(|| splat_decoder::decode_native_to(&bytes, fit))
                .flatten()
        })
    });
    ImageFile {
        loaded,
        file,
        seen,
        fit,
        grows: false,
    }
}

/// The system decoder recognises formats by their content, not by the file name, so a file only goes there when
/// its content is one of the formats Rust has no decoder for.
pub fn native_only(path: &Path, bytes: &[u8]) -> bool {
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
    fn each_way_of_writing_a_path_is_the_same_picture() {
        let dir = Some(Path::new("/docs/text"));
        let plain = resolve_path("../d/a.png", dir);
        assert_eq!(plain, Some(PathBuf::from("/docs/d/a.png")));
        assert_eq!(resolve_path("../d/../d/./a.png", dir), plain);
        assert_eq!(resolve_path("/docs/text/../d/a.png", None), plain);
        assert_eq!(resolve_path("../../../../a.png", dir), Some(PathBuf::from("/a.png")));
        assert_eq!(resolve_path("../a.png", None), None);
    }

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
            resolve_path("FILE:///docs/a.png", None),
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
