use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSEventModifierFlags, NSLayoutManager, NSMenu, NSMenuItem, NSTextContainer, NSTextInputClient, NSTextStorage,
    NSTextView,
};
use objc2_foundation::{NSRange, NSString};

use crate::markdown::text::{Shown, U16, Utf16Map};

/// Reads a constant that AppKit, Foundation or WebKit exports.
macro_rules! constant {
    ($name:path) => {
        // SAFETY: the frameworks set their exported constants when they load, before any Rust code runs, and never
        // change them.
        unsafe { $name }
    };
}
pub(crate) use constant;

/// A size as AppKit's signed counts have it; no text or table comes near the difference.
pub fn to_isize(n: usize) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

/// A count from AppKit as a size; a negative one, which says "none", counts nothing.
pub fn to_usize(n: isize) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// The bytes of a regular file of at most `limit` bytes. A path from a document may name anything: a device has no end,
/// and opening a named pipe blocks until someone writes to it.
pub fn read_regular_file(path: &Path, limit: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > limit {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}

/// Upcasts any Objective-C object for heterogeneous containers such as attribute dictionaries.
pub fn obj<T: objc2::ClassType + 'static>(r: Retained<T>) -> Retained<objc2::runtime::AnyObject> {
    r.into()
}

/// How many arguments the method behind `selector` takes: its name has one colon for each.
fn arguments_of(selector: Sel) -> usize {
    selector.name().to_bytes().iter().filter(|&&byte| byte == b':').count()
}

/// Sends `selector` to `target` after `delay` seconds, replacing a request for it that has not run yet. With
/// `common_modes` it also runs while a scroller is dragged or a menu is open.
pub fn perform_later(target: &objc2::runtime::NSObject, selector: Sel, delay: f64, common_modes: bool) {
    use objc2_foundation::NSObjectNSDelayedPerforming;
    assert_eq!(arguments_of(selector), 0, "{selector:?} would get nil as its argument");
    // SAFETY: A selector without arguments, checked above; the run loop retains the target until the call.
    unsafe {
        objc2::runtime::NSObject::cancelPreviousPerformRequestsWithTarget_selector_object(target, selector, None);
        if common_modes {
            let modes = objc2_foundation::NSArray::from_slice(&[objc2_foundation::NSRunLoopCommonModes]);
            target.performSelector_withObject_afterDelay_inModes(selector, None, delay, &modes);
        } else {
            target.performSelector_withObject_afterDelay(selector, None, delay);
        }
    }
}

/// The current local time in `format`, an NSDateFormatter pattern read with fixed English conventions.
pub fn now(format: &str) -> String {
    let formatter = objc2_foundation::NSDateFormatter::new();
    formatter.setLocale(Some(&objc2_foundation::NSLocale::localeWithLocaleIdentifier(&ns(
        "en_US_POSIX",
    ))));
    formatter.setDateFormat(Some(&ns(format)));
    formatter.stringFromDate(&objc2_foundation::NSDate::now()).to_string()
}

/// Modification time and size of a file; `None` when it cannot be reached.
pub type Stamp = Option<(SystemTime, u64)>;

pub fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

type Job = Box<dyn FnOnce() + Send>;

/// Threads that run jobs one after the other, started by the first job. A job that panics ends only itself: every job
/// behind it would otherwise wait forever.
pub struct Lane {
    name: &'static str,
    workers: usize,
    stack: Option<usize>,
    jobs: OnceLock<Sender<Job>>,
}

impl Lane {
    pub const fn new(name: &'static str) -> Lane {
        Lane {
            name,
            workers: 1,
            stack: None,
            jobs: OnceLock::new(),
        }
    }

    pub const fn with_workers(mut self, workers: usize) -> Lane {
        self.workers = workers;
        self
    }

    pub const fn with_stack(mut self, bytes: usize) -> Lane {
        self.stack = Some(bytes);
        self
    }

