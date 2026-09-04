//! Subprocess handle stored behind an `IoOp::Spawn`/`ProcessWait` request, and
//! the record a child's exit status is kept in.

use std::cell::RefCell;
use std::process::Child;
use std::sync::{Arc, Mutex};

/// Handle to a running subprocess. Stored as ExternalObject with type_name "process".
#[derive(Debug)]
pub(crate) struct ProcessHandle {
    pid: u32,
    /// The spawned child, kept so an unreaped one can be reaped on drop. The
    /// exit status is NOT read back through it: a wait reaps with `waitpid(2)`
    /// or `IORING_OP_WAITID` on the pid, which leaves this `Child` believing
    /// the process is still running.
    child: RefCell<Child>,
    exit: ExitRecord,
}

impl ProcessHandle {
    pub fn new(pid: u32, child: Child) -> Self {
        ProcessHandle {
            pid,
            child: RefCell::new(child),
            exit: ExitRecord::new(),
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Where this child's exit status is kept. Every operation that may reap
    /// the child carries a clone; see src/io/AGENTS.md § "A reap is never
    /// wasted".
    pub(crate) fn exit(&self) -> &ExitRecord {
        &self.exit
    }
}

/// Reap the subprocess on drop to prevent zombie accumulation.
/// `try_wait` is non-blocking; if the process hasn't exited yet,
/// it stays in the OS process table until it does.
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if self.exit.status().is_none() {
            let _ = self.child.borrow_mut().try_wait();
        }
    }
}

/// Where a child's exit status is kept once somebody has reaped it.
///
/// A reap consumes the status: the kernel hands it over once, and the child is
/// then gone. So the status cannot travel only in the completion of the
/// operation that took it — a wait a deadline cancelled reaps just as
/// effectively as one that answers, and its completion reaches nobody. See
/// src/io/AGENTS.md § "A reap is never wasted" for the argument.
///
/// Shared, and shared across threads: a pool worker takes the status on its own
/// thread, while the ring's status is read off a `siginfo_t` on the scheduler's.
/// The pending entry and the pool operation each hold a clone, so recording
/// reaches no heap value and stays sound on a teardown drain.
#[derive(Debug, Clone)]
pub(crate) struct ExitRecord(Arc<Mutex<Option<i32>>>);

/// What one ask of the kernel produced.
pub(crate) enum Reap {
    /// The child's exit status: this ask reaped it, or the record was already
    /// holding it. Both are an answer, and a waiter cannot tell them apart.
    Exited(i32),
    /// The child is still running. Ask again later.
    Running,
    /// The ask failed, with this errno.
    Failed(i32),
}

impl ExitRecord {
    /// A record for a child nobody has reaped.
    pub(crate) fn new() -> ExitRecord {
        ExitRecord(Arc::new(Mutex::new(None)))
    }

    /// The status this process is holding for the child, if any.
    pub(crate) fn status(&self) -> Option<i32> {
        *self.held()
    }

    /// Keep a status somebody else's reap produced — the kernel's `waitid`,
    /// whose result arrives as a filled `siginfo_t` rather than through
    /// [`reap`](Self::reap).
    ///
    /// The first status wins. A child is reaped once, so a second value under
    /// the same record would be a second reading of one event rather than news.
    pub(crate) fn keep(&self, code: i32) {
        let mut held = self.held();
        if held.is_none() {
            *held = Some(code);
        }
    }

    /// Ask the kernel for `pid`'s status once, and keep whatever comes back.
    ///
    /// The record is held across the `waitpid` call, which is what makes the
    /// ask and the record one step. Two waits on one child are legal, and the
    /// loser's `waitpid` finds a child that is gone; a record consulted after
    /// the syscall would leave the loser reading in the gap between the
    /// winner's reap and the winner's write, and reporting `ECHILD` for a
    /// status this process is holding.
    ///
    /// `WNOHANG` rather than a blocking wait: the kernel reports no readiness
    /// for a child that has not exited, so the caller paces its asks with the
    /// stop pipe visible between them (`src/io/threadpool/child.rs`).
    pub(crate) fn reap(&self, pid: u32) -> Reap {
        let mut held = self.held();
        if let Some(code) = *held {
            return Reap::Exited(code);
        }
        loop {
            let mut status: libc::c_int = 0;
            let ret = unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) };
            if ret > 0 {
                let code = exit_code_from_wait_status(status);
                *held = Some(code);
                return Reap::Exited(code);
            }
            if ret == 0 {
                return Reap::Running;
            }
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            if errno == libc::EINTR {
                continue;
            }
            return Reap::Failed(errno);
        }
    }

    /// The status under the lock. A poisoned mutex still holds a readable
    /// `Option<i32>` — a panic elsewhere cannot leave a half-written status —
    /// so the guard is taken back rather than propagated.
    fn held(&self) -> std::sync::MutexGuard<'_, Option<i32>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The exit code in a `waitpid(2)` status word. A signalled child reports the
/// signal number negated, which is what `subprocess/wait` answers with.
pub(crate) fn exit_code_from_wait_status(status: libc::c_int) -> i32 {
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else if libc::WIFSIGNALED(status) {
        -libc::WTERMSIG(status)
    } else {
        -1
    }
}

/// The exit code in a `siginfo_t` the kernel filled for `IORING_OP_WAITID`.
///
/// The `si_code` values SIGCHLD carries: `CLD_EXITED` (1) puts an exit code in
/// `si_status`, `CLD_KILLED` (2) and `CLD_DUMPED` (3) put a signal number
/// there. Same convention as the status word above — a signal comes back
/// negated.
///
/// # Safety
/// `si` must be a `siginfo_t` the kernel filled: the accessor reads a union
/// arm, and only a completed `waitid` says which arm is live.
pub(crate) unsafe fn exit_code_from_siginfo(si: &libc::siginfo_t) -> i32 {
    match si.si_code {
        1 => si.si_status(),
        2 | 3 => -si.si_status(),
        _ => -1,
    }
}

/// A child that has exited and is still waiting to be reaped.
///
/// The trap the tests here depend on: `waitpid` cannot leave a status in place.
/// `WNOWAIT` is a `waitid(2)` flag, and `wait4(2)` — which is what `waitpid`
/// becomes on Linux — rejects it with `EINVAL`. So the ask below is a `waitid`,
/// and it blocks until the child has exited while leaving the status for the
/// real reap the test is about to make.
///
/// `false` rather than `true`, so a status read back as `0` cannot be a default
/// that nothing wrote.
#[cfg(test)]
pub(crate) fn zombie_child() -> Child {
    let child = std::process::Command::new("false").spawn().unwrap();
    let pid = child.id() as libc::id_t;
    // SAFETY: `infop` is a live, writable siginfo_t for the duration of the call.
    let ret = unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOWAIT)
    };
    assert_eq!(
        ret,
        0,
        "waitid(WNOWAIT) must leave the child reapable: {}",
        std::io::Error::last_os_error(),
    );
    child
}
