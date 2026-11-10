use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

pub use splat_render::files::{read_regular_file, stamp};

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

/// A name for a file made from what a window shows, which may hold anything.
pub fn file_name_of(title: &str) -> String {
    title.replace(['/', ':'], "-")
}
