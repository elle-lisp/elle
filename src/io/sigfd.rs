//! POSIX signal reception via signalfd (Linux) and kqueue (macOS).
//!
//! `SignalReceiver` is the External object behind `os/sig-watch`. Each
//! receiver owns a kernel file descriptor (signalfd on Linux, a dedicated
//! kqueue fd on macOS) that becomes readable when a watched signal is
//! delivered. The scheduler reads the fd via the same IoOp dispatch
//! machinery as filesystem watchers.
//!
//! ## Mask policy
//!
//! The kernel only queues a signal onto signalfd/kqueue if the signal is
//! blocked from default delivery in every thread that might otherwise
//! absorb it. A receiver therefore must block its target signals on the
//! main thread before opening the fd, and worker threads must mask all
//! signals so the kernel never selects them as the delivery target.
//!
//! A module-level [`WatchedSet`] holds the union of currently-blocked
//! signals and per-signal refcounts. `SignalReceiver::new` increments
//! refcounts (blocking each new signal as it crosses zero); `Drop`
//! decrements (unblocking when refcount returns to zero). Pending
//! instances in the kernel queue at the moment of unblock fire their
//! default disposition — preferred to silent swallowing. See
//! `docs/posix-signals.md` for the user-facing contract.

use crate::config::TraceCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Emit a `[trace:posix] …` line to stderr when `trace`'s owning instance has the
/// `posix` bit set (`--trace=posix`, `--trace=all`, or `(vm/config-set :trace …)`
/// at runtime). Used to triage POSIX-signal regressions — correlate these with the
/// per-test progress lines emitted by `tests/elle/posix.lisp` to pinpoint exactly
/// which kernel call diverges between Linux and macOS.
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

/// Process-wide signal trap installation, called exactly once from
/// `main()` before any worker thread spawns. Workers call
/// `mask_all_signals_on_this_thread` on entry and thereby inherit a
/// no-async-signal-delivery posture (the fault set stays deliverable);
/// the *main* thread is what this function
/// configures, and the policy below decides which signals get a
/// sigaction handler (delivered to the main thread by the kernel
/// because everyone else has them masked) and which are
/// `pthread_sigmask`-blocked on the main thread (queued by the kernel
/// for `signalfd`-style consumption).
///
/// ## Disposition table
///
/// | Set | Signals | What we do |
/// |-----|---------|------------|
/// | Terminate | TERM, INT, QUIT, HUP | `sigaction(SA_RESTART)` to a handler that writes a tagged line to stderr via `write(2)` and `_exit(128 + signum)`. The handler is async-signal-safe — no allocation, no Rust stdio, no locks. |
/// | Job control | TSTP, TTIN, TTOU | `sigaction` to a handler that calls `raise(SIGSTOP)`. The kernel stops the process; the shell can later `bg`/`fg` it. On `SIGCONT` the process resumes mid-handler and returns normally. |
/// | Resume | CONT | `sigaction` to an empty handler so the delivery is consumed and the kernel doesn't try anything else. (No state to clean up — io_uring + signalfd survive across SIGSTOP/SIGCONT untouched.) |
/// | Pipe | PIPE | `sigaction(SIG_IGN)`. Writes to broken pipes surface as `EPIPE`. |
/// | Absorb | USR1, USR2, CHLD, URG, WINCH, ALRM | `pthread_sigmask(SIG_BLOCK)` on the main thread. With every worker also masking on spawn, no thread has these unblocked, the kernel queues them, and nobody reads. They are silently absorbed unless a user `os/sig-watch` opens a `signalfd` to drain. |
/// | Fault | SEGV, BUS, FPE, ILL, ABRT, TRAP, SYS | Untouched. These are synchronous fault signals; intercepting them only obscures real bugs. The kernel default (core/term) runs. |
/// | Uncatchable | KILL, STOP | Kernel forbids touching these. Pass through. |
///
/// ## Watcher override semantics
///
/// A user `os/sig-watch :sigterm` (etc.) lazily `pthread_sigmask`-blocks
/// the watched signal on the main thread before opening its
/// per-receiver `signalfd`. With the main thread blocking the signal
/// and every worker thread already masking every asynchronous signal,
/// the kernel has
/// no delivery target — the sigaction handler installed here cannot
/// fire while a watcher is alive. The signalfd reads it instead.
/// When the last watcher closes, the lazy-block unblocks the signal,
/// the kernel can again pick the main thread, and the sigaction handler
/// re-arms. No explicit watcher-vs-builtin coordination logic in user
/// space — the kernel's delivery rules do it for free.
///
/// ## Counter-cases this defends against
///
/// 1. **Startup race**: a `SIGTERM` arriving between `config::init` and
///    the first `os/sig-watch` no longer kills the program — the
///    handler runs.
/// 2. **C-spawned thread inheritance**: Cranelift / FFI cdylib threads
///    inherit the main thread's startup mask. After this function,
///    that mask blocks the absorb-set, narrowing the window where a
///    rogue thread could absorb a signal the user intended to watch.
/// 3. **Accidental `kill -USR1`**: previously killed the process
///    (kernel default Term). Now absorbed by the startup mask.
///
/// Idempotent: safe to call multiple times. (`sigaction` overwrites the
/// previous handler; `pthread_sigmask(SIG_BLOCK)` is additive but the
/// set is constant.) Tests fork and call it in each child.
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
/// Worker masks therefore always exclude this set — the disposition
/// stays "Untouched" per docs/posix-signals.md § "Disposition table".
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
/// consumed (e.g. macOS test 5: two kill(SIGUSR1) calls, kqueue
/// `EVFILT_SIGNAL` reported count=2 but only one delivery actually
/// drained the process pending queue) would fire its now-restored
/// default disposition on `pthread_sigmask(SIG_UNBLOCK, …)` and
/// terminate the process mid-close. On Linux the situation is the same
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
