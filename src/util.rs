//! Internal utilities.

/// Checks whether the process with the given PID is still alive.
///
/// Uses `kill(pid, 0)` — signal 0 checks existence without delivering a signal.
#[must_use]
pub fn is_process_alive(pid: u32) -> bool {
    // SAFETY: `kill(pid, 0)` is a standard POSIX existence check that does
    // not deliver any signal.
    #[allow(clippy::cast_possible_wrap)]
    unsafe {
        libc::kill(pid as libc::pid_t, 0) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_alive() {
        assert!(is_process_alive(std::process::id()));
    }

    #[test]
    fn dead_pid_is_not_alive() {
        assert!(!is_process_alive(999_999_999));
    }
}
