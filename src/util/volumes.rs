use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::lanes::{DISK, Lane};

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
            let bytes = fs.f_mntonname.map(|c| u8::from_ne_bytes(c.to_ne_bytes()));
            let name = std::ffi::CStr::from_bytes_until_nul(&bytes).ok()?;
            name.to_str().ok().map(str::to_owned)
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

/// What `f` finds at `path`, for a thread that cannot wait for a volume that stopped answering, or for a helper that does:
/// `None` after `wait`, and at once while the last question to that volume is still open.
pub fn answer_from<T: Send + 'static>(
    path: &Path,
    wait: std::time::Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    static ASKED: BusyVolumes = BusyVolumes::new();
    let volume = volume_of(path);
    let on_volume = ASKED.enter(&volume)?;
    let (answered, answer) = std::sync::mpsc::sync_channel(1);
    lane_of_volume(&volume).run(move || {
        let _ = answered.send(f());
        drop(on_volume);
    });
    answer.recv_timeout(wait).ok()
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
        assert_eq!(
            answer_from(Path::new("/tmp/a.png"), Duration::from_secs(5), || 3),
            Some(3),
            "the start-up disk goes the same way"
        );
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
}
