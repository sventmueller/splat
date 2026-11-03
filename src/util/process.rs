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
