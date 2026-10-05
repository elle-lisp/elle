// audited: 2026-10-04
//! Runs a test's body as the only thread of a fresh process, for the tests that signal, fault or rewire descriptor 0.
//!
//! docs/analysis/testing.md

use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

/// Set in the copy of the test binary that `run` starts. Its value is the
/// name of the test the copy runs.
pub(crate) const COPY: &str = "ELLE_ISOLATE_COPY";

/// How the body's process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The body returned this code, or passed it to `_exit`.
    Exited(i32),
    /// This signal ended the body's process.
    Signaled(i32),
    /// The body outlived its deadline, and was killed.
    Hung,
}

/// What `run` saw of the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Report {
    /// How the body's process ended.
    pub(crate) end: Outcome,
    /// How many times the body's process stopped. `run` continues a stopped
    /// body each time, so a stop does not end the run.
    pub(crate) stops: u32,
}

/// Run `body` in a process of its own, and report how that process ended.
///
/// The body's return value is its exit code. A body that is still running when
/// `deadline` passes is killed and reported as `Outcome::Hung`.
pub(crate) fn run(deadline: Duration, body: fn() -> i32) -> Report {
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        panic!("fork failed: {}", std::io::Error::last_os_error());
    }
    if pid == 0 {
        let code = body();
        unsafe { libc::_exit(code) };
    }
    wait_for(pid, deadline)
}

/// Wait for the body's process `pid` until it ends or `deadline` passes.
/// Continue it each time it stops.
fn wait_for(pid: libc::pid_t, deadline: Duration) -> Report {
    let until = Instant::now() + deadline;
    let mut stops = 0;
    let mut status: libc::c_int = 0;
    loop {
        let reaped = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG | libc::WUNTRACED) };
        if reaped < 0 {
            panic!("waitpid({pid}): {}", std::io::Error::last_os_error());
        }
        if reaped == pid {
            if libc::WIFSTOPPED(status) {
                stops += 1;
                unsafe { libc::kill(pid, libc::SIGCONT) };
                continue;
            }
            let end = if libc::WIFSIGNALED(status) {
                Outcome::Signaled(libc::WTERMSIG(status))
            } else {
                Outcome::Exited(libc::WEXITSTATUS(status))
            };
            return Report { end, stops };
        }
        if Instant::now() >= until {
            unsafe { libc::kill(pid, libc::SIGKILL) };
            unsafe { libc::waitpid(pid, &mut status, 0) };
            return Report {
                end: Outcome::Hung,
                stops,
            };
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
