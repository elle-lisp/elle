use super::super::*;

/// Regression test for the macOS `EVFILT_SIGNAL` hang that prevented
/// tests/elle/posix.lisp from passing on macOS, and a counter-factual
/// guard for the Linux signalfd EAGAIN fix from commit f7aed410.
///
/// Forks a child process so we get a clean thread topology that
/// mirrors production: only the main thread plus our intentionally-
/// spawned threadpool worker, all with the watched signal masked.
/// In the cargo test runner this isn't true — peer test threads have
/// SIGUSR1 unmasked and would absorb the `kill()` before our
/// signalfd/kqueue worker reads it.
///
/// Child flow:
///   1. Open a `SignalReceiver` for SIGUSR1 (blocks it on this
///      thread; the threadpool worker spawned in step 2 inherits the
///      mask).
///   2. Submit the platform's blocking signal-read op (`SigfdRead` on
///      Linux, `KqSigRead` on macOS) — the same threadpool primitive
///      `submit_sig_next` uses in production.
///   3. `kill(getpid(), SIGUSR1)` from the main thread.
///   4. Wait up to 5 s for a completion; assert it parses to a
///      single SIGUSR1 event.
///
/// Child exits 0 on success, a small positive code on failure.
///
/// On macOS this gates the fix: kqueue's `EVFILT_SIGNAL` fires from
/// the in-kernel delivery path, so if every thread in the process
/// blocks the signal the kernel parks it on the process pending list
/// and the knote is never activated. Without the fix the child hangs
/// past the parent's wait timeout (waitpid loop bounded at 10 s).
#[test]
fn sig_read_returns_after_kill_to_self() {
    use std::time::{Duration, Instant};

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        panic!("fork failed: {}", std::io::Error::last_os_error());
    }
    if pid == 0 {
        // CHILD: run the test logic and _exit. Use _exit to skip
        // atexit/destructors — Rust drop glue across the fork
        // boundary is unsupported in general.
        let code = sig_read_child_logic();
        unsafe { libc::_exit(code) };
    }

    // PARENT: bounded waitpid so a regression surfaces fast instead
    // of wedging the test runner.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut status: libc::c_int = 0;
    loop {
        let wret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if wret == pid {
            break;
        }
        if wret < 0 {
            let errno = std::io::Error::last_os_error();
            panic!("waitpid({}): {}", pid, errno);
        }
        if Instant::now() >= deadline {
            // Kill the child so we don't leak the process and panic
            // with a meaningful message.
            unsafe { libc::kill(pid, libc::SIGKILL) };
            let _ = unsafe { libc::waitpid(pid, &mut status, 0) };
            panic!("sig_read child hung past 10s");
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    if libc::WIFSIGNALED(status) {
        panic!("sig_read child died from signal {}", libc::WTERMSIG(status));
    }
    let code = libc::WEXITSTATUS(status);
    assert_eq!(
        code, 0,
        "sig_read child failed with code {} (see codes in sig_read_child_logic)",
        code
    );
}

/// Body of the forked child for `sig_read_returns_after_kill_to_self`.
/// Returns a small positive exit code identifying which step failed,
/// or 0 on success. Kept narrow on purpose: no allocations between
/// fork and the kernel calls beyond what `SignalReceiver` and
/// `CompletionHub` already do.
fn sig_read_child_logic() -> i32 {
    use crate::io::sigfd::SignalReceiver;
    use std::time::Duration;

    let r = match SignalReceiver::new(vec![libc::SIGUSR1]) {
        Ok(r) => r,
        Err(_) => return 11,
    };
    let fd = match r.raw_fd() {
        Ok(f) => f,
        Err(_) => return 12,
    };

    let mut pool = CompletionHub::new();
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let submit = pool.submit(SubmissionId::from_raw(1), PoolOp::SigfdRead { fd });
    #[cfg(target_os = "macos")]
    let submit = pool.submit(
        1,
        PoolOp::KqSigRead {
            fd,
            signals: vec![libc::SIGUSR1],
        },
    );
    if submit.is_err() {
        return 13;
    }

    // Let the worker enter the blocking syscall first — matches the
    // (ev/sleep 0.05) preamble in tests/elle/posix.lisp test #1.
    std::thread::sleep(Duration::from_millis(50));

    if unsafe { libc::kill(libc::getpid(), libc::SIGUSR1) } != 0 {
        return 14;
    }

    let completions = match pool.wait_pool(Some(5000)) {
        Ok(c) => c,
        Err(_) => return 15,
    };
    if completions.is_empty() {
        return 16;
    }
    let pc = &completions[0];
    if pc.result_code <= 0 {
        return 17;
    }
    let events = r.parse_events(&pc.data[..pc.result_code as usize]);
    if events.is_empty() {
        return 18;
    }
    if events[0].signum != libc::SIGUSR1 {
        return 19;
    }
    r.close();
    0
}

/// Regression test for the macOS test 5 failure mode: after the
/// kqueue worker reports the event for a `kill(getpid(), SIGUSR1)`,
/// macOS leaves an instance of the signal in the process pending
/// queue (EVFILT_SIGNAL counts kill() generations on the knote but
/// does not consume from the pending queue, and the worker's brief
/// SIG_UNBLOCK + no-op handler delivery only drains at most one
/// instance). Before the `rollback`-time drain
/// (src/io/sigfd.rs::drain_pending_blocked) `os/sig-close` would
/// restore the SIGUSR1 default disposition (Term) and then
/// `pthread_sigmask(SIG_UNBLOCK, …)`, firing the pending Term on
/// the closing thread and killing the process mid-close — exactly
/// the silent death observed at `test 5: pre-close` in
/// tests/elle/posix.lisp on macOS CI.
///
/// This test reproduces the shape (two kills, one read, close)
/// inside a forked child and asserts the child exits 0 rather
/// than dying from signal 10/SIGUSR1.
#[test]
fn close_drains_pending_after_two_kills() {
    use std::time::{Duration, Instant};

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        panic!("fork failed: {}", std::io::Error::last_os_error());
    }
    if pid == 0 {
        let code = close_drain_child_logic();
        unsafe { libc::_exit(code) };
    }

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut status: libc::c_int = 0;
    loop {
        let wret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if wret == pid {
            break;
        }
        if wret < 0 {
            let errno = std::io::Error::last_os_error();
            panic!("waitpid({}): {}", pid, errno);
        }
        if Instant::now() >= deadline {
            unsafe { libc::kill(pid, libc::SIGKILL) };
            let _ = unsafe { libc::waitpid(pid, &mut status, 0) };
            panic!("close_drains_pending child hung past 10s");
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    if libc::WIFSIGNALED(status) {
        let sig = libc::WTERMSIG(status);
        panic!(
            "close_drains_pending child died from signal {} \
                 (expected clean exit; pending signal at close-time \
                 unblock was NOT drained)",
            sig
        );
    }
    let code = libc::WEXITSTATUS(status);
    assert_eq!(
        code, 0,
        "close_drains_pending child failed with code {} (see codes in close_drain_child_logic)",
        code
    );
}

/// Body of the forked child for `close_drains_pending_after_two_kills`.
/// Reads ONE signal via sig-next (proving the watcher works), then
/// raises SIGUSR1 AGAIN with no reader pending so the signal sits in
/// the kernel queue at close time. The drain in rollback must
/// consume it; otherwise close's post-restore unblock fires the
/// default disposition (Term for SIGUSR1) on the calling thread and
/// kills us. Reaching `return 0` after `r.close()` IS the test.
///
/// This reproduces on both Linux and macOS:
///  - Linux: signalfd dequeues at read time, so the post-read kill
///    is what leaves something stuck in the queue at close.
///  - macOS: the EVFILT_SIGNAL knote never dequeues from the
///    process pending queue, so the original kill ALSO survives —
///    but the post-read kill is the portable trigger.
fn close_drain_child_logic() -> i32 {
    use crate::io::sigfd::SignalReceiver;
    use std::time::Duration;

    let r = match SignalReceiver::new(vec![libc::SIGUSR1]) {
        Ok(r) => r,
        Err(_) => return 21,
    };
    let fd = match r.raw_fd() {
        Ok(f) => f,
        Err(_) => return 22,
    };

    // First kill + sig-next round-trip. Proves the watcher works
    // and consumes one pending instance through the kernel.
    if unsafe { libc::kill(libc::getpid(), libc::SIGUSR1) } != 0 {
        return 23;
    }
    let mut pool = CompletionHub::new();
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let submit = pool.submit(SubmissionId::from_raw(1), PoolOp::SigfdRead { fd });
    #[cfg(target_os = "macos")]
    let submit = pool.submit(
        1,
        PoolOp::KqSigRead {
            fd,
            signals: vec![libc::SIGUSR1],
        },
    );
    if submit.is_err() {
        return 24;
    }
    let completions = match pool.wait_pool(Some(5000)) {
        Ok(c) => c,
        Err(_) => return 25,
    };
    if completions.is_empty() {
        return 26;
    }
    if completions[0].result_code <= 0 {
        return 27;
    }

    // SECOND kill — no reader pending. The signal sits in the
    // process pending queue (SIGUSR1 still blocked on this thread
    // from SignalReceiver::new). On close, without the drain in
    // rollback the pthread_sigmask SIG_UNBLOCK fires the
    // about-to-be-restored SIGUSR1 default (Term) and the child
    // dies from signal 10 — observable as WIFSIGNALED=true,
    // WTERMSIG=SIGUSR1 in the parent.
    if unsafe { libc::kill(libc::getpid(), libc::SIGUSR1) } != 0 {
        return 28;
    }
    // Brief sleep so the kill is definitely queued before close.
    std::thread::sleep(Duration::from_millis(10));

    // The smoking-gun call. With the drain it returns; without it
    // the process dies here.
    r.close();
    std::thread::sleep(Duration::from_millis(10));
    0
}
