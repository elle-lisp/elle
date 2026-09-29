// audited: 2026-09-29
//! POSIX signal reception through signalfd (Linux) and kqueue (macOS), and the process-wide signal traps.
//!
//! docs/posix-signals.md
//!
//! `SignalReceiver` is the External object behind `os/sig-watch`. Each
//! receiver owns a kernel file descriptor (signalfd on Linux, a dedicated
//! kqueue fd on macOS) that becomes readable when a watched signal is
//! delivered. The scheduler reads the fd through the same IoOp dispatch as
//! filesystem watchers.
//!
//! A module-level [`WatchedSet`] holds a refcount per watched signal.
//! `SignalReceiver::new` increments it and blocks a signal as its count leaves
//! zero. `Drop` decrements it; at zero it drains the pending instances and
//! unblocks the signal, unless the signal is in [`ABSORB_SET`], which stays
//! blocked. The mask policy is in the governing document.

use crate::config::TraceCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Emit a `[trace:posix] …` line to stderr when `trace`'s owning instance has the
/// `posix` bit set (`--trace=posix`, `--trace=all`, or `(vm/config-set :trace …)`
/// at runtime). Read beside the per-test progress lines `tests/lang/posix.lisp`
/// writes to stderr, the trace names the kernel call where Linux and macOS part.
///
/// `trace` is the instance's own [`TraceCell`], threaded here from a
/// `SignalReceiver` (which captured it at `os/sig-watch`), a `NativeCtx`'s heap,
/// or a `PoolOp` that carried it onto a worker thread — so every one of these
/// context-free call sites gates on the right instance with no process-global.
///
/// Output goes via a direct `write(2, …)` syscall, bypassing the elle
/// scheduler and Rust's stdio buffering, so trace lines survive even
/// when the process is about to be killed by an outer timeout.
pub(crate) fn posix_trace(trace: &TraceCell, args: std::fmt::Arguments<'_>) {
    if trace.load(std::sync::atomic::Ordering::Relaxed) & crate::config::trace_bits::POSIX == 0 {
        return;
    }
    let line = format!("[trace:posix] {}\n", args);
    unsafe {
        libc::write(2, line.as_ptr() as *const libc::c_void, line.len());
    }
}

/// A parsed signal delivery.
#[derive(Debug, Clone)]
pub(crate) struct SigEvent {
    pub signum: libc::c_int,
    /// Sender pid. `None` on macOS (kqueue doesn't populate siginfo).
    pub sender_pid: Option<u32>,
    /// Sender uid. `None` on macOS.
    pub sender_uid: Option<u32>,
    /// `ssi_code` (e.g. SI_USER=0, SI_KERNEL=128, CLD_EXITED=1). `0` on macOS.
    pub code: i32,
    /// Coalesced count. Always `1` on Linux; kevent `data` on macOS.
    pub count: u32,
}

/// Process-wide tally of how many `SignalReceiver`s currently want a
/// given signal blocked. When a refcount transitions 0 → 1 we block;
/// 1 → 0 we unblock.
struct WatchedSet {
    refcount: HashMap<libc::c_int, usize>,
}

fn watched_set() -> &'static Mutex<WatchedSet> {
    static SET: OnceLock<Mutex<WatchedSet>> = OnceLock::new();
    SET.get_or_init(|| {
        Mutex::new(WatchedSet {
            refcount: HashMap::new(),
        })
    })
}

/// Run `f` with the process-global watched-signal set locked. Centralises
/// the `watched_set().lock().unwrap()` boilerplate and scopes the lock to
/// the closure.
fn with_watched_set<R>(f: impl FnOnce(&mut WatchedSet) -> R) -> R {
    f(&mut watched_set().lock().unwrap())
}