    pub fn run(&self, job: impl FnOnce() + Send + 'static) {
        let jobs = self.jobs.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Job>();
            let rx = Arc::new(Mutex::new(rx));
            for n in 0..self.workers {
                let rx = rx.clone();
                let mut thread = std::thread::Builder::new().name(if self.workers == 1 {
                    self.name.into()
                } else {
                    format!("{}-{n}", self.name)
                });
                if let Some(bytes) = self.stack {
                    thread = thread.stack_size(bytes);
                }
                thread
                    .spawn(move || {
                        // The lock is held while waiting only, and each worker takes one job at a time.
                        while let Ok(Ok(job)) = rx.lock().map(|rx| rx.recv()) {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                        }
                    })
                    .expect("lane thread");
            }
            tx
        });
        let _ = jobs.send(Box::new(job));
    }
}

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

/// Ends a process of this app's own, a decoder helper or a web process that stopped answering, at once.
pub fn end_process(pid: i32) {
    const SIGKILL: i32 = 9;
    // Zero and negative numbers name whole groups of processes.
    if pid > 0 {
        // SAFETY: Plain signal delivery to a process number that names no one else: a child not waited for yet, or the
        // web process of a view that has its website data store to itself.
        unsafe { kill(pid, SIGKILL) };
    }
}

/// Whether a process with this number exists.
pub fn process_exists(pid: u32) -> bool {
    // SAFETY: Signal 0 only asks whether the process exists; nothing is delivered.
    i32::try_from(pid).is_ok_and(|pid| unsafe { kill(pid, 0) } == 0)
}

/// A name for a file made from what a window shows, which may hold anything.
pub fn file_name_of(title: &str) -> String {
    title.replace(['/', ':'], "-")
}

thread_local! {
    static CONTAINED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the panic under way ends in `contained`, so the app goes on.
pub fn panic_is_contained() -> bool {
    CONTAINED.with(std::cell::Cell::get)
}

/// Runs `f`; `None` if it panicked. A bug in parsing or layout then costs the preview of this text and a bug in a command
/// that command, not the app: all of them compute values, and the text stays safe in its view.
pub fn contained<T>(f: impl FnOnce() -> T) -> Option<T> {
    let before = CONTAINED.replace(true);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok();
    CONTAINED.set(before);
    result
}

/// CPU work off the main thread.
pub static BACKGROUND: Lane = Lane::new("splat-render");

/// Frees a large value elsewhere: a whole document tree takes long enough to be felt while typing.
pub fn free_in_background<T: Send + 'static>(value: T) {
    BACKGROUND.run(move || drop(value));
}

/// Everything that touches the user's folders: a network volume that stopped answering may hold this thread for
/// minutes, and the main thread must not wait for it.
pub static DISK: Lane = Lane::new("splat-disk");

/// The volume a path lies on: what is not the start-up disk may stop answering. Asks no file system, so it cannot hang.
pub fn volume_of(path: &Path) -> String {
    static KNOWN: Mutex<Option<(std::time::Instant, Arc<Vec<PathBuf>>)>> = Mutex::new(None);
    let mounts = {
        let mut known = KNOWN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match &*known {
            Some((at, mounts)) if at.elapsed().as_secs() < 1 => mounts.clone(),
            _ => known
                .insert((std::time::Instant::now(), Arc::new(mounted_elsewhere())))
                .1
                .clone(),
        }
    };
    volume_among(path, &mounts)
}

/// Cloud folders and the automounter's places are named, since they hang like a volume without being mounted as one.
fn volume_among(path: &Path, mounts: &[PathBuf]) -> String {
    let parts: Vec<&str> = path.iter().filter_map(|c| c.to_str()).collect();
    let named = match parts.as_slice() {
        ["/", "Volumes" | "net" | "Network" | "home", _, ..] => 3,
        ["/", "Users", _, "Library", "CloudStorage", _, ..] => 6,
        ["/", "Users", _, "Library", "Mobile Documents", ..] => 5,
        _ => 0,
    };
    let mounted = mounts
        .iter()
        .filter(|mount| path.starts_with(mount))
        .map(|mount| mount.components().count())
        .max()
        .unwrap_or(0);
    let depth = named.max(mounted);
    if depth == 0 {
        return "/".to_string();
    }
    path.components()
        .take(depth)
        .collect::<PathBuf>()
        .to_string_lossy()
        .into_owned()
}

