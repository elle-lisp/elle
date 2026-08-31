use super::super::*;

/// The macOS signal read leaves the thread's mask as it found it.
///
/// The trap: `EVFILT_SIGNAL` only fires when the kernel can pick a thread to
/// deliver to, so the read unblocks the watched signals on its own worker. A
/// worker runs the operations submitted after that one too, and every one of
/// them needs the thread unselectable for delivery — so the unblock has to end
/// with the read rather than with the thread.
///
/// The counter-factual: without the restore this test's second assertion is the
/// only thing that fails. Everything the signal path itself does still works —
/// the leak shows up in some later operation's thread being chosen for a signal
/// nobody meant it to take.
#[cfg(target_os = "macos")]
#[test]
fn the_macos_signal_read_blocks_again_what_it_unblocked() {
    fn blocked(signum: libc::c_int) -> bool {
        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), &mut set) };
        unsafe { libc::sigismember(&set, signum) == 1 }
    }

    // Stand this thread up as a worker does: the signal blocked to start with.
    let mut usr1: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe { libc::sigemptyset(&mut usr1) };
    unsafe { libc::sigaddset(&mut usr1, libc::SIGUSR1) };
    let mut previous: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &usr1, &mut previous) };

    {
        let _unblocked = super::super::event::Unblocked::on_this_thread(&[libc::SIGUSR1]);
        assert!(
            !blocked(libc::SIGUSR1),
            "the read must make this thread selectable for delivery"
        );
    }
    assert!(
        blocked(libc::SIGUSR1),
        "the read must block again what it unblocked"
    );

    unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut()) };
}

/// Bounds for a signal read in a forked child. Every case here sends the signal
/// it waits for, so the deadline is only there to make a regression a failed
/// assertion rather than a child that hangs until the parent's own timeout.
fn watch_bounds() -> Bounds {
    Bounds::new(Some(std::time::Duration::from_secs(5)), None)
}

/// Outcome of running a forked child to completion (or killing it on timeout).
enum ForkOutcome {
    /// Child called `_exit(code)`.
    Exited(i32),
    /// Child was terminated by signal `signum` (e.g. an undrained pending
    /// SIGUSR1 firing its default `Term` disposition).
    Signaled(i32),
    /// Child did not reap within the timeout and was `SIGKILL`ed.
    Hung,
}

