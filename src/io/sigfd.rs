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

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Emit a `[trace:posix] …` line to stderr when the `posix` trace bit
/// is active (set via `--trace=posix`, `--trace=all`, or `(vm/config)`
/// at runtime). Used to triage POSIX-signal regressions — correlate
/// these with the per-test progress lines emitted by
/// `tests/elle/posix.lisp` to pinpoint exactly which kernel call
/// diverges between Linux and macOS.
///
/// Gated on the process-global `crate::config::GLOBAL_TRACE_BITS`
/// mirror so threadpool worker threads and other off-VM callers (which
/// have no `&VM` and therefore can't use the per-VM `etrace!` macro)
/// can still gate cheaply.
///
/// Output goes via a direct `write(2, …)` syscall, bypassing the elle
/// scheduler and Rust's stdio buffering, so trace lines survive even
/// when the process is about to be killed by an outer timeout.
pub(crate) fn posix_trace(args: std::fmt::Arguments<'_>) {
    if !crate::config::global_trace_bit_enabled(crate::config::trace_bits::POSIX) {
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
/// no-signal-delivery posture; the *main* thread is what this function
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
/// and every worker thread already masking everything, the kernel has
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
    // Diagnostic: flush the `--trace=ioring` buffer before we die, so a hang
    // killed by a `timeout`/CI `SIGTERM` leaves its recent I/O-event tail.
    // No-op unless the `ioring` bit is set; async-signal-safe (try_lock +
    // bare write(2)) — see `crate::io::io_ring_dump`.
    crate::io::io_ring_dump();
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

/// Mask all maskable signals on the calling thread. Workers call this
/// as their first action after spawn so the kernel never selects them
/// as a signal delivery target. Must not be called on the main VM
/// thread (the lazy-block policy depends on the main thread starting
/// with an empty mask).
pub fn mask_all_signals_on_this_thread() {
    unsafe {
        let mut full: libc::sigset_t = std::mem::zeroed();
        libc::sigfillset(&mut full);
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
    let set = watched_set().lock().unwrap();
    let mut out: Vec<libc::c_int> = set
        .refcount
        .iter()
        .filter_map(|(s, c)| if *c > 0 { Some(*s) } else { None })
        .collect();
    out.sort_unstable();
    out
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
fn drain_pending_blocked(signals: &[libc::c_int]) {
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
        posix_trace(format_args!(
            "rollback: drained pending signum={} via sigwait (ret={})",
            sig_dequeued, ret
        ));
        if ret != 0 {
            // sigwait shouldn't fail for a blocked, already-pending
            // signal. If it does, fall through to the unblock rather
            // than spinning.
            return;
        }
    }
    posix_trace(format_args!(
        "rollback: drain loop hit ceiling for signals={:?}; pending instances may remain",
        signals
    ));
}

// ── Linux: signalfd ────────────────────────────────────────────────────

#[cfg(any(target_os = "linux", target_os = "android"))]
mod platform {
    use super::{drain_pending_blocked, posix_trace, watched_set, SigEvent};
    use std::cell::RefCell;
    use std::os::unix::io::RawFd;

    pub(crate) struct SignalReceiver {
        inner: RefCell<SignalReceiverInner>,
    }

    struct SignalReceiverInner {
        fd: RawFd,
        signals: Vec<libc::c_int>,
        closed: bool,
    }

    impl SignalReceiver {
        pub fn new(signals: Vec<libc::c_int>) -> Result<Self, String> {
            posix_trace(format_args!(
                "linux: SignalReceiver::new signals={:?}",
                signals
            ));
            // Reject sigkill/sigstop — kernel forbids blocking them.
            for &s in &signals {
                if s == libc::SIGKILL || s == libc::SIGSTOP {
                    return Err(format!(
                        "os/sig-watch: signal {} cannot be watched (kernel forbids blocking it)",
                        s
                    ));
                }
            }

            // Bump refcounts and pthread_sigmask-block any signal whose
            // refcount transitions 0 -> 1. Done under the watched-set
            // mutex so concurrent watchers can't race.
            let mut mask: libc::sigset_t = unsafe { std::mem::zeroed() };
            unsafe { libc::sigemptyset(&mut mask) };
            {
                let mut set = watched_set().lock().unwrap();
                for &s in &signals {
                    let entry = set.refcount.entry(s).or_insert(0);
                    if *entry == 0 {
                        unsafe { libc::sigaddset(&mut mask, s) };
                    }
                    *entry += 1;
                }
            }
            // Block the newly-added signals on the current thread.
            unsafe {
                libc::pthread_sigmask(libc::SIG_BLOCK, &mask, std::ptr::null_mut());
            }
            posix_trace(format_args!(
                "linux: blocked newly-watched signals on main thread"
            ));

            // Build the signalfd mask: all watched signals (including
            // ones that were already blocked by some other receiver).
            let mut sfd_mask: libc::sigset_t = unsafe { std::mem::zeroed() };
            unsafe { libc::sigemptyset(&mut sfd_mask) };
            for &s in &signals {
                unsafe { libc::sigaddset(&mut sfd_mask, s) };
            }
            let fd =
                unsafe { libc::signalfd(-1, &sfd_mask, libc::SFD_CLOEXEC | libc::SFD_NONBLOCK) };
            if fd < 0 {
                // Roll back refcounts and unblock the signals we just blocked.
                let err = std::io::Error::last_os_error();
                posix_trace(format_args!("linux: signalfd() failed: {}", err));
                rollback(&signals);
                return Err(format!("os/sig-watch: signalfd: {}", err));
            }
            posix_trace(format_args!(
                "linux: signalfd opened fd={} for {:?}",
                fd, signals
            ));

            Ok(SignalReceiver {
                inner: RefCell::new(SignalReceiverInner {
                    fd,
                    signals,
                    closed: false,
                }),
            })
        }

        pub fn raw_fd(&self) -> Result<RawFd, String> {
            let inner = self.inner.borrow();
            if inner.closed {
                return Err("os/sig-next: receiver is closed".into());
            }
            Ok(inner.fd)
        }

        #[allow(dead_code)]
        pub fn signals(&self) -> Vec<libc::c_int> {
            self.inner.borrow().signals.clone()
        }

        pub fn close(&self) {
            let mut inner = self.inner.borrow_mut();
            if inner.closed {
                posix_trace(format_args!(
                    "linux: SignalReceiver::close already closed (idempotent)"
                ));
                return;
            }
            posix_trace(format_args!(
                "linux: SignalReceiver::close fd={} signals={:?}",
                inner.fd, inner.signals
            ));
            unsafe { libc::close(inner.fd) };
            inner.closed = true;
            rollback(&inner.signals);
            inner.signals.clear();
        }

        /// Parse a buffer of signalfd_siginfo structs into SigEvents.
        /// signalfd writes one struct per delivered signal.
        pub fn parse_events(&self, buf: &[u8]) -> Vec<SigEvent> {
            let entry_size = std::mem::size_of::<libc::signalfd_siginfo>();
            let mut events = Vec::new();
            let mut offset = 0;
            while offset + entry_size <= buf.len() {
                let raw = unsafe { &*(buf.as_ptr().add(offset) as *const libc::signalfd_siginfo) };
                events.push(SigEvent {
                    signum: raw.ssi_signo as libc::c_int,
                    sender_pid: Some(raw.ssi_pid),
                    sender_uid: Some(raw.ssi_uid),
                    code: raw.ssi_code,
                    count: 1,
                });
                offset += entry_size;
            }
            events
        }
    }

    impl Drop for SignalReceiver {
        fn drop(&mut self) {
            let mut inner = self.inner.borrow_mut();
            if !inner.closed {
                unsafe { libc::close(inner.fd) };
                inner.closed = true;
                rollback(&inner.signals);
            }
        }
    }

    impl std::fmt::Debug for SignalReceiver {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let inner = self.inner.borrow();
            write!(
                f,
                "SignalReceiver(fd={}, signals={:?}, closed={})",
                inner.fd, inner.signals, inner.closed
            )
        }
    }

    fn rollback(signals: &[libc::c_int]) {
        // Decrement refcounts; collect signals whose refcount fell to
        // zero so we can pthread_sigmask-unblock them.
        //
        // Signals in the eager-trap absorb set
        // (`crate::io::sigfd::ABSORB_SET`) are intentionally blocked
        // process-wide by `init_process_signals`. The rollback must
        // NOT unblock them — otherwise an external `kill -USR1`
        // arriving between close and process exit would be delivered
        // by the kernel to the main thread (the only thread without
        // the signal masked), find no sigaction handler, and run the
        // kernel default disposition (Term for USR1/USR2/ALRM). We
        // still drain any pending instances before returning so the
        // signalfd close doesn't leave them dangling, but we leave
        // the mask bit set.
        let mut to_unblock: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe { libc::sigemptyset(&mut to_unblock) };
        let mut newly_freed_drainable: Vec<libc::c_int> = Vec::new();
        let mut newly_freed_unblockable: Vec<libc::c_int> = Vec::new();
        {
            let mut set = watched_set().lock().unwrap();
            for &s in signals {
                if let Some(c) = set.refcount.get_mut(&s) {
                    if *c > 0 {
                        *c -= 1;
                    }
                    if *c == 0 {
                        newly_freed_drainable.push(s);
                        if !super::ABSORB_SET.contains(&s) {
                            unsafe { libc::sigaddset(&mut to_unblock, s) };
                            newly_freed_unblockable.push(s);
                        }
                    }
                }
            }
        }
        if !newly_freed_drainable.is_empty() {
            // Drain pending watched signals BEFORE any (selective)
            // unblock. If a watcher closes with signals still queued
            // (signalfd was never read for them, or the user `kill`d
            // after the last sig-next), the unblock — for the
            // unblockable subset — would otherwise fire each one's
            // default disposition on this thread.  Absorb-set
            // signals also benefit from the drain: leaving instances
            // queued is harmless on its own, but a future user
            // os/sig-watch on the same signum would observe stale
            // pending state. See `drain_pending_blocked`.
            posix_trace(format_args!(
                "linux: rollback draining newly_freed={:?}; unblocking subset {:?}",
                newly_freed_drainable, newly_freed_unblockable
            ));
            drain_pending_blocked(&newly_freed_drainable);
            if !newly_freed_unblockable.is_empty() {
                unsafe {
                    libc::pthread_sigmask(libc::SIG_UNBLOCK, &to_unblock, std::ptr::null_mut());
                }
            }
        }
    }
}

// ── macOS: kqueue + EVFILT_SIGNAL ──────────────────────────────────────

#[cfg(target_os = "macos")]
mod platform {
    use super::{drain_pending_blocked, posix_trace, saved_dispositions, watched_set, SigEvent};
    use std::cell::RefCell;
    use std::os::unix::io::RawFd;

    pub(crate) struct SignalReceiver {
        inner: RefCell<SignalReceiverInner>,
    }

    struct SignalReceiverInner {
        kq: RawFd,
        signals: Vec<libc::c_int>,
        closed: bool,
    }

    /// No-op signal handler installed on the watched signals so kqueue's
    /// `EVFILT_SIGNAL` can fire without the kernel running the default
    /// disposition (Term for SIGUSR1, etc.). `kq_sig_read_blocking` in
    /// `src/io/threadpool.rs` unmasks the watched signals on its own
    /// thread; the kernel then picks that thread for delivery and runs
    /// this no-op before `kevent()` returns.
    extern "C" fn noop_handler(_signum: libc::c_int) {}

    /// Build a `sigaction` that points at `noop_handler` with SA_RESTART
    /// so the no-op delivery doesn't surface as EINTR on long-running
    /// syscalls elsewhere in the process. Empty `sa_mask` — we don't
    /// want to compound the watcher mask while the handler runs.
    fn noop_sigaction() -> libc::sigaction {
        let mut sa: libc::sigaction = unsafe { std::mem::zeroed() };
        sa.sa_sigaction = noop_handler as *const () as libc::sighandler_t;
        sa.sa_flags = libc::SA_RESTART;
        unsafe { libc::sigemptyset(&mut sa.sa_mask) };
        sa
    }

    impl SignalReceiver {
        pub fn new(signals: Vec<libc::c_int>) -> Result<Self, String> {
            posix_trace(format_args!(
                "macos: SignalReceiver::new signals={:?}",
                signals
            ));
            for &s in &signals {
                if s == libc::SIGKILL || s == libc::SIGSTOP {
                    return Err(format!(
                        "os/sig-watch: signal {} cannot be watched (kernel forbids blocking it)",
                        s
                    ));
                }
            }

            let mut mask: libc::sigset_t = unsafe { std::mem::zeroed() };
            unsafe { libc::sigemptyset(&mut mask) };
            // Track which signals transitioned 0 → 1 so we install the
            // no-op handler exactly once per signal, holding the
            // `WatchedSet` lock for the duration to serialise with
            // concurrent `SignalReceiver::new` / drop on other receivers.
            let mut newly_watched: Vec<libc::c_int> = Vec::new();
            {
                let mut set = watched_set().lock().unwrap();
                for &s in &signals {
                    let entry = set.refcount.entry(s).or_insert(0);
                    if *entry == 0 {
                        unsafe { libc::sigaddset(&mut mask, s) };
                        newly_watched.push(s);
                    }
                    *entry += 1;
                }
            }
            unsafe {
                libc::pthread_sigmask(libc::SIG_BLOCK, &mask, std::ptr::null_mut());
            }
            posix_trace(format_args!(
                "macos: pthread_sigmask blocked newly_watched={:?} on main thread",
                newly_watched
            ));

            // Install the no-op handler on each newly-watched signal,
            // saving the old disposition so `rollback` can restore it
            // when the refcount drops back to zero.
            //
            // sigaction is process-wide; both the install here and the
            // unblock done by `kq_sig_read_blocking` are required for
            // EVFILT_SIGNAL to fire (delivery requires both: a thread
            // with the signal unmasked, and a handler that doesn't
            // terminate the process).
            if !newly_watched.is_empty() {
                let new_sa = noop_sigaction();
                let mut saved = saved_dispositions().lock().unwrap();
                for &s in &newly_watched {
                    let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
                    let ret = unsafe { libc::sigaction(s, &new_sa, &mut old) };
                    if ret == 0 {
                        saved.insert(s, old);
                        posix_trace(format_args!(
                            "macos: installed no-op sigaction for signum={}",
                            s
                        ));
                    } else {
                        let err = std::io::Error::last_os_error();
                        posix_trace(format_args!(
                            "macos: sigaction install FAILED for signum={}: {} — kqueue may not fire",
                            s,
                            err
                        ));
                    }
                    // sigaction failure on a watchable signal is rare;
                    // fall back to whatever disposition was already in
                    // place. EVFILT_SIGNAL will still fire if a user
                    // handler exists, which is acceptable here.
                }
            }

            let kq = unsafe { libc::kqueue() };
            if kq < 0 {
                let err = std::io::Error::last_os_error();
                posix_trace(format_args!("macos: kqueue() failed: {}", err));
                rollback(&signals);
                return Err(format!("os/sig-watch: kqueue: {}", err));
            }
            unsafe { libc::fcntl(kq, libc::F_SETFD, libc::FD_CLOEXEC) };
            posix_trace(format_args!("macos: kqueue() opened kq={}", kq));

            // Register one EVFILT_SIGNAL filter per signal.
            let changelist: Vec<libc::kevent> = signals
                .iter()
                .map(|&s| libc::kevent {
                    ident: s as libc::uintptr_t,
                    filter: libc::EVFILT_SIGNAL,
                    flags: libc::EV_ADD | libc::EV_CLEAR,
                    fflags: 0,
                    data: 0,
                    udata: std::ptr::null_mut(),
                })
                .collect();
            let ret = unsafe {
                libc::kevent(
                    kq,
                    changelist.as_ptr(),
                    changelist.len() as libc::c_int,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                )
            };
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                posix_trace(format_args!(
                    "macos: kevent EV_ADD EVFILT_SIGNAL FAILED for {:?}: {}",
                    signals, err
                ));
                unsafe { libc::close(kq) };
                rollback(&signals);
                return Err(format!("os/sig-watch: kevent register: {}", err));
            }
            posix_trace(format_args!(
                "macos: kevent EV_ADD EVFILT_SIGNAL registered {:?} on kq={}",
                signals, kq
            ));

            Ok(SignalReceiver {
                inner: RefCell::new(SignalReceiverInner {
                    kq,
                    signals,
                    closed: false,
                }),
            })
        }

        pub fn raw_fd(&self) -> Result<RawFd, String> {
            let inner = self.inner.borrow();
            if inner.closed {
                return Err("os/sig-next: receiver is closed".into());
            }
            Ok(inner.kq)
        }

        #[allow(dead_code)]
        pub fn signals(&self) -> Vec<libc::c_int> {
            self.inner.borrow().signals.clone()
        }

        pub fn close(&self) {
            let mut inner = self.inner.borrow_mut();
            if inner.closed {
                posix_trace(format_args!(
                    "macos: SignalReceiver::close already closed (idempotent)"
                ));
                return;
            }
            posix_trace(format_args!(
                "macos: SignalReceiver::close kq={} signals={:?}",
                inner.kq, inner.signals
            ));
            unsafe { libc::close(inner.kq) };
            inner.closed = true;
            rollback(&inner.signals);
            inner.signals.clear();
        }

        /// macOS encodes kevent results as a sequence of (ident:i32, data:u32)
        /// LE pairs in `buf` (written by the threadpool worker after a
        /// blocking `kevent()` call).
        pub fn parse_events(&self, buf: &[u8]) -> Vec<SigEvent> {
            let entry_size = 4 + 4;
            let mut events = Vec::new();
            let mut offset = 0;
            while offset + entry_size <= buf.len() {
                let signum = i32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
                let count = u32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap());
                events.push(SigEvent {
                    signum,
                    sender_pid: None,
                    sender_uid: None,
                    code: 0,
                    count,
                });
                offset += entry_size;
            }
            events
        }
    }

    impl Drop for SignalReceiver {
        fn drop(&mut self) {
            let mut inner = self.inner.borrow_mut();
            if !inner.closed {
                unsafe { libc::close(inner.kq) };
                inner.closed = true;
                rollback(&inner.signals);
            }
        }
    }

    impl std::fmt::Debug for SignalReceiver {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let inner = self.inner.borrow();
            write!(
                f,
                "SignalReceiver(kq={}, signals={:?}, closed={})",
                inner.kq, inner.signals, inner.closed
            )
        }
    }

    fn rollback(signals: &[libc::c_int]) {
        let mut to_unblock: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe { libc::sigemptyset(&mut to_unblock) };
        // Collect signals whose refcount fell to zero so we can both
        // restore their saved sigaction and unblock them on the calling
        // thread. We have to drop the WatchedSet lock before doing the
        // sigaction restore to avoid holding two global locks in series.
        //
        // Signals in `crate::io::sigfd::ABSORB_SET` are kept masked at
        // process scope by `init_process_signals`; the rollback must
        // not unblock them on close even when refcount hits zero. On
        // macOS the leak that would otherwise happen is identical to
        // Linux: a future `kill -USR1 $pid` would, after the close,
        // find the kqueue no-op handler restored to default (Term)
        // *and* the main-thread mask cleared, and terminate the
        // process. We still drain + sigaction-restore for absorb-set
        // signums; we just skip the pthread_sigmask unblock.
        let mut newly_freed: Vec<libc::c_int> = Vec::new();
        let mut newly_freed_unblockable: Vec<libc::c_int> = Vec::new();
        {
            let mut set = watched_set().lock().unwrap();
            for &s in signals {
                if let Some(c) = set.refcount.get_mut(&s) {
                    if *c > 0 {
                        *c -= 1;
                    }
                    if *c == 0 {
                        newly_freed.push(s);
                        if !super::ABSORB_SET.contains(&s) {
                            unsafe { libc::sigaddset(&mut to_unblock, s) };
                            newly_freed_unblockable.push(s);
                        }
                    }
                }
            }
        }
        if newly_freed.is_empty() {
            return;
        }
        // Drain pending watched signals via sigwait BEFORE we restore
        // the saved disposition or (selectively) unblock. macOS's
        // EVFILT_SIGNAL only counts kill() generations on the knote —
        // it does NOT consume from the process pending queue — and
        // the kqueue worker's brief pthread_sigmask SIG_UNBLOCK +
        // no-op-handler delivery only drains at most one queued
        // instance. Test 5 (two kill(SIGUSR1) calls in a row, one
        // sig-next read reporting count=2) leaves a SIGUSR1 in the
        // pending queue even after the worker reports the event.
        // Restoring the default disposition (Term for SIGUSR1) and
        // then unblocking would deliver that orphan to this thread
        // with its newly-restored default and kill the process
        // mid-close. sigwait consumes pending instances without
        // invoking the (still no-op) handler. Done while the signals
        // are still blocked, so sigwait returns immediately for
        // already-pending entries.
        posix_trace(format_args!(
            "macos: rollback draining newly_freed={:?}; unblocking subset {:?}",
            newly_freed, newly_freed_unblockable
        ));
        drain_pending_blocked(&newly_freed);
        // Restore the saved sigactions, then unblock the subset of
        // signums that should re-arm their kernel default. Order
        // matters: any signal generated AFTER the unblock should
        // fire the user's original disposition (typically the
        // default), not our no-op.
        {
            let mut saved = saved_dispositions().lock().unwrap();
            for &s in &newly_freed {
                if let Some(old) = saved.remove(&s) {
                    unsafe { libc::sigaction(s, &old, std::ptr::null_mut()) };
                    posix_trace(format_args!(
                        "macos: rollback restored sigaction for signum={}",
                        s
                    ));
                }
            }
        }
        if !newly_freed_unblockable.is_empty() {
            unsafe {
                libc::pthread_sigmask(libc::SIG_UNBLOCK, &to_unblock, std::ptr::null_mut());
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
#[allow(unused_imports)]
pub(crate) use platform::SignalReceiver;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_thread_blocked_starts_empty_or_known() {
        // We can only assert this on a freshly-spawned thread because the
        // main thread inherits whatever the test runner set up. Run on a
        // thread with an explicitly-empty mask.
        let h = std::thread::spawn(|| {
            unsafe {
                let mut empty: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut empty);
                libc::pthread_sigmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
            }
            current_thread_blocked()
        });
        let blocked = h.join().unwrap();
        assert!(blocked.is_empty(), "fresh thread starts with empty mask");
    }

    #[test]
    fn mask_all_signals_blocks_everything_on_this_thread() {
        let h = std::thread::spawn(|| {
            mask_all_signals_on_this_thread();
            current_thread_blocked()
        });
        let blocked = h.join().unwrap();
        // sigkill and sigstop can't be blocked even via SIG_SETMASK with a
        // full set — the kernel silently strips them. Everything else
        // should be present.
        assert!(blocked.contains(&libc::SIGTERM), "SIGTERM blocked");
        assert!(blocked.contains(&libc::SIGUSR1), "SIGUSR1 blocked");
        assert!(blocked.contains(&libc::SIGUSR2), "SIGUSR2 blocked");
        assert!(blocked.contains(&libc::SIGINT), "SIGINT blocked");
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn parse_events_decodes_synthesized_siginfo() {
        // Build a fake signalfd_siginfo by hand.
        let mut buf = vec![0u8; std::mem::size_of::<libc::signalfd_siginfo>() * 2];
        let entry_size = std::mem::size_of::<libc::signalfd_siginfo>();
        unsafe {
            let p0 = buf.as_mut_ptr() as *mut libc::signalfd_siginfo;
            (*p0).ssi_signo = libc::SIGUSR1 as u32;
            (*p0).ssi_pid = 4242;
            (*p0).ssi_uid = 1000;
            (*p0).ssi_code = 0;
            let p1 = (buf.as_mut_ptr().add(entry_size)) as *mut libc::signalfd_siginfo;
            (*p1).ssi_signo = libc::SIGCHLD as u32;
            (*p1).ssi_pid = 5151;
            (*p1).ssi_uid = 1000;
            (*p1).ssi_code = 1; // CLD_EXITED
        }
        // Create a SignalReceiver just to call parse_events; the fd it
        // opens is real but we don't read from it.
        let r = SignalReceiver::new(vec![libc::SIGWINCH]).expect("receiver");
        let events = r.parse_events(&buf);
        r.close();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].signum, libc::SIGUSR1);
        assert_eq!(events[0].sender_pid, Some(4242));
        assert_eq!(events[1].signum, libc::SIGCHLD);
        assert_eq!(events[1].code, 1);
    }

    /// Refcount accounting + absorb-set unblock suppression. SIGURG is
    /// in the eager-trap absorb set (`ABSORB_SET`) so once a watcher
    /// blocks it, close-time rollback intentionally does NOT unblock —
    /// otherwise the kernel default (Term) would be reachable on the
    /// main thread after the last watcher closes. The refcount itself
    /// transitions correctly (0 → 1 → 2 → 1 → 0) and is observable
    /// via `currently_watched`; only the mask bit is sticky.
    ///
    /// SIGURG is rarely touched by the runtime or other tests; safer
    /// than SIGWINCH (which the parse_events test below also opens
    /// a receiver for, racing against the WatchedSet refcount).
    #[test]
    fn refcount_block_while_watched_absorb_set_stays_blocked_after_close() {
        let h = std::thread::spawn(|| {
            unsafe {
                let mut empty: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut empty);
                libc::pthread_sigmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
            }
            assert!(!current_thread_blocked().contains(&libc::SIGURG));

            let r1 = SignalReceiver::new(vec![libc::SIGURG]).unwrap();
            assert!(current_thread_blocked().contains(&libc::SIGURG));
            assert!(currently_watched().contains(&libc::SIGURG));

            let r2 = SignalReceiver::new(vec![libc::SIGURG]).unwrap();
            assert!(current_thread_blocked().contains(&libc::SIGURG));

            r1.close();
            // Still blocked because r2 holds the refcount > 0.
            assert!(current_thread_blocked().contains(&libc::SIGURG));
            assert!(currently_watched().contains(&libc::SIGURG));

            r2.close();
            // Refcount transitioned 1 → 0, but SIGURG is in ABSORB_SET
            // so rollback skips the unblock — see `rollback` in this
            // file. The mask bit stays set; `currently_watched` flips
            // off as the source of truth for "is anyone watching this?".
            assert!(
                current_thread_blocked().contains(&libc::SIGURG),
                "ABSORB_SET signal must stay masked after last close"
            );
            assert!(!currently_watched().contains(&libc::SIGURG));
        });
        h.join().unwrap();
    }

    #[test]
    fn cannot_watch_sigkill() {
        let r = SignalReceiver::new(vec![libc::SIGKILL]);
        assert!(r.is_err());
    }

    #[test]
    fn cannot_watch_sigstop() {
        let r = SignalReceiver::new(vec![libc::SIGSTOP]);
        assert!(r.is_err());
    }

    // ── Eager-trap (init_process_signals) regression tests ──────────────
    //
    // These tests fork because the test runner has many peer threads with
    // various signal masks and unmasked signals would be absorbed by
    // them before `init_process_signals` could install the disposition we
    // want to assert. Inside the forked child there is exactly one thread,
    // making the kernel's signal-delivery target deterministic.
    //
    // All five tests follow the same shape:
    //
    //   1. fork().
    //   2. Child calls `init_process_signals()` to install the eager
    //      sigaction handlers + the absorb-set mask.
    //   3. Child triggers the scenario (e.g. raise(SIGTERM)).
    //   4. Parent observes the child via waitpid and asserts on the
    //      exit status.
    //
    // The exit codes are conventional `128 + signum` for terminate-class
    // signals — matches what the sigaction handlers will encode in their
    // `_exit()` call. A `WIFSIGNALED` parent status means the kernel
    // default fired instead of our handler — that's the regression we're
    // pinning against.

    /// Run `child_logic` in a forked child with a `deadline_secs` timeout.
    /// Returns the child's exit status struct so individual tests can
    /// assert on it without each one reimplementing the fork dance.
    fn fork_run(deadline_secs: u64, child_logic: fn() -> i32) -> libc::c_int {
        use std::time::{Duration, Instant};
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            panic!("fork failed: {}", std::io::Error::last_os_error());
        }
        if pid == 0 {
            let code = child_logic();
            unsafe { libc::_exit(code) };
        }
        let deadline = Instant::now() + Duration::from_secs(deadline_secs);
        let mut status: libc::c_int = 0;
        loop {
            let wret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if wret == pid {
                return status;
            }
            if wret < 0 {
                panic!("waitpid({}): {}", pid, std::io::Error::last_os_error());
            }
            if Instant::now() >= deadline {
                unsafe { libc::kill(pid, libc::SIGKILL) };
                let _ = unsafe { libc::waitpid(pid, &mut status, 0) };
                panic!("fork_run child hung past {}s", deadline_secs);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// SIGTERM must run the built-in sigaction handler and produce a
    /// clean `WIFEXITED == 143` (= `128 + SIGTERM`), not a
    /// `WIFSIGNALED` death. Counter-factual: comment out the SIGTERM
    /// branch in `install_terminate_handlers` and this test fails
    /// because the kernel default (Term) fires instead.
    #[test]
    fn sigterm_terminates_via_handler_with_code_143() {
        let status = fork_run(5, || {
            init_process_signals();
            unsafe { libc::raise(libc::SIGTERM) };
            // The handler should have called _exit before we get here.
            // If we reach this line the handler is broken — return a
            // distinguishable code so the parent's assertion message
            // makes it clear.
            std::thread::sleep(std::time::Duration::from_millis(200));
            91
        });
        assert!(
            !libc::WIFSIGNALED(status),
            "child died from signal {} — sigaction handler did not fire",
            libc::WTERMSIG(status),
        );
        assert!(libc::WIFEXITED(status));
        assert_eq!(
            libc::WEXITSTATUS(status),
            128 + libc::SIGTERM,
            "SIGTERM handler must _exit(128 + SIGTERM)"
        );
    }

    /// Same as SIGTERM but for SIGINT / SIGQUIT / SIGHUP — they all
    /// share the terminate-class dispatch.
    #[test]
    fn sigint_terminates_via_handler_with_code_130() {
        let status = fork_run(5, || {
            init_process_signals();
            unsafe { libc::raise(libc::SIGINT) };
            std::thread::sleep(std::time::Duration::from_millis(200));
            91
        });
        assert!(!libc::WIFSIGNALED(status));
        assert_eq!(libc::WEXITSTATUS(status), 128 + libc::SIGINT);
    }

    #[test]
    fn sigquit_terminates_via_handler_with_code_131() {
        let status = fork_run(5, || {
            init_process_signals();
            unsafe { libc::raise(libc::SIGQUIT) };
            std::thread::sleep(std::time::Duration::from_millis(200));
            91
        });
        assert!(!libc::WIFSIGNALED(status));
        assert_eq!(libc::WEXITSTATUS(status), 128 + libc::SIGQUIT);
    }

    #[test]
    fn sighup_terminates_via_handler_with_code_129() {
        let status = fork_run(5, || {
            init_process_signals();
            unsafe { libc::raise(libc::SIGHUP) };
            std::thread::sleep(std::time::Duration::from_millis(200));
            91
        });
        assert!(!libc::WIFSIGNALED(status));
        assert_eq!(libc::WEXITSTATUS(status), 128 + libc::SIGHUP);
    }

    /// SIGTSTP must translate into a `raise(SIGSTOP)` from the handler,
    /// the kernel stops the process, and after the parent sends
    /// SIGCONT execution resumes. We verify resumption by having the
    /// child write a sentinel to a pipe AFTER the SIGTSTP raise: if
    /// the sigaction handler is missing, the kernel default for
    /// SIGTSTP (also Stop, but no sigaction means we never write the
    /// sentinel because Continue-only-on-cont still works); to make
    /// this a sharp test we instead assert the child exits 0 within
    /// the timeout AFTER parent sends SIGCONT — without the SIGCONT
    /// the child would hang in the kernel-imposed stop and the
    /// timeout would fire.
    ///
    /// Counter-factual: with no SIGTSTP handler we'd still get a
    /// kernel-imposed stop (same observed result). So this test
    /// uniquely pins SIGTSTP → SIGSTOP-via-handler only when paired
    /// with the next test (sigtstp_handler_returns_to_caller).
    #[test]
    fn sigtstp_pauses_and_sigcont_resumes() {
        use std::time::{Duration, Instant};
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            panic!("fork: {}", std::io::Error::last_os_error());
        }
        if pid == 0 {
            init_process_signals();
            unsafe { libc::raise(libc::SIGTSTP) };
            // After SIGCONT we should reach this line and exit cleanly.
            unsafe { libc::_exit(0) };
        }

        // Parent: wait until child is stopped, then SIGCONT it.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut status: libc::c_int = 0;
        let mut stopped = false;
        loop {
            let wret = unsafe { libc::waitpid(pid, &mut status, libc::WUNTRACED | libc::WNOHANG) };
            if wret == pid {
                if libc::WIFSTOPPED(status) && !stopped {
                    // Now resume it.
                    stopped = true;
                    unsafe { libc::kill(pid, libc::SIGCONT) };
                } else if libc::WIFEXITED(status) {
                    break;
                }
            } else if wret < 0 {
                panic!("waitpid: {}", std::io::Error::last_os_error());
            }
            if Instant::now() >= deadline {
                unsafe { libc::kill(pid, libc::SIGKILL) };
                let _ = unsafe { libc::waitpid(pid, &mut status, 0) };
                panic!("child hung in stop state past 5s");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);
        assert!(
            stopped,
            "child must have transitioned through stopped state"
        );
    }

    /// SIGPIPE must be installed as SIG_IGN at startup. Write on a
    /// closed pipe should return -1/EPIPE rather than terminating
    /// the process.
    ///
    /// Counter-factual: removing the SIG_IGN install would cause the
    /// child to die with SIGPIPE (WIFSIGNALED, WTERMSIG=SIGPIPE).
    #[test]
    fn sigpipe_is_ignored_at_startup() {
        let status = fork_run(5, || {
            init_process_signals();
            let mut fds: [libc::c_int; 2] = [0; 2];
            if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
                return 71;
            }
            // Close the read end first.
            unsafe { libc::close(fds[0]) };
            let buf = [0u8; 4];
            let ret =
                unsafe { libc::write(fds[1], buf.as_ptr() as *const libc::c_void, buf.len()) };
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            unsafe { libc::close(fds[1]) };
            if ret == -1 && errno == libc::EPIPE {
                0
            } else {
                72
            }
        });
        assert!(
            !libc::WIFSIGNALED(status),
            "child died from signal {} — SIGPIPE not ignored",
            libc::WTERMSIG(status),
        );
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);
    }

    /// When nothing watches SIGUSR1, the eager-trap policy is "absorb":
    /// the signal is blocked at startup, the kernel queues it but no
    /// thread reads it, and the process keeps running. Counter-factual:
    /// without the absorb-set block, the kernel default for SIGUSR1
    /// (Term) fires and the child dies signalled.
    #[test]
    fn sigusr1_absorbed_when_unwatched() {
        let status = fork_run(5, || {
            init_process_signals();
            unsafe { libc::raise(libc::SIGUSR1) };
            // Give the kernel a beat to deliver if it were going to.
            std::thread::sleep(std::time::Duration::from_millis(100));
            0
        });
        assert!(
            !libc::WIFSIGNALED(status),
            "child died from signal {} — SIGUSR1 was not absorbed",
            libc::WTERMSIG(status),
        );
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);
    }

    /// Watcher overrides built-in: a live `SignalReceiver` on SIGTERM
    /// must keep the process alive even after SIGTERM is raised — the
    /// signal goes to the receiver's signalfd instead of the sigaction
    /// handler. Counter-factual: without the watcher-override
    /// mechanism (i.e. if the sigaction handler fires regardless), the
    /// child terminates with code 143 before it can read the receiver.
    #[test]
    fn watcher_overrides_builtin_for_sigterm() {
        let status = fork_run(5, || {
            init_process_signals();
            let r = match SignalReceiver::new(vec![libc::SIGTERM]) {
                Ok(r) => r,
                Err(_) => return 81,
            };
            unsafe { libc::raise(libc::SIGTERM) };
            // Poll signalfd directly for the event — we don't need the
            // full async-backend pipeline here.
            let fd = match r.raw_fd() {
                Ok(f) => f,
                Err(_) => return 82,
            };
            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let pret = unsafe { libc::poll(&mut pfd, 1, 1000) };
            if pret <= 0 {
                return 83;
            }
            let mut buf = vec![0u8; std::mem::size_of::<libc::signalfd_siginfo>() * 4];
            let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n <= 0 {
                return 84;
            }
            buf.truncate(n as usize);
            let events = r.parse_events(&buf);
            if events.is_empty() || events[0].signum != libc::SIGTERM {
                return 85;
            }
            r.close();
            0
        });
        assert!(
            !libc::WIFSIGNALED(status),
            "child died from signal {} — watcher did not override built-in handler",
            libc::WTERMSIG(status),
        );
        assert!(libc::WIFEXITED(status));
        assert_eq!(
            libc::WEXITSTATUS(status),
            0,
            "child exited with {} (see codes 81-85 in test)",
            libc::WEXITSTATUS(status)
        );
    }
}