/// The mount points outside the start-up disk, whose system and data parts count as one.
fn mounted_elsewhere() -> Vec<PathBuf> {
    // SAFETY: Without a buffer it only counts. `MNT_NOWAIT` takes what the kernel knows without asking a file system.
    let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
    let Ok(count) = usize::try_from(count) else {
        return Vec::new();
    };
    let mut all: Vec<libc::statfs> = Vec::with_capacity(count);
    let Ok(room) = libc::c_int::try_from(count * std::mem::size_of::<libc::statfs>()) else {
        return Vec::new();
    };
    // SAFETY: The buffer has room for `room` bytes, and the call returns how many entries it filled.
    let filled = unsafe { libc::getfsstat(all.as_mut_ptr(), room, libc::MNT_NOWAIT) };
    // SAFETY: That many entries were just written.
    unsafe { all.set_len(usize::try_from(filled).unwrap_or(0).min(count)) };
    all.iter()
        .filter_map(|fs| {
            // SAFETY: The kernel ends the name of a mount point with a zero inside its fixed field.
            let name = unsafe { std::ffi::CStr::from_ptr(fs.f_mntonname.as_ptr()) };
            name.to_str().ok()
        })
        .filter(|mount| *mount != "/" && !mount.starts_with("/System/Volumes/"))
        .map(PathBuf::from)
        .collect()
}

/// As many volumes as get a thread of their own; a document naming more shares `DISK` for the rest.
const MAX_VOLUME_LANES: usize = 16;

/// Where work on the files of `volume` runs: on a thread of its own, since whoever touches a network volume that stopped
/// answering hangs with it, and that must stay the problem of its own files.
pub fn lane_of_volume(volume: &str) -> &'static Lane {
    static LANES: Mutex<Option<std::collections::HashMap<String, &'static Lane>>> = Mutex::new(None);
    if volume == "/" {
        return &DISK;
    }
    let mut lanes = LANES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let lanes = lanes.get_or_insert_default();
    if let Some(lane) = lanes.get(volume) {
        return lane;
    }
    if lanes.len() >= MAX_VOLUME_LANES {
        return &DISK;
    }
    let lane: &'static Lane = Box::leak(Box::new(Lane::new("splat-disk-volume")));
    lanes.insert(volume.to_string(), lane);
    lane
}

pub fn lane_of(path: &Path) -> &'static Lane {
    lane_of_volume(&volume_of(path))
}

/// What `f` finds at `path`, for a thread that cannot wait for a volume that stopped answering: `None` after `wait`, and
/// at once while the last question to that volume is still open. The start-up disk is simply asked.
pub fn answer_from<T: Send + 'static>(
    path: &Path,
    wait: std::time::Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    static ASKED: BusyVolumes = BusyVolumes::new();
    let volume = volume_of(path);
    if volume == "/" {
        return Some(f());
    }
    let on_volume = ASKED.enter(&volume)?;
    let (answered, answer) = std::sync::mpsc::sync_channel(1);
    lane_of_volume(&volume).run(move || {
        let _ = answered.send(f());
        drop(on_volume);
    });
    answer.recv_timeout(wait).ok()
}

/// What `f` returns on the main thread; `None` if it panicked. For a thread the main thread never waits for.
pub fn on_main<T: Send>(f: impl FnOnce() -> T + Send) -> Option<T> {
    if objc2::MainThreadMarker::new().is_some() {
        return contained(f);
    }
    let mut out = None;
    dispatch2::DispatchQueue::main().exec_sync(|| out = contained(f));
    out
}

/// Volumes something is under way on, so that it does not pile up behind one that stopped answering while the others go on.
pub struct BusyVolumes(Mutex<Vec<String>>);

/// Says that a volume is free again when dropped, so a panic on the way cannot leave it busy for good.
pub struct OnVolume {
    busy: &'static BusyVolumes,
    volume: String,
}