/// Process-wide table of saved sigaction dispositions for signals on
/// which we installed a no-op handler. macOS only — Linux's signalfd
/// reads pending signals directly without needing a handler to be
/// installed. Keyed by signum; populated on refcount 0→1, restored and
/// removed on 1→0. See `mod platform`'s `new` and `rollback`.
#[cfg(target_os = "macos")]
fn saved_dispositions() -> &'static Mutex<HashMap<libc::c_int, libc::sigaction>> {
    static DISP: OnceLock<Mutex<HashMap<libc::c_int, libc::sigaction>>> = OnceLock::new();
    DISP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Install the process-wide signal traps, on the main thread and before any
/// worker thread spawns. What each set of signals gets, and how a watcher
/// overrides a trap, is in docs/posix-signals.md.
///
/// A thread a C library spawns later (Cranelift, an FFI cdylib) inherits the
/// mask set here. It blocks the absorb set too, so it cannot take a signal a
/// user watches.
///
/// Idempotent: `sigaction` overwrites the previous handler, and the blocked
/// set is constant. The tests fork and call it in each child.
pub fn init_process_signals() {
    install_terminate_handlers();
    install_job_control_handlers();
    install_cont_handler();
    install_pipe_ignore();
    block_absorb_set_on_main_thread();
}

extern "C" fn terminate_handler(signum: libc::c_int) {
    // Async-signal-safe. No allocation, no Rust stdio, no locks.
    // `write(2)` and `_exit(2)` are on the POSIX async-signal-safe list.
    // Tag the message so a user staring at unfamiliar `^elle:
    // terminated by SIGTERM` output can correlate with this code.
    let tag: &[u8] = match signum {
        s if s == libc::SIGTERM => b"elle: terminated by SIGTERM\n",
        s if s == libc::SIGINT => b"elle: interrupted by SIGINT\n",
        s if s == libc::SIGQUIT => b"elle: quit by SIGQUIT\n",
        s if s == libc::SIGHUP => b"elle: hung up by SIGHUP\n",
        _ => b"elle: terminated\n",
    };
    unsafe {
        libc::write(2, tag.as_ptr() as *const libc::c_void, tag.len());
        libc::_exit(128 + signum);
    }
}

extern "C" fn job_control_handler(_signum: libc::c_int) {
    // SIGTSTP / SIGTTIN / SIGTTOU all map to "actually stop the
    // process". `raise(SIGSTOP)` is async-signal-safe and the kernel
    // honours it even from inside a signal handler.
    unsafe {
        libc::raise(libc::SIGSTOP);
    }
}

extern "C" fn cont_handler(_signum: libc::c_int) {
    // Nothing to do — the kernel already resumed us. The handler
    // exists so the kernel has a delivery target for SIGCONT instead
    // of running the (no-op) default disposition; without it, a tool
    // that expects a SIGCONT round-trip (e.g. `kill -CONT` from a
    // shell-script supervisor) sees the signal vanish.
}

/// Build a `sigaction` pointing at `handler` with `SA_RESTART` so the
/// handler running on a syscall doesn't surface as `EINTR` to the
/// caller (we use io_uring and a few raw `libc::poll` calls; both can
/// handle EINTR but auto-restart is friendlier).
fn build_sigaction(handler: extern "C" fn(libc::c_int)) -> libc::sigaction {
    let mut sa: libc::sigaction = unsafe { std::mem::zeroed() };
    sa.sa_sigaction = handler as *const () as libc::sighandler_t;
    sa.sa_flags = libc::SA_RESTART;
    unsafe { libc::sigemptyset(&mut sa.sa_mask) };
    sa
}

fn install_terminate_handlers() {
    let sa = build_sigaction(terminate_handler);
    for s in [libc::SIGTERM, libc::SIGINT, libc::SIGQUIT, libc::SIGHUP] {
        unsafe { libc::sigaction(s, &sa, std::ptr::null_mut()) };
    }
}

fn install_job_control_handlers() {
    let sa = build_sigaction(job_control_handler);
    for s in [libc::SIGTSTP, libc::SIGTTIN, libc::SIGTTOU] {
        unsafe { libc::sigaction(s, &sa, std::ptr::null_mut()) };
    }
}