/// Fork, run `child_logic` in the child (which must `_exit` its return code),
/// and reap the child in the parent with a bounded `waitpid` poll.
///
/// The child is forked from the **multithreaded cargo-test harness** and then
/// does non-async-signal-safe work (allocations in `SignalReceiver::new` /
/// `CompletionHub`, a threadpool worker spawn). POSIX permits only
/// async-signal-safe calls between `fork` and `exec` in a multithreaded
/// process, so a peer harness thread that happens to hold the allocator lock at
/// the fork instant can wedge the child's first `malloc` (surfacing as `Hung`)
/// or make it fail transiently. That is a fork/harness artifact, not a product
/// defect — callers retry and fail only when *every* attempt fails, which still
/// catches a real regression (broken code fails all attempts). Forking is
/// nonetheless required: it yields the single-thread topology a process-directed
/// SIGUSR1 needs (a peer thread with the signal unmasked would otherwise absorb
/// or be killed by the `kill`).
fn run_forked(child_logic: fn() -> i32, timeout: std::time::Duration) -> ForkOutcome {
    use std::time::Instant;

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        panic!("fork failed: {}", std::io::Error::last_os_error());
    }
    if pid == 0 {
        // CHILD: run the logic and _exit. `_exit` skips atexit/destructors —
        // Rust drop glue across the fork boundary is unsupported in general.
        let code = child_logic();
        unsafe { libc::_exit(code) };
    }

    // PARENT: bounded waitpid so a regression surfaces fast instead of wedging
    // the runner.
    let deadline = Instant::now() + timeout;
    let mut status: libc::c_int = 0;
    loop {
        let wret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if wret == pid {
            return if libc::WIFSIGNALED(status) {
                ForkOutcome::Signaled(libc::WTERMSIG(status))
            } else {
                ForkOutcome::Exited(libc::WEXITSTATUS(status))
            };
        }
        if wret < 0 {
            panic!("waitpid({}): {}", pid, std::io::Error::last_os_error());
        }
        if Instant::now() >= deadline {
            unsafe { libc::kill(pid, libc::SIGKILL) };
            let _ = unsafe { libc::waitpid(pid, &mut status, 0) };
            return ForkOutcome::Hung;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Run `child_logic` in a forked child up to `ATTEMPTS` times, returning once an
/// attempt exits 0. Only when *every* attempt fails does this return the last
/// failure description — so a genuine regression (which fails deterministically)
/// still fails the test, while a transient fork/harness race (see `run_forked`)
/// is absorbed by a retry.
fn forked_child_must_succeed(child_logic: fn() -> i32, what: &str) {
    const ATTEMPTS: usize = 3;
    // Per-attempt bound; a real hang regression pays ATTEMPTS × this at most.
    let timeout = std::time::Duration::from_secs(8);
    let mut last = String::new();
    for attempt in 1..=ATTEMPTS {
        match run_forked(child_logic, timeout) {
            ForkOutcome::Exited(0) => return,
            ForkOutcome::Exited(code) => {
                last = format!("attempt {attempt}: {what} child failed with code {code}")
            }
            ForkOutcome::Signaled(sig) => {
                last = format!("attempt {attempt}: {what} child died from signal {sig}")
            }
            ForkOutcome::Hung => last = format!("attempt {attempt}: {what} child hung past 8s"),
        }
    }
    panic!("{what}: all {ATTEMPTS} attempts failed (last: {last})");
}

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
    forked_child_must_succeed(sig_read_child_logic, "sig_read");
}

/// Body of the forked child for `sig_read_returns_after_kill_to_self`.
/// Returns a small positive exit code identifying which step failed,
/// or 0 on success. Kept narrow on purpose: no allocations between
/// fork and the kernel calls beyond what `SignalReceiver` and
/// `CompletionHub` already do.
fn sig_read_child_logic() -> i32 {
    use crate::io::sigfd::SignalReceiver;
    use std::time::Duration;

    let r = match SignalReceiver::new(
        vec![libc::SIGUSR1],
        std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
    ) {
        Ok(r) => r,
        Err(_) => return 11,
    };
    let fd = match r.raw_fd() {
        Ok(f) => f,
        Err(_) => return 12,
    };

    let mut pool = CompletionHub::new();
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let submit = pool.submit(
        SubmissionId::from_raw(1),
        PoolOp::SigfdRead {
            fd,
            trace: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        },
        watch_bounds(),
    );
    #[cfg(target_os = "macos")]
    let submit = pool.submit(
        crate::io::SubmissionId::from_raw(1),
        PoolOp::KqSigRead {
            fd,
            signals: vec![libc::SIGUSR1],
            trace: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        },
        watch_bounds(),
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
    forked_child_must_succeed(close_drain_child_logic, "close_drains_pending");
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

    let r = match SignalReceiver::new(
        vec![libc::SIGUSR1],
        std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
    ) {
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
    let submit = pool.submit(
        SubmissionId::from_raw(1),
        PoolOp::SigfdRead {
            fd,
            trace: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        },
        watch_bounds(),
    );
    #[cfg(target_os = "macos")]
    let submit = pool.submit(
        crate::io::SubmissionId::from_raw(1),
        PoolOp::KqSigRead {
            fd,
            signals: vec![libc::SIGUSR1],
            trace: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        },
        watch_bounds(),
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

/// A signal read that nobody satisfies must end when the operation is stopped.
///
/// `os/sig-watch` names no deadline, so a watcher for a signal that never
/// arrives waits for the life of the process. `io/cancel` — which `ev/timeout`
/// issues on every call the body wins — is the only thing that ends it, and it
/// can only reach a worker that watches its stop pipe alongside the descriptor.
///
/// Forked for the reason the tests above are: `SignalReceiver::new` changes
/// process-wide signal disposition, which peer test threads share.
#[test]
fn a_stopped_sig_read_ends_rather_than_waiting_for_a_signal() {
    forked_child_must_succeed(stopped_sig_read_child_logic, "stopped_sig_read");
}

/// Body of the forked child for `a_stopped_sig_read_ends_rather_than_waiting`.
/// Returns a small positive exit code identifying which step failed, or 0.
fn stopped_sig_read_child_logic() -> i32 {
    use crate::io::sigfd::SignalReceiver;
    use std::time::{Duration, Instant};

    let r = match SignalReceiver::new(
        vec![libc::SIGUSR1],
        std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
    ) {
        Ok(r) => r,
        Err(_) => return 31,
    };
    let fd = match r.raw_fd() {
        Ok(f) => f,
        Err(_) => return 32,
    };

    let mut pool = CompletionHub::new();
    let id = SubmissionId::from_raw(1);
    // No deadline, exactly as `submit_sig_next` builds it: the stop pipe is the
    // whole bound, so this measures the stop and nothing else.
    let bounds = pool.bounds(id, None);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let submit = pool.submit(
        id,
        PoolOp::SigfdRead {
            fd,
            trace: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        },
        bounds,
    );
    #[cfg(target_os = "macos")]
    let submit = pool.submit(
        id,
        PoolOp::KqSigRead {
            fd,
            signals: vec![libc::SIGUSR1],
            trace: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        },
        bounds,
    );
    if submit.is_err() {
        return 33;
    }

    // Let the worker reach its wait, so the stop arrives at a worker already
    // waiting — the order a cancel meets in production.
    std::thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    pool.stop(id);

    let completions = match pool.wait_pool(Some(5000)) {
        Ok(c) => c,
        Err(_) => return 34,
    };
    if completions.is_empty() {
        return 35;
    }
    if completions[0].result_code != -libc::ECANCELED {
        return 36;
    }
    // No signal was ever sent, so a read that returned for any other reason
    // returned for the wrong one. The elapsed check separates "ended on the
    // stop" from "ended on something else that happened to be quick".
    if started.elapsed() > Duration::from_secs(2) {
        return 37;
    }
    pool.forget_stop(id);
    r.close();
    0
}
