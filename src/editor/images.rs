use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};

use block2::RcBlock;
use dispatch2::{DispatchObject, DispatchRetained, DispatchSource};
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{MainThreadMarker, MainThreadOnly, Message as _, sel};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSCompositingOperation, NSImage, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::EditorController;
use super::decoder::{Decoded, MAX_FILE_BYTES, SHOWN_SIDE};
use super::layers::{ForPictures, Message, Messenger};
use super::links::locate;
use super::picture::{LOADERS, Rendered, Sizing, allowed_side, count_decoded, room_for_more, set_low_memory};
use crate::i18n::t;
use crate::markdown::url;
use crate::util::{BusyVolumes, Stamp, answer_from, lane_of_volume, ns, stamp, volume_of};

/// Height of the bar above the preview that offers to grant the document's folder.
const BANNER_HEIGHT: f64 = 48.0;

enum ImageLoad {
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
    files: HashMap<PathBuf, ImageLoad>,
    dimmed: HashMap<String, Rendered>,
    /// Which failed image gets the next try.
    next_try: usize,
}

impl Drop for ImageStore {
    fn drop(&mut self) {
        for slot in self.files.values() {
            if let ImageLoad::Done(done) = slot {
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

impl ImageFile {
    /// What a load that ended in a panic leaves: nothing to show, and no reason to wait for it any longer.
    fn failed(file: PathBuf) -> ImageFile {
        ImageFile {
            loaded: None,
            file,
            seen: Stamp::UNREACHABLE,
            side: allowed_side(),
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
        crate::util::guarded((), || {
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
        super::decoder::release_idle();
        super::js::trim();
        super::remote::forget_unused();
    }
}

/// Draws the image slightly transparent, so bright pictures do not glare on a dark background.
fn dimmed(image: &NSImage) -> Retained<NSImage> {
    let source = image.retain();
    let block = RcBlock::new(move |rect: NSRect| -> Bool {
        crate::util::guarded(Bool::NO, || {
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
    let without_suffix = super::links::strip_suffixes(raw);
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
    let decoded = crate::markdown::url::percent_decode(raw);
    let path = PathBuf::from(&decoded);
    if path.is_absolute() {
        return Some(path);
    }
    if let Some(stripped) = decoded.strip_prefix("~/")
        && let Some(home) = crate::markdown::url::home()
    {
        return Some(home.join(stripped));
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
            Some(ImageLoad::Done(done)) if requester.is_some() || !done.reduced => return done.found.clone(),
            // Paper does not wait for a load under way: it prints once.
            Some(ImageLoad::Loading) if requester.is_some() => return None,
            _ => {}
        }
        let Some(requester) = requester else {
            let (file, dir) = (path.clone(), doc_dir.map(Path::to_path_buf));
            let file = answer_from(&path, PAPER_WAIT, move || load(&file, dir.as_deref(), SHOWN_SIDE))?;
            self.finish(path.clone(), file);
            return match self.files.get(&path) {
                Some(ImageLoad::Done(done)) => done.found.clone(),
                _ => None,
            };
        };
        self.files.insert(path.clone(), ImageLoad::Loading);
        read_again(path, doc_dir, Messenger::of(requester));
        None
    }

    /// Lets go of what a document stopped showing, as an image taken out of the text: its pixels would stay for as long as
    /// the window is open. One still loading stays, as it reports to this store.
    fn keep_only(&mut self, shown: &std::collections::HashSet<PathBuf>) {
        self.files.retain(|path, slot| match slot {
            ImageLoad::Loading => true,
            ImageLoad::Done(done) => {
                let keep = shown.contains(path);
                if !keep {
                    count_decoded(-crate::util::to_isize(done.bytes));
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
                ImageLoad::Loading => None,
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
    fn note_seen(&mut self, path: &Path, seen: Stamp) {
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
            self.files.values().any(|slot| matches!(slot, ImageLoad::Loading)),
        )
    }

    /// Whether the image at `path` loaded and could be shown; `None` while it is not done.
    #[cfg(feature = "selftest")]
    pub fn shown(&self, path: &Path) -> Option<bool> {
        match self.files.get(path)? {
            ImageLoad::Done(done) => Some(done.found.is_some()),
            ImageLoad::Loading => None,
        }
    }

    /// The size of the image at `path`, once it is shown.
    #[cfg(feature = "selftest")]
    pub fn shown_size(&self, path: &Path) -> Option<NSSize> {
        match self.files.get(path)? {
            ImageLoad::Done(done) => done.found.as_ref().map(|r| r.image.size()),
            ImageLoad::Loading => None,
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
        if let Some(ImageLoad::Done(old)) = self.files.insert(path, ImageLoad::Done(done)) {
            count_decoded(-crate::util::to_isize(old.bytes));
        }
    }
}

/// Reads the image in the background; whoever shows it hears when it arrives, and until then it stays as it is.
fn read_again(path: PathBuf, doc_dir: Option<&Path>, requester: Messenger) {
    let doc_dir = doc_dir.map(Path::to_path_buf);
    let volume = volume_of(&path);
    // An image on a volume that may stop answering must not take up a place that other images wait for.
    let lane = if volume == "/" {
        &LOADERS
    } else {
        lane_of_volume(&volume)
    };
    let asked = path.clone();
    lane.ask(
        move || {
            if room_for_more() {
                load(&asked, doc_dir.as_deref(), allowed_side())
            } else {
                ImageFile::failed(asked)
            }
        },
        move |file| {
            let file = file.unwrap_or_else(|| ImageFile::failed(path.clone()));
            requester.send_soon(Message::ImageLoaded(path, file));
        },
    );
}

/// Every this many looks at the images, one that is missing is searched for again.
const LOOKS_PER_TRY: u32 = 15;
static LOOKING: BusyVolumes = BusyVolumes::new();

thread_local! {
    static LOOKS: Cell<u32> = const { Cell::new(0) };
}

/// Compares the images of open previews with their files, so one that was replaced, or one that appeared only after
/// the document named it, shows without opening the document again.
pub fn look_at_images(_mtm: MainThreadMarker) {
    super::remote::try_failed_again();
    let look = LOOKS.with(|n| n.replace(n.get().wrapping_add(1)));
    let editors = super::open_editors();
    let mut by_volume: BTreeMap<String, Vec<LookAtEditor>> = BTreeMap::new();
    for (e, store) in editors.iter().filter_map(|e| Some((e, e.images_shown()?))) {
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
                editor: Messenger::of(e),
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
            crate::util::main_soon(move || drop(on_volume));
            for (editor, changed) in changed {
                editor.send_soon(Message::ImagesChanged(changed));
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

#[derive(Default)]
pub(super) struct PictureState {
    images: RefCell<ImageStore>,
    /// The bar above the preview, which offers to grant the document's folder.
    banner: OnceCell<Retained<NSView>>,
    banner_parts: OnceCell<(Retained<objc2_app_kit::NSTextField>, Retained<objc2_app_kit::NSButton>)>,
}

/// What the bar above the preview offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Offer {
    /// The sandbox keeps the folder around the document closed, and the document names files in it.
    GrantFolder,
}

impl Offer {
    fn wording(self) -> (&'static str, &'static str, objc2::runtime::Sel) {
        match self {
            Offer::GrantFolder => (
                t(
                    "Bilder, Notizen und Git-Daten neben diesem Dokument kann Splat erst lesen, wenn du ihren Ordner freigibst.",
                ),
                t("Ordner freigeben …"),
                sel!(mdGrantFolder:),
            ),
        }
    }
}

pub(super) struct Pictures<'a> {
    host: ForPictures<'a>,
    state: &'a PictureState,
}

impl<'a> Pictures<'a> {
    pub(super) fn new(host: ForPictures<'a>, state: &'a PictureState) -> Self {
        Pictures { host, state }
    }
}

#[cfg(feature = "selftest")]
impl Pictures<'_> {
    pub(super) fn banner_shown(&self) -> Option<bool> {
        self.state.banner.get().map(|b| !b.isHidden())
    }

    pub(super) fn banner_offers(&self) -> Option<String> {
        let shown = self.state.banner.get().is_some_and(|b| !b.isHidden());
        let (_, button) = self.state.banner_parts.get()?;
        shown.then(|| button.title().to_string())
    }
}

impl<'a> Pictures<'a> {
    pub(super) fn forget_images_no_longer_shown(&self) {
        if self.state.images.borrow().files.is_empty() {
            return;
        }
        let doc_dir = self.host.doc_dir();
        let shown: std::collections::HashSet<PathBuf> = self.host.model().read(move |st| {
            st.layout()
                .blocks
                .iter()
                .flat_map(|block| super::state::images_of(block))
                .filter_map(|url| resolve_path(url, doc_dir.as_deref()))
                .collect()
        });
        self.state.images.borrow_mut().keep_only(&shown);
    }

    pub(super) fn store(&self) -> &'a RefCell<ImageStore> {
        &self.state.images
    }

    /// The bar is in the view hierarchy now.
    pub(super) fn install_banner(&self, banner: Retained<NSView>) {
        let _ = self.state.banner.set(banner);
    }

    pub(super) fn read_images_again(&self, changed: Vec<(PathBuf, Stamp)>) {
        let doc_dir = self.host.doc_dir();
        for (path, now) in changed {
            self.state.images.borrow_mut().note_seen(&path, now);
            read_again(path, doc_dir.as_deref(), self.host.object().messenger());
        }
    }

    pub(super) fn image_loaded(&self, path: PathBuf, file: ImageFile) {
        let key = path.to_string_lossy().into_owned();
        self.state.images.borrow_mut().finish(path, file);
        self.host.image_arrived(key);
    }

    pub(super) fn build_banner(&self, size: NSSize) -> Retained<NSView> {
        let mtm = self.host.mtm();
        let frame = NSRect::new(
            NSPoint::new(0.0, size.height - BANNER_HEIGHT),
            NSSize::new(size.width, BANNER_HEIGHT),
        );
        let banner = objc2_app_kit::NSBox::initWithFrame(objc2_app_kit::NSBox::alloc(mtm), frame);
        banner.setBoxType(objc2_app_kit::NSBoxType::Custom);
        banner.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
        banner.setBorderWidth(0.0);
        banner.setContentViewMargins(NSSize::new(0.0, 0.0));
        banner.setFillColor(&objc2_app_kit::NSColor::controlBackgroundColor());
        banner.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        banner.setHidden(true);
        let content = banner.contentView().expect("box content view");
        let (text, title, action) = Offer::GrantFolder.wording();
        let button = crate::util::button(mtm, title, self.host.object().target(), action);
        button.setControlSize(objc2_app_kit::NSControlSize::Small);
        button.setFont(Some(&objc2_app_kit::NSFont::systemFontOfSize(
            objc2_app_kit::NSFont::smallSystemFontSize(),
        )));
        button.sizeToFit();
        let b = button.frame().size;
        button.setFrameOrigin(NSPoint::new(
            size.width - b.width - 12.0,
            (BANNER_HEIGHT - b.height) / 2.0,
        ));
        button.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        content.addSubview(&button);
        let label = objc2_app_kit::NSTextField::wrappingLabelWithString(&ns(text), mtm);
        label.setFont(Some(&objc2_app_kit::NSFont::systemFontOfSize(
            objc2_app_kit::NSFont::smallSystemFontSize(),
        )));
        label.setTextColor(Some(&objc2_app_kit::NSColor::secondaryLabelColor()));
        label.setMaximumNumberOfLines(2);
        label.setFrame(NSRect::new(
            NSPoint::new(12.0, 4.0),
            NSSize::new((size.width - b.width - 36.0).max(40.0), BANNER_HEIGHT - 8.0),
        ));
        label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        content.addSubview(&label);
        let _ = self.state.banner_parts.set((label.clone(), button.clone()));
        let line = objc2_app_kit::NSBox::initWithFrame(
            objc2_app_kit::NSBox::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size.width, 1.0)),
        );
        line.setBoxType(objc2_app_kit::NSBoxType::Separator);
        line.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        content.addSubview(&line);
        Retained::into_super(banner)
    }

    /// Shows the bar with what it offers, or hides it, and gives the preview the room that is left.
    pub(super) fn show_banner(&self, offer: Option<Offer>) {
        let Some(banner) = self.state.banner.get() else {
            return;
        };
        if let (Some(offer), Some((label, button))) = (offer, self.state.banner_parts.get()) {
            let (text, title, action) = offer.wording();
            label.setStringValue(&ns(text));
            button.setTitle(&ns(title));
            crate::util::set_action(button, self.host.object().target(), action);
            button.sizeToFit();
            let width = banner.frame().size.width;
            let b = button.frame().size;
            button.setFrameOrigin(NSPoint::new(width - b.width - 12.0, (BANNER_HEIGHT - b.height) / 2.0));
            label.setFrameSize(NSSize::new((width - b.width - 36.0).max(40.0), BANNER_HEIGHT - 8.0));
        }
        let show = offer.is_some();
        if banner.isHidden() != show {
            return;
        }
        banner.setHidden(!show);
        self.host
            .panes()
            .leave_room_above_preview(if show { BANNER_HEIGHT } else { 0.0 });
    }

    /// The images of files, read again: a folder granted just now holds those that failed before.
    pub(super) fn read_all_again(&self) {
        self.state
            .images
            .borrow_mut()
            .keep_only(&std::collections::HashSet::new());
    }
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

/// Reads and decodes an image file in a decoder helper; only formats Rust cannot decode go to the system.
fn load(path: &Path, doc_dir: Option<&Path>, side: u32) -> ImageFile {
    let file = locate(path, doc_dir);
    let seen = stamp(&file);
    let loaded = seen
        .is_reached()
        .then(|| {
            let bytes = crate::util::read_regular_file(&file, MAX_FILE_BYTES as u64)?;
            super::decoder::decode(&bytes, side).or_else(|| {
                native_only(&file, &bytes)
                    .then(|| super::decoder::decode_native(&bytes, side))
                    .flatten()
            })
        })
        .flatten();
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
