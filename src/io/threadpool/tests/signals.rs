// audited: 2026-10-04
//! The signal reads on the thread pool: the mask they leave, the signal they return, the close that drains.
//!
//! docs/posix-signals.md

use super::super::*;
use crate::io::isolate::{run, Outcome};

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

/// Bounds for a signal read in an isolated body. Every case here sends the signal
/// it waits for, so the deadline is only there to make a regression a failed
/// assertion rather than a body that hangs until `run`'s own deadline.
fn watch_bounds() -> Bounds {
    Bounds::new(
        crate::io::request::Bound::per_op(std::time::Duration::from_secs(5)),
        None,
    )
}

/// Run `body` as the only thread of its own process, and fail the test unless
/// it exits 0. `what` names the body in the failure.
///
/// A process-directed `kill(getpid(), SIGUSR1)` reaches any thread that leaves
/// the signal unblocked. The body's process holds only the body's thread and
/// the pool worker it starts, both with the signal blocked, as production does.
fn body_must_succeed(body: fn() -> i32, what: &str) {
    let report = run(std::time::Duration::from_secs(8), body);
    assert_eq!(
        report.end,
        Outcome::Exited(0),
        "{what}: the body failed; an exit code names the step in its body"
    );
}

/// A signal read on the pool returns the signal the process sends itself.
///
/// The body runs in a process of its own, for a clean thread topology that
/// mirrors production: only the main thread plus our intentionally-
/// spawned threadpool worker, all with the watched signal masked.
/// In the cargo test runner this isn't true — peer test threads have
/// SIGUSR1 unmasked and would absorb the `kill()` before our
/// signalfd/kqueue worker reads it.
///
/// Body flow:
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
/// The body exits 0 on success, a small positive code on failure.
///
/// The trap on macOS: kqueue's `EVFILT_SIGNAL` fires from the in-kernel
/// delivery path, so if every thread in the process blocks the signal the
/// kernel parks it on the process pending list and the knote is never
/// activated. A worker that does not unblock the signal for its read hangs
/// the body past `run`'s 8 s deadline.
///
/// The counter-factual on Linux: a worker that reads the non-blocking
/// signalfd without polling it for readiness first reads `EAGAIN` before the
/// signal arrives, and the body exits 17.
#[test]
fn sig_read_returns_after_kill_to_self() {
    body_must_succeed(sig_read_child_logic, "sig_read");
}

/// The isolated body of `sig_read_returns_after_kill_to_self`.
/// Returns a small positive exit code identifying which step failed,
/// or 0 on success. Kept narrow on purpose: no allocations before
/// the kernel calls beyond what `SignalReceiver` and
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
    // (ev/sleep 0.05) preamble in tests/lang/posix.lisp test #1.
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

/// Closing a receiver with a signal still pending leaves the process alive.
///
/// The trap on macOS: after the kqueue worker reports the event for a
/// `kill(getpid(), SIGUSR1)`, macOS leaves an instance of the signal in the
/// process pending queue. `EVFILT_SIGNAL` counts `kill()` generations on the
/// knote but does not consume from the pending queue, and the worker's brief
/// unblock and no-op handler drain at most one instance.
///
/// The counter-factual: without the drain in `rollback`
/// (`crate::io::sigfd::drain_pending_blocked`), `os/sig-close` restores the
/// SIGUSR1 default disposition (Term) and then unblocks the signal. The
/// pending Term fires on the closing thread and kills the process mid-close,
/// as `tests/lang/posix.lisp` test 5 would at `test 5: pre-close`.
///
/// This test runs that shape (two kills, one read, close) in an isolated body
/// and asserts the body exits 0 rather than dying from SIGUSR1.
#[test]
fn close_drains_pending_after_two_kills() {
    body_must_succeed(close_drain_child_logic, "close_drains_pending");
}

/// The isolated body of `close_drains_pending_after_two_kills`.
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
    // about-to-be-restored SIGUSR1 default (Term) and the body
    // dies from SIGUSR1 — observable as `Outcome::Signaled(SIGUSR1)`
    // in the report `run` returns.
    if unsafe { libc::kill(libc::getpid(), libc::SIGUSR1) } != 0 {
        return 28;
    }
    // Brief sleep so the kill is definitely queued before close.
    std::thread::sleep(Duration::from_millis(10));

    // With the drain this returns; without it the process dies here.
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
/// Isolated for the reason the tests above are: `SignalReceiver::new` changes
/// process-wide signal disposition, which peer test threads share.
#[test]
fn a_stopped_sig_read_ends_rather_than_waiting_for_a_signal() {
    body_must_succeed(stopped_sig_read_child_logic, "stopped_sig_read");
}

/// The isolated body of
/// `a_stopped_sig_read_ends_rather_than_waiting_for_a_signal`.
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
    let bounds = pool.bounds(id, crate::io::request::Bound::NONE);
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