impl BusyVolumes {
    pub const fn new() -> BusyVolumes {
        BusyVolumes(Mutex::new(Vec::new()))
    }

    /// `None` if something is still under way on `volume`.
    pub fn enter(&'static self, volume: &str) -> Option<OnVolume> {
        let mut busy = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if busy.iter().any(|v| v == volume) {
            return None;
        }
        busy.push(volume.to_string());
        Some(OnVolume {
            busy: self,
            volume: volume.to_string(),
        })
    }
}

impl Drop for OnVolume {
    fn drop(&mut self) {
        let mut busy = self.busy.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        busy.retain(|v| *v != self.volume);
    }
}

pub fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

pub fn to_nsrange(r: Range<U16>) -> NSRange {
    NSRange::new(r.start.0, r.end.0.saturating_sub(r.start.0))
}

pub fn from_nsrange(r: NSRange) -> Range<U16> {
    U16(r.location)..U16(r.location + r.length)
}

/// A range of the preview view's text.
pub fn shown_to_ns(r: Range<Shown>) -> NSRange {
    NSRange::new(r.start.0, r.end.0.saturating_sub(r.start.0))
}

/// A range of the preview view's text.
pub fn shown_from_ns(r: NSRange) -> Range<Shown> {
    Shown(r.location)..Shown(r.location + r.length)
}

pub fn byte_range_to_ns(map: &Utf16Map, r: &Range<usize>) -> NSRange {
    to_nsrange(map.to_u16(r.start)..map.to_u16(r.end))
}

/// Key equivalent with modifiers: "cmd", "shift", "alt", "ctrl" joined by '+', e.g. "alt+cmd+p".
pub fn item(mtm: MainThreadMarker, title: &str, action: Option<Sel>, key: &str) -> Retained<NSMenuItem> {
    let mut modifiers = NSEventModifierFlags::empty();
    let mut equivalent = "";
    if !key.is_empty() {
        let parts: Vec<&str> = key.split('+').collect();
        let (last, mods) = parts.split_last().unwrap();
        equivalent = last;
        for m in mods {
            modifiers |= match *m {
                "cmd" => NSEventModifierFlags::Command,
                "shift" => NSEventModifierFlags::Shift,
                "alt" => NSEventModifierFlags::Option,
                "ctrl" => NSEventModifierFlags::Control,
                _ => NSEventModifierFlags::empty(),
            };
        }
    }
    assert!(
        action.is_none_or(|action| arguments_of(action) <= 1),
        "{action:?} takes more than the sender"
    );
    // SAFETY: With at most one argument, checked above, the action takes nothing but the sender AppKit passes.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &ns(title), action, &ns(equivalent))
    };
    if !equivalent.is_empty() {
        item.setKeyEquivalentModifierMask(modifiers);
    }
    item
}

pub fn tagged(mtm: MainThreadMarker, title: &str, action: Sel, key: &str, tag: isize) -> Retained<NSMenuItem> {
    let i = item(mtm, title, Some(action), key);
    i.setTag(tag);
    i
}

pub fn submenu(
    mtm: MainThreadMarker,
    title: &str,
    items: Vec<Retained<NSMenuItem>>,
) -> (Retained<NSMenuItem>, Retained<NSMenu>) {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &ns(title));
    for i in &items {
        menu.addItem(i);
    }
    let parent = item(mtm, title, None, "");
    parent.setSubmenu(Some(&menu));
    (parent, menu)
}

pub fn separator(mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    NSMenuItem::separatorItem(mtm)
}

pub fn storage(view: &NSTextView) -> Retained<NSTextStorage> {
    // SAFETY: A text view keeps its text system alive, those made in `views.rs` through their `owner`.
    unsafe { view.textStorage() }.expect("text view without storage")
}

pub fn layout_manager(view: &NSTextView) -> Option<Retained<NSLayoutManager>> {
    // SAFETY: A text view keeps its text system alive, those made in `views.rs` through their `owner`.
    unsafe { view.layoutManager() }
}

