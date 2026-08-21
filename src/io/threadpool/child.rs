//! Waiting for a subprocess to exit.

use super::*;

/// How long a wait pauses between asks, and the ceiling it grows to.
///
/// The first slices are short so a child that exits at once is reported at
/// once, which is the common case: a `subprocess/exec` of a short command is
/// usually finished before the fiber that spawned it asks. The ceiling keeps a
/// long-running child from costing a wakeup every millisecond for its whole
/// life.
const CHILD_FIRST_PACE: Duration = Duration::from_millis(1);
const CHILD_MAX_PACE: Duration = Duration::from_millis(50);

/// Reap `pid` and report its exit code, or `-errno`.
///
/// `waitpid(pid, .., 0)` holds this worker for the child's whole life, where
/// neither `io/cancel` nor the caller's deadline can reach it — a child that
/// never exits would cost one OS thread for the life of the process, and the
/// fiber that asked would never be resumed. `WNOHANG` asks instead, and the
/// pause between asks watches the stop pipe throughout.
///
/// The exit code travels in `data` rather than in the result code, so a
/// non-zero exit cannot be read as a negative errno.
pub(super) fn process_wait(bound: OpBound, pid: u32) -> (i32, Vec<u8>) {
    let deadline = bound.timeout().map(|t| std::time::Instant::now() + t);
    let mut pace = CHILD_FIRST_PACE;
    loop {
        let mut status: libc::c_int = 0;
        let ret = unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) };
        if ret < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            if errno == libc::EINTR {
                continue;
            }
            return (-errno, Vec::new());
        }
        if ret > 0 {
            let exit_code: i32 = if libc::WIFEXITED(status) {
                libc::WEXITSTATUS(status)
            } else if libc::WIFSIGNALED(status) {
                -libc::WTERMSIG(status)
            } else {
                -1
            };
            return (0, exit_code.to_le_bytes().to_vec());
        }
        // `ret == 0`: the child is still running. The kernel offers no
        // readiness for that, so the next ask is paced.
        match pace_retry(&bound, deadline, pace) {
            Wake::Ready => {}
            Wake::Stopped => return (-libc::ECANCELED, Vec::new()),
            Wake::TimedOut => return (-libc::ETIMEDOUT, Vec::new()),
        }
        pace = (pace * 2).min(CHILD_MAX_PACE);
    }
}