fn install_cont_handler() {
    let sa = build_sigaction(cont_handler);
    unsafe { libc::sigaction(libc::SIGCONT, &sa, std::ptr::null_mut()) };
}

fn install_pipe_ignore() {
    let mut sa: libc::sigaction = unsafe { std::mem::zeroed() };
    sa.sa_sigaction = libc::SIG_IGN;
    sa.sa_flags = 0;
    unsafe {
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGPIPE, &sa, std::ptr::null_mut());
    }
}

/// Signals that should be silently absorbed unless a `SignalReceiver`
/// is actively watching them. Blocked process-wide on the main thread
/// at startup; workers already mask everything on spawn, so the kernel
/// has no delivery target and the signals just queue.
const ABSORB_SET: &[libc::c_int] = &[
    libc::SIGUSR1,
    libc::SIGUSR2,
    libc::SIGCHLD,
    libc::SIGURG,
    libc::SIGWINCH,
    libc::SIGALRM,
];

fn block_absorb_set_on_main_thread() {
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe { libc::sigemptyset(&mut set) };
    for &s in ABSORB_SET {
        unsafe { libc::sigaddset(&mut set, s) };
    }
    unsafe {
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

/// The fault set: signals the CPU raises synchronously at a specific
/// instruction. They are bound to the faulting thread, so a mask cannot
/// reroute them — it can only jam delivery. Jammed, Linux force-kills
/// the process anyway, but macOS leaves the signal pending and
/// re-executes the faulting instruction, pinning the thread at one PC
/// forever (`fault_on_a_masked_thread_kills_the_process` is the pin).
/// Worker masks therefore always exclude this set, and every signal in it
/// keeps the kernel's default disposition.
const FAULT_SET: &[libc::c_int] = &[
    libc::SIGSEGV,
    libc::SIGBUS,
    libc::SIGILL,
    libc::SIGFPE,
    libc::SIGTRAP,
    libc::SIGSYS,
    libc::SIGABRT,
];

/// Mask every asynchronous signal on the calling thread; the fault set
/// stays deliverable (see `FAULT_SET`). Workers call this as their
/// first action after spawn so the kernel never selects them as the
/// delivery target for an asynchronous signal. Must not be called on
/// the main VM thread (the lazy-block policy depends on the main
/// thread starting with an empty mask).
pub fn mask_all_signals_on_this_thread() {
    unsafe {
        let mut full: libc::sigset_t = std::mem::zeroed();
        libc::sigfillset(&mut full);
        for &s in FAULT_SET {
            libc::sigdelset(&mut full, s);
        }
        // SIG_BLOCK is additive; SIG_SETMASK replaces. We want SIG_SETMASK
        // so a worker thread that gets recycled doesn't accumulate state
        // from a previous owner.
        libc::pthread_sigmask(libc::SIG_SETMASK, &full, std::ptr::null_mut());
    }
}

/// Return the set of signals currently blocked on the calling thread,
/// as libc signum integers. Used by `os/sig-mask`.
pub fn current_thread_blocked() -> Vec<libc::c_int> {
    let mut current: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::pthread_sigmask(0, std::ptr::null(), &mut current);
    }
    // NSIG isn't exposed by Rust libc; 65 covers SIGRTMAX on Linux and is
    // larger than every named signal on macOS. sigismember returns -1 for
    // out-of-range signals, which we filter.
    (1..65i32)
        .filter(|&s| unsafe { libc::sigismember(&current, s) == 1 })
        .collect()
}

/// Return the set of signals currently pending on the calling thread.
/// Used by `os/sig-pending`.
pub fn current_thread_pending() -> Vec<libc::c_int> {
    let mut pending: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::sigemptyset(&mut pending);
        libc::sigpending(&mut pending);
    }
    // NSIG isn't exposed by Rust libc; 65 covers SIGRTMAX on Linux and is
    // larger than every named signal on macOS. sigismember returns -1 for
    // out-of-range signals, which we filter.
    (1..65i32)
        .filter(|&s| unsafe { libc::sigismember(&pending, s) == 1 })
        .collect()
}

