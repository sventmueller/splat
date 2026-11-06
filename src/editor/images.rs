use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};

use block2::RcBlock;
use dispatch2::{DispatchObject, DispatchRetained, DispatchSource};
use objc2::rc::{Retained, Weak};
use objc2::runtime::Bool;
use objc2::{MainThreadMarker, MainThreadOnly, Message, sel};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSCompositingOperation, NSImage, NSView};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSUserDefaults};

use super::layers::ForPictures;
use super::picture::{LOADERS, Rendered, Sizing, allowed_side, count_decoded, room_for_more, set_low_memory};
use super::safe_image::{Decoded, MAX_FILE_BYTES, SHOWN_SIDE};
use super::{EditorController, OPEN_EDITORS};
use crate::i18n::t;
use crate::markdown::url;
use crate::util::{BusyVolumes, Stamp, answer_from, lane_of_volume, ns, stamp, volume_of};

/// Height of the bar above the preview that offers to load images from the web.
const BANNER_HEIGHT: f64 = 48.0;
/// User defaults key listing the documents whose images from the web load without asking.
pub(super) const ALLOWED_KEY: &str = "RemoteImagesAllowed";
const MAX_ALLOWED_DOCUMENTS: usize = 500;

pub(super) fn allowed_documents() -> Vec<String> {
    NSUserDefaults::standardUserDefaults()
        .stringArrayForKey(&ns(ALLOWED_KEY))
        .map(|a| a.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

/// The list of allowed documents once `path` is one of them: the last, once, and only the newest are kept.
fn allowing(mut paths: Vec<String>, path: String) -> Vec<String> {
    paths.retain(|p| *p != path);
    paths.push(path);
    let keep = paths.len().saturating_sub(MAX_ALLOWED_DOCUMENTS);
    paths.split_off(keep)
}

/// A document without a file cannot have been allowed.
fn is_allowed(paths: &[String], path: Option<&str>) -> bool {
    path.is_some_and(|path| paths.iter().any(|p| p == path))
}

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
        source.drawInRect_fromRect_operation_fraction(rect, NSRect::ZERO, NSCompositingOperation::SourceOver, 0.82);
        Bool::YES
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
        read_again(path, doc_dir, requester);
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
fn read_again(path: PathBuf, doc_dir: Option<&Path>, requester: &EditorController) {
    let me = dispatch2::MainThreadBound::new(Weak::from(requester), requester.mtm());
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
            crate::util::main_soon(move || {
                let Some(mtm) = MainThreadMarker::new() else {
                    return;
                };
                if let Some(c) = me.get(mtm).load() {
                    c.image_loaded(path, file);
                }
            });
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
pub fn look_at_images(mtm: MainThreadMarker) {
    super::remote::try_failed_again();
    let look = LOOKS.with(|n| n.replace(n.get().wrapping_add(1)));
    let editors: Vec<Retained<EditorController>> =
        OPEN_EDITORS.with(|e| e.borrow().iter().filter_map(Weak::load).collect());
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
            crate::util::main_soon(move || {
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

#[derive(Default)]
pub(super) struct PictureState {
    images: RefCell<ImageStore>,
    remote_allowed: Cell<bool>,
    /// The bar above the preview offering to load images from the web.
    remote_banner: OnceCell<Retained<NSView>>,
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
        self.state.remote_banner.get().map(|b| !b.isHidden())
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

    pub(super) fn remote_allowed(&self) -> bool {
        self.state.remote_allowed.get()
    }

    /// The bar is in the view hierarchy now; a document allowed earlier keeps its permission from here on.
    pub(super) fn install_banner(&self, banner: Retained<NSView>) {
        let _ = self.state.remote_banner.set(banner);
        let allowed = is_allowed(&allowed_documents(), self.document_path().as_deref());
        self.state.remote_allowed.set(allowed);
    }

    pub(super) fn read_images_again(&self, changed: Vec<(PathBuf, Stamp)>) {
        let doc_dir = self.host.doc_dir();
        for (path, now) in changed {
            self.state.images.borrow_mut().note_seen(&path, now);
            read_again(path, doc_dir.as_deref(), self.host.object());
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
        // SAFETY: the target is this controller, which owns the window the button sits in.
        let button = unsafe {
            objc2_app_kit::NSButton::buttonWithTitle_target_action(
                &ns(t("Bilder laden")),
                Some(self.host.object()),
                Some(sel!(mdLoadRemoteImages:)),
                mtm,
            )
        };
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
        let label = objc2_app_kit::NSTextField::wrappingLabelWithString(
            &ns(t(
                "Bilder aus dem Internet sind blockiert: Beim Laden erfahren deren Server, dass und wann du dieses Dokument öffnest.",
            )),
            mtm,
        );
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
        let line = objc2_app_kit::NSBox::initWithFrame(
            objc2_app_kit::NSBox::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size.width, 1.0)),
        );
        line.setBoxType(objc2_app_kit::NSBoxType::Separator);
        line.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        content.addSubview(&line);
        Retained::into_super(banner)
    }

    /// Shows or hides the bar for images from the web and gives the preview the room that is left.
    pub(super) fn show_remote_banner(&self, show: bool) {
        let Some(banner) = self.state.remote_banner.get() else {
            return;
        };
        if banner.isHidden() != show {
            return;
        }
        banner.setHidden(!show);
        // SAFETY: the banner sits in the preview pane for the window's lifetime, so its superview is alive.
        let container = unsafe { banner.superview() };
        let (Some(container), Some(scroll)) = (container, self.host.panes().preview().enclosing_scroll_view()) else {
            return;
        };
        let bounds = container.bounds();
        let height = bounds.size.height - if show { BANNER_HEIGHT } else { 0.0 };
        scroll.setFrame(NSRect::new(
            bounds.origin,
            NSSize::new(bounds.size.width, height.max(0.0)),
        ));
    }

    /// The user allowed images from the web for this document; it is remembered for the file.
    pub(super) fn allow_remote_images(&self) {
        self.state.remote_allowed.set(true);
        self.show_remote_banner(false);
        if let Some(path) = self.document_path() {
            let paths = allowing(allowed_documents(), path);
            let list = NSArray::from_retained_slice(&paths.iter().map(|p| ns(p)).collect::<Vec<_>>());
            // SAFETY: an array of strings is a property list value, as user defaults require.
            unsafe { NSUserDefaults::standardUserDefaults().setObject_forKey(Some(&list), &ns(ALLOWED_KEY)) };
        }
        self.host.invalidate(super::Invalidate::RemoteAllowed);
    }

    fn document_path(&self) -> Option<String> {
        Some(self.host.document()?.fileURL()?.path()?.to_string())
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

/// Whether what leaves the Mac as a file may take this one along: a document from someone else must not pull what it names
/// from the user's folders into a file that gets sent on. Its own folder and its repository are what a document brings along.
pub(super) fn may_carry_along(path: &Path, doc_dir: Option<&Path>) -> bool {
    let Some(doc_dir) = doc_dir else {
        return false;
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let root = what_a_document_brings_along(doc_dir, home.as_deref());
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(file), Ok(root)) => file.starts_with(root),
        _ => false,
    }
}

/// A repository that holds the home folder, as one for dotfiles does, says nothing about what belongs to the document.
fn what_a_document_brings_along<'a>(doc_dir: &'a Path, home: Option<&Path>) -> &'a Path {
    super::env::repository_root(doc_dir)
        .filter(|root| !home.is_some_and(|home| home.starts_with(root)))
        .unwrap_or(doc_dir)
}

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
    fn a_file_leaves_with_a_document_only_from_where_the_document_lies() {
        let root = std::env::temp_dir().join(format!("splat-carry-{}", std::process::id()));
        let repo = root.join("repo");
        let docs = repo.join("docs");
        let plain = root.join("plain");
        for dir in [&repo.join(".git"), &docs, &plain] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let file = |at: &Path| {
            std::fs::write(at, b"x").unwrap();
            at.to_path_buf()
        };
        let (beside, in_repo, elsewhere) = (
            file(&docs.join("a.png")),
            file(&repo.join("b.png")),
            file(&root.join("c.png")),
        );
        let in_plain = file(&plain.join("d.png"));
        let through = plain.join("durch.png");
        std::os::unix::fs::symlink(&elsewhere, &through).unwrap();
        assert!(may_carry_along(&beside, Some(&docs)));
        assert!(
            may_carry_along(&in_repo, Some(&docs)),
            "a repository keeps its pictures anywhere inside"
        );
        assert!(!may_carry_along(&elsewhere, Some(&docs)));
        assert!(may_carry_along(&in_plain, Some(&plain)));
        assert!(!may_carry_along(&elsewhere, Some(&plain)));
        assert!(!may_carry_along(&plain.join("../c.png"), Some(&plain)), "up and out");
        assert!(!may_carry_along(&through, Some(&plain)), "a link that points out");
        assert!(!may_carry_along(&beside, None), "a document without a place");
        assert!(!may_carry_along(&docs.join("missing.png"), Some(&docs)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_repository_around_the_home_folder_opens_nothing() {
        let root = std::env::temp_dir().join(format!("splat-carry-home-{}", std::process::id()));
        let (home, docs) = (root.join("home"), root.join("home/docs"));
        std::fs::create_dir_all(home.join(".git")).unwrap();
        std::fs::create_dir_all(&docs).unwrap();
        assert_eq!(
            what_a_document_brings_along(&docs, Some(&home)),
            docs,
            "a repository of dotfiles"
        );
        assert_eq!(what_a_document_brings_along(&docs, Some(&root.join("other"))), home);
        assert_eq!(what_a_document_brings_along(&docs, None), home);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn web_images_load_only_for_a_document_that_was_allowed() {
        let allowed = vec!["/a.md".to_string()];
        assert!(is_allowed(&allowed, Some("/a.md")));
        assert!(!is_allowed(&allowed, Some("/b.md")));
        assert!(
            !is_allowed(&allowed, None),
            "a document without a file has no permission to remember"
        );
        assert!(
            !PictureState::default().remote_allowed.get(),
            "and nothing is allowed before asking"
        );
    }

    #[test]
    fn the_list_of_allowed_documents_keeps_each_once_and_only_the_newest() {
        let list = allowing(vec!["/a.md".into(), "/b.md".into()], "/a.md".into());
        assert_eq!(list, ["/b.md", "/a.md"]);

        let full: Vec<String> = (0..MAX_ALLOWED_DOCUMENTS).map(|n| format!("/{n}.md")).collect();
        let list = allowing(full, "/neu.md".into());
        assert_eq!(list.len(), MAX_ALLOWED_DOCUMENTS);
        assert_eq!(list[0], "/1.md");
        assert_eq!(list[MAX_ALLOWED_DOCUMENTS - 1], "/neu.md");
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
