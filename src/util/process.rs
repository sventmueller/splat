use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Ends a child of this app that has not been waited for yet, a decoder helper that stopped answering, at once.
pub fn end_child(pid: i32) {
    // Zero and negative numbers name whole groups of processes.
    if pid > 0 {
        // SAFETY: Plain signal delivery to a child that has not been waited for, whose number names no other process
        // until it is.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
}

/// A process that is not a child of this app, told apart from a later one under the same number by when it started.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Process {
    pid: i32,
    started: SystemTime,
}

impl Process {
    /// The process that `pid` names, if the name of its program file starts with `program` and it started before
    /// `asked`, the moment its number was asked for: a process that took the number after that started later.
    pub fn named(pid: i32, asked: SystemTime, program: &str) -> Option<Process> {
        if pid <= 0 {
            return None;
        }
        let started = started(pid)?;
        let runs = program_of(pid)?
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(program));
        (runs && started <= asked).then_some(Process { pid, started })
    }

    /// Ends the process at once if its number still names it, and says whether it did.
    pub fn end(self) -> bool {
        if started(self.pid) != Some(self.started) {
            return false;
        }
        // SAFETY: Plain signal delivery to a positive number, which named this process just now: only its end and a
        // new process in the moment since could give the number to another.
        unsafe { libc::kill(self.pid, libc::SIGKILL) == 0 }
    }
}

/// When the process `pid` started, or `None` if no process has that number.
fn started(pid: i32) -> Option<SystemTime> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = i32::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: The buffer has the size of the kind of answer asked for.
    let filled = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, info.as_mut_ptr().cast(), size) };
    if filled != size {
        return None;
    }
    // SAFETY: The call has filled the whole buffer, as just checked.
    let info = unsafe { info.assume_init() };
    let since = Duration::from_secs(info.pbi_start_tvsec).checked_add(Duration::from_micros(info.pbi_start_tvusec))?;
    UNIX_EPOCH.checked_add(since)
}

/// The program file of the process `pid`.
fn program_of(pid: i32) -> Option<PathBuf> {
    let mut path = vec![0u8; usize::try_from(libc::PROC_PIDPATHINFO_MAXSIZE).ok()?];
    let size = u32::try_from(path.len()).ok()?;
    // SAFETY: The buffer is as long as said.
    let len = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), size) };
    let len = usize::try_from(len).ok().filter(|&len| len > 0 && len <= path.len())?;
    path.truncate(len);
    Some(PathBuf::from(std::ffi::OsString::from_vec(path)))
}

/// Whether a process with this number exists.
pub fn process_exists(pid: u32) -> bool {
    // SAFETY: Signal 0 only asks whether the process exists; nothing is delivered.
    i32::try_from(pid).is_ok_and(|pid| unsafe { libc::kill(pid, 0) } == 0)
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Child, Command};

    use super::*;

    fn sleeper() -> (Child, i32) {
        let child = Command::new("/bin/sleep").arg("30").spawn().expect("a child");
        let pid = i32::try_from(child.id()).expect("a process number");
        (child, pid)
    }

    #[test]
    fn a_process_is_known_by_its_number_its_program_and_its_start() {
        let before = SystemTime::now() - Duration::from_secs(1);
        let (mut child, pid) = sleeper();
        let asked = SystemTime::now();
        let process = Process::named(pid, asked, "sleep").expect("the child");
        assert!(process.started > before && process.started <= asked);
        assert_eq!(
            Process::named(pid, asked, "com.apple.WebKit.WebContent"),
            None,
            "another program"
        );
        assert_eq!(
            Process::named(pid, before, "sleep"),
            None,
            "started after its number was asked for"
        );
        assert!(process.end());
        let status = child.wait().expect("its end");
        assert_eq!(status.signal(), Some(libc::SIGKILL));
    }

    #[test]
    fn a_process_that_started_at_another_time_is_left_alone() {
        let (mut child, pid) = sleeper();
        let process = Process::named(pid, SystemTime::now(), "sleep").expect("the child");
        let other = Process {
            started: process.started - Duration::from_micros(1),
            ..process
        };
        assert!(!other.end(), "another process under the same number");
        assert_eq!(child.try_wait().expect("a status"), None, "still running");
        child.kill().expect("its end");
        child.wait().expect("its end");
        assert!(!process.end(), "its number names nobody now");
    }

    #[test]
    fn numbers_of_no_single_process_name_none() {
        let now = SystemTime::now();
        assert_eq!(Process::named(0, now, ""), None, "the group of the app");
        assert_eq!(Process::named(-1, now, ""), None, "every process");
        let (mut child, pid) = sleeper();
        child.kill().expect("its end");
        child.wait().expect("its end");
        assert_eq!(
            Process::named(pid, now, ""),
            None,
            "gone, and whatever has its number now started later"
        );
    }

    #[test]
    fn a_child_ends_at_once() {
        let (mut child, pid) = sleeper();
        end_child(pid);
        assert_eq!(child.wait().expect("its end").signal(), Some(libc::SIGKILL));
    }
}