/// Return the set of signals currently watched by at least one live
/// receiver (refcount > 0). Used by `os/sig-watching`.
pub fn currently_watched() -> Vec<libc::c_int> {
    with_watched_set(|set| {
        let mut out: Vec<libc::c_int> = set
            .refcount
            .iter()
            .filter_map(|(s, c)| if *c > 0 { Some(*s) } else { None })
            .collect();
        out.sort_unstable();
        out
    })
}

/// Drain any of `signals` still pending on the calling thread or the
/// process-shared queue, using `sigwait`. The signals must still be
/// blocked on the calling thread for `sigwait` to consume from the
/// queue without invoking a handler.
///
/// Called from each platform's `rollback` immediately before the saved
/// disposition is restored and the signals are unblocked. Without this
/// drain, a signal queued during the watch that the watcher never
/// consumed would fire its now-restored default disposition on
/// `pthread_sigmask(SIG_UNBLOCK, …)` and terminate the process mid-close.
/// The trap on macOS: after two `kill(SIGUSR1)` calls, kqueue
/// `EVFILT_SIGNAL` reports count=2, but one delivery drains the process
/// pending queue and the other stays (`tests/lang/posix.lisp` test 5).
/// On Linux the situation is the same
/// when a user opens a `SignalReceiver`, the kernel queues a signal
/// they intentionally chose to watch, and they close without ever
/// calling `os/sig-next`: the unblock would Term them on the way out.
///
/// `sigwait` is POSIX and present on both Linux and macOS. It blocks
/// until a signal in `set` becomes pending — we gate every call on
/// `sigpending` so it returns immediately. The dequeued signum is
/// discarded; this is the close path, no one is left to observe it.
///
/// `trace` is the closing receiver's instance trace cell, threaded from
/// `rollback` for the `posix_trace` diagnostics below.
fn drain_pending_blocked(trace: &TraceCell, signals: &[libc::c_int]) {
    if signals.is_empty() {
        return;
    }
    let mut drain_set: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe { libc::sigemptyset(&mut drain_set) };
    for &s in signals {
        unsafe { libc::sigaddset(&mut drain_set, s) };
    }
    // Bounded loop — defends against a kernel that somehow keeps
    // re-queuing the same signal while we drain. We've never observed
    // more than 2 queued for a non-realtime signal in practice; 64 is
    // a generous ceiling.
    for _ in 0..64 {
        let mut pending: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe { libc::sigemptyset(&mut pending) };
        unsafe { libc::sigpending(&mut pending) };
        let any = signals
            .iter()
            .any(|&s| unsafe { libc::sigismember(&pending, s) } == 1);
        if !any {
            return;
        }
        let mut sig_dequeued: libc::c_int = 0;
        let ret = unsafe { libc::sigwait(&drain_set, &mut sig_dequeued) };
        posix_trace(
            trace,
            format_args!(
                "rollback: drained pending signum={} via sigwait (ret={})",
                sig_dequeued, ret
            ),
        );
        if ret != 0 {
            // sigwait shouldn't fail for a blocked, already-pending
            // signal. If it does, fall through to the unblock rather
            // than spinning.
            return;
        }
    }
    posix_trace(
        trace,
        format_args!(
            "rollback: drain loop hit ceiling for signals={:?}; pending instances may remain",
            signals
        ),
    );
}

// ── Linux: signalfd ────────────────────────────────────────────────────

#[cfg(any(target_os = "linux", target_os = "android"))]
mod linux;

// ── macOS: kqueue + EVFILT_SIGNAL ──────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos;

#[cfg(any(target_os = "linux", target_os = "android"))]
#[allow(unused_imports)]
pub(crate) use linux::SignalReceiver;
#[cfg(target_os = "macos")]
#[allow(unused_imports)]
pub(crate) use macos::SignalReceiver;

#[cfg(test)]
mod tests;
