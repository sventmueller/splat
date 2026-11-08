use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use splat_file_state::Stamp;

/// The user's home folder from the user database: in the App Sandbox `HOME` names the app's container instead.
pub fn real_home() -> Option<PathBuf> {
    let mut buffer = vec![0 as libc::c_char; 4096];
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut found = std::ptr::null_mut();
    // SAFETY: Valid places for the entry, its strings and the answer, with the length of the string buffer.
    let failed = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            entry.as_mut_ptr(),
            buffer.as_mut_ptr(),
            buffer.len(),
            &raw mut found,
        )
    };
    if failed != 0 || found.is_null() {
        return None;
    }
    // SAFETY: On success the entry is filled, and its directory points into `buffer`, which lives until here.
    let dir = unsafe { std::ffi::CStr::from_ptr(entry.assume_init().pw_dir) };
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes())))
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

pub fn stamp(path: &Path) -> Stamp {
    let Ok(meta) = std::fs::metadata(path) else {
        return Stamp::UNREACHABLE;
    };
    meta.modified()
        .map_or(Stamp::UNREACHABLE, |modified| Stamp::new(modified, meta.len()))
}

/// A name for a file made from what a window shows, which may hold anything.
pub fn file_name_of(title: &str) -> String {
    title.replace(['/', ':'], "-")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
