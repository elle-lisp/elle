//! Waiting for a subprocess to exit.

use super::*;
use crate::io::request::{ExitRecord, Reap};

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
/// The ask goes through `exit`, which keeps whatever it produces. A stop is
/// only visible at the pauses, so this worker can be cancelled after it has
/// already taken the child's status from the kernel — and a cancelled
/// operation's completion reaches nobody (src/io/AGENTS.md § "A reap is never
/// wasted").
///
/// The exit code travels in `data` rather than in the result code, so a
/// non-zero exit cannot be read as a negative errno.
pub(super) fn process_wait(bound: OpBound, pid: u32, exit: ExitRecord) -> (i32, Vec<u8>) {
    let deadline = bound.timeout().map(|t| std::time::Instant::now() + t);
    let mut pace = CHILD_FIRST_PACE;
    loop {
        match exit.reap(pid) {
            Reap::Exited(code) => return (0, code.to_le_bytes().to_vec()),
            Reap::Failed(errno) => return (-errno, Vec::new()),
            // The child is still running. The kernel offers no readiness for
            // that, so the next ask is paced.
            Reap::Running => {}
        }
        match pace_retry(&bound, deadline, pace) {
            Wake::Ready => {}
            Wake::Stopped => return (-libc::ECANCELED, Vec::new()),
            Wake::TimedOut => return (-libc::ETIMEDOUT, Vec::new()),
        }
        pace = (pace * 2).min(CHILD_MAX_PACE);
    }
}