pub fn text_container(view: &NSTextView) -> Option<Retained<NSTextContainer>> {
    // SAFETY: A text view keeps its text system alive, those made in `views.rs` through their `owner`.
    unsafe { view.textContainer() }
}

/// Replaces `range` like typing would.
pub fn insert_text(view: &NSTextView, text: &NSString, range: NSRange) {
    // SAFETY: The inserted object is an NSString, one of the two types this method accepts.
    unsafe { view.insertText_replacementRange(text, range) };
}

#[cfg(test)]
mod exception_tests {
    /// Without objc2's `catch-all`, an Objective-C exception ends the process before the panic hook can save text.
    #[test]
    #[should_panic]
    fn objective_c_exceptions_become_panics() {
        let array = objc2_foundation::NSArray::<objc2_foundation::NSString>::new();
        let _ = array.objectAtIndex(3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_may_stop_answering_is_a_volume_of_its_own() {
        for (path, volume) in [
            ("/Users/a/Dokumente/x.md", "/"),
            ("/tmp/x.md", "/"),
            ("x.md", "/"),
            ("/Volumes/NAS/ordner/x.md", "/Volumes/NAS"),
            ("/net/server/x.md", "/net/server"),
            ("/Network/Servers/x.md", "/Network/Servers"),
            (
                "/Users/a/Library/CloudStorage/Dropbox/x.md",
                "/Users/a/Library/CloudStorage/Dropbox",
            ),
            (
                "/Users/a/Library/Mobile Documents/com~apple~CloudDocs/x.md",
                "/Users/a/Library/Mobile Documents",
            ),
        ] {
            assert_eq!(volume_of(Path::new(path)), volume, "{path}");
        }
    }

    #[test]
    fn a_mount_point_anywhere_is_a_volume_of_its_own() {
        let mounts = [PathBuf::from("/Users/a/Server"), PathBuf::from("/Volumes/NAS/tief")];
        for (path, volume) in [
            ("/Users/a/Server/ordner/x.md", "/Users/a/Server"),
            ("/Users/a/Serverin/x.md", "/"),
            ("/Volumes/NAS/tief/x.md", "/Volumes/NAS/tief"),
            ("/Volumes/NAS/x.md", "/Volumes/NAS"),
            ("/home/a/x.md", "/home/a"),
        ] {
            assert_eq!(volume_among(Path::new(path), &mounts), volume, "{path}");
        }
        let mounted = mounted_elsewhere();
        assert!(mounted.iter().any(|m| m == Path::new("/dev")), "{mounted:?}");
        assert!(!mounted.iter().any(|m| m == Path::new("/")), "{mounted:?}");
    }

    #[test]
    fn a_question_to_a_volume_that_hangs_is_given_up() {
        use std::time::{Duration, Instant};
        let at = Path::new("/Volumes/Antwortet-Nicht/a.png");
        let (release, hung) = std::sync::mpsc::channel::<()>();
        let asked = Instant::now();
        let first = answer_from(at, Duration::from_millis(50), move || {
            let _ = hung.recv();
        });
        assert_eq!(first, None);
        assert!(asked.elapsed() >= Duration::from_millis(50));
        let asked = Instant::now();
        assert_eq!(answer_from(at, Duration::from_secs(30), || 1), None, "still hanging");
        assert!(asked.elapsed() < Duration::from_secs(5), "no second wait");
        release.send(()).unwrap();
        let again = std::iter::repeat_with(|| answer_from(at, Duration::from_secs(5), || 2))
            .take(500)
            .find_map(|answer| {
                answer.or_else(|| {
                    std::thread::sleep(Duration::from_millis(10));
                    None
                })
            });
        assert_eq!(again, Some(2), "it answers again");
        assert_eq!(answer_from(Path::new("/tmp/a.png"), Duration::ZERO, || 3), Some(3));
    }

    #[test]
    fn a_volume_that_hangs_holds_up_nothing_but_its_own_files() {
        let (release, hung) = std::sync::mpsc::channel::<()>();
        let (done, finished) = std::sync::mpsc::channel();
        lane_of(Path::new("/Volumes/Haengt/a.md")).run(move || {
            let _ = hung.recv();
        });
        for path in ["/Volumes/Anderes/b.md", "/Users/a/c.md"] {
            let done = done.clone();
            lane_of(Path::new(path)).run(move || done.send(()).unwrap());
        }
        for _ in 0..2 {
            finished
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("work on another volume goes on");
        }
        assert!(std::ptr::eq(
            lane_of(Path::new("/Volumes/Haengt/x.md")),
            lane_of(Path::new("/Volumes/Haengt/y.md"))
        ));
        release.send(()).unwrap();
    }

    #[test]
    fn a_busy_volume_turns_away_what_follows_until_it_is_done() {
        static BUSY: BusyVolumes = BusyVolumes::new();
        let entered = BUSY.enter("/Volumes/A").unwrap();
        assert!(BUSY.enter("/Volumes/A").is_none());
        assert!(BUSY.enter("/Volumes/B").is_some(), "another volume goes on");
        drop(entered);
        assert!(BUSY.enter("/Volumes/A").is_some());
        let caught = std::panic::catch_unwind(|| {
            let _entered = BUSY.enter("/Volumes/C").unwrap();
            panic!("a job that fails");
        });
        assert!(caught.is_err());
        assert!(BUSY.enter("/Volumes/C").is_some(), "a panic leaves it free");
    }

    #[test]
    fn a_lane_keeps_going_through_panics_and_loses_no_job() {
        static LANE: Lane = Lane::new("splat-test-stress").with_workers(4);
        let (done, finished) = std::sync::mpsc::channel();
        const JOBS: usize = 2000;
        for n in 0..JOBS {
            let done = done.clone();
            LANE.run(move || {
                assert!(n % 7 != 0, "a job that fails");
                done.send(n).unwrap();
            });
        }
        drop(done);
        let mut ran: Vec<usize> = finished.iter().collect();
        ran.sort_unstable();
        assert_eq!(ran, (0..JOBS).filter(|n| n % 7 != 0).collect::<Vec<_>>());
    }

    #[test]
    fn only_regular_files_within_the_limit_are_read() {
        let dir = std::env::temp_dir().join(format!("splat-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, "1234").unwrap();
        assert_eq!(read_regular_file(&file, 4).as_deref(), Some(&b"1234"[..]));
        assert_eq!(read_regular_file(&file, 3), None, "too large");
        assert_eq!(
            read_regular_file(Path::new("/dev/zero"), 1 << 30),
            None,
            "a device has no end"
        );
        assert_eq!(read_regular_file(&dir, 1 << 30), None, "a folder");
        let pipe = dir.join("pipe");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&pipe)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(read_regular_file(&pipe, 1 << 30), None, "opening a pipe would block");
        std::fs::remove_dir_all(&dir).unwrap();
    }
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn a_selector_has_one_argument_for_each_colon() {
        assert_eq!(arguments_of(objc2::sel!(a)), 0);
        assert_eq!(arguments_of(objc2::sel!(a:)), 1);
        assert_eq!(arguments_of(objc2::sel!(a:b:)), 2);
    }

    #[test]
    #[should_panic(expected = "would get nil")]
    fn a_selector_that_expects_an_argument_is_not_performed_later() {
        perform_later(&objc2::runtime::NSObject::new(), objc2::sel!(a:), 0.0, false);
    }

    #[test]
    fn a_panic_ends_in_contained_and_is_known_while_it_unwinds() {
        assert_eq!(contained(|| 7), Some(7));
        assert!(!panic_is_contained());
        let during = std::sync::Arc::new(AtomicBool::new(false));
        let seen = during.clone();
        let previous = std::panic::take_hook();
        // Other tests panic at the same time, and their panics are not contained.
        std::panic::set_hook(Box::new(move |_| {
            seen.fetch_or(panic_is_contained(), Ordering::SeqCst);
        }));
        let result = contained(|| -> u8 { panic!("a bug in the parser") });
        std::panic::set_hook(previous);
        assert_eq!(result, None);
        assert!(during.load(Ordering::SeqCst));
        assert!(!panic_is_contained());
    }
}
