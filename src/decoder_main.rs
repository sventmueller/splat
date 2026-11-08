//! The decoding service of Splat. macOS starts it from the app bundle in a sandbox of its own, without the user's files
//! and without the network; the app hands it a socket per helper it wants and reads pixels and PDFs back. With `--stdio`
//! it answers on its standard streams instead, for the tests, without a sandbox.

mod editor {
    pub mod safe_image;

    pub mod decoder {
        #[allow(
            dead_code,
            reason = "the app reads the answers and the service the requests; each uses its half"
        )]
        pub mod protocol;
        pub mod serve;
    }
}

mod util {
    #[allow(dead_code, reason = "the app connects and the service serves; each uses its half")]
    pub mod xpc;
}

use std::os::fd::OwnedFd;

use editor::decoder::serve;

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--stdio") => serve::answer(std::io::stdin().lock(), std::io::stdout().lock(), false),
        Some("--stdio-native") => serve::answer(std::io::stdin().lock(), std::io::stdout().lock(), true),
        _ => {
            #[cfg(coverage)]
            measuring::start();
            serve::watch(end);
            util::xpc::serve(take, end);
        }
    }
}

fn take(stream: OwnedFd, native: bool) {
    let stream = std::os::unix::net::UnixStream::from(stream);
    if let Ok(output) = stream.try_clone() {
        serve::answer(stream, output, native);
    }
    #[cfg(coverage)]
    measuring::write();
}

#[cfg(not(coverage))]
fn end(code: i32) {
    std::process::exit(code);
}

#[cfg(coverage)]
fn end(code: i32) {
    measuring::end(code);
}

/// A measured build writes what ran after each socket and renames the finished file, as launchd ends the service
/// without a word, also in the middle of a write, and one cut-off file fails the merge of all measurements.
#[cfg(coverage)]
mod measuring {
    use std::ffi::{CString, c_char};
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::{Mutex, PoisonError};

    unsafe extern "C" {
        fn __llvm_profile_set_filename(name: *const c_char);
        fn __llvm_profile_write_file() -> i32;
    }

    /// The file written and the name it gets once complete; held during a write, which the runtime cannot do from two
    /// threads at once.
    static FILES: Mutex<Option<(PathBuf, PathBuf)>> = Mutex::new(None);

    pub fn start() {
        if let Some((partial, done)) = files()
            && let Ok(path) = CString::new(partial.as_os_str().as_bytes())
        {
            // SAFETY: A NUL-terminated path, which the runtime copies; a name set this way takes precedence over the
            // environment's.
            unsafe { __llvm_profile_set_filename(path.as_ptr()) };
            *FILES.lock().unwrap_or_else(PoisonError::into_inner) = Some((partial, done));
        }
    }

    /// Named like the files the measurement tool collects, after the pattern `LLVM_PROFILE_FILE` gives.
    fn files() -> Option<(PathBuf, PathBuf)> {
        let pattern = PathBuf::from(std::env::var_os("LLVM_PROFILE_FILE")?);
        let prefix = pattern.file_name()?.to_string_lossy().split('%').next()?.to_owned();
        let named = |ending: &str| {
            Some(
                pattern
                    .parent()?
                    .join(format!("{prefix}{}-decoder.{ending}", std::process::id())),
            )
        };
        Some((named("partial")?, named("profraw")?))
    }

    pub fn write() {
        let files = FILES.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((partial, done)) = files.as_ref() {
            // SAFETY: The runtime's own call, which writes all counters so far to the file `start` named.
            if unsafe { __llvm_profile_write_file() } == 0 {
                let _ = std::fs::rename(partial, done);
            }
        }
    }

    pub fn end(code: i32) -> ! {
        write();
        // SAFETY: Quits at once; skipping the runtime's write at exit leaves no partial file behind.
        unsafe { libc::_exit(code) }
    }
}
