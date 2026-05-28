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

// ── Linux: signalfd ────────────────────────────────────────────────────

#[cfg(any(target_os = "linux", target_os = "android"))]
mod platform {
    use super::{watched_set, SigEvent};
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
                rollback(&signals);
                return Err(format!("os/sig-watch: signalfd: {}", err));
            }

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
                return;
            }
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
        let mut to_unblock: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe { libc::sigemptyset(&mut to_unblock) };
        let mut any = false;
        {
            let mut set = watched_set().lock().unwrap();
            for &s in signals {
                if let Some(c) = set.refcount.get_mut(&s) {
                    if *c > 0 {
                        *c -= 1;
                    }
                    if *c == 0 {
                        unsafe { libc::sigaddset(&mut to_unblock, s) };
                        any = true;
                    }
                }
            }
        }
        if any {
            unsafe {
                libc::pthread_sigmask(libc::SIG_UNBLOCK, &to_unblock, std::ptr::null_mut());
            }
        }
    }
}

// ── macOS: kqueue + EVFILT_SIGNAL ──────────────────────────────────────

#[cfg(target_os = "macos")]
mod platform {
    use super::{watched_set, SigEvent};
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

    impl SignalReceiver {
        pub fn new(signals: Vec<libc::c_int>) -> Result<Self, String> {
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
            unsafe {
                libc::pthread_sigmask(libc::SIG_BLOCK, &mask, std::ptr::null_mut());
            }

            let kq = unsafe { libc::kqueue() };
            if kq < 0 {
                let err = std::io::Error::last_os_error();
                rollback(&signals);
                return Err(format!("os/sig-watch: kqueue: {}", err));
            }
            unsafe { libc::fcntl(kq, libc::F_SETFD, libc::FD_CLOEXEC) };

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
                unsafe { libc::close(kq) };
                rollback(&signals);
                return Err(format!("os/sig-watch: kevent register: {}", err));
            }

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
                return;
            }
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
        let mut any = false;
        {
            let mut set = watched_set().lock().unwrap();
            for &s in signals {
                if let Some(c) = set.refcount.get_mut(&s) {
                    if *c > 0 {
                        *c -= 1;
                    }
                    if *c == 0 {
                        unsafe { libc::sigaddset(&mut to_unblock, s) };
                        any = true;
                    }
                }
            }
        }
        if any {
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

    #[test]
    fn refcount_blocks_and_unblocks() {
        // SIGURG is rarely touched by the runtime or other tests; safer
        // than SIGWINCH (which the parse_events test below also opens
        // a receiver for, racing against the WatchedSet refcount).
        let h = std::thread::spawn(|| {
            unsafe {
                let mut empty: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut empty);
                libc::pthread_sigmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
            }
            assert!(!current_thread_blocked().contains(&libc::SIGURG));

            let r1 = SignalReceiver::new(vec![libc::SIGURG]).unwrap();
            assert!(current_thread_blocked().contains(&libc::SIGURG));

            let r2 = SignalReceiver::new(vec![libc::SIGURG]).unwrap();
            assert!(current_thread_blocked().contains(&libc::SIGURG));

            r1.close();
            // Still blocked because r2 holds it.
            assert!(current_thread_blocked().contains(&libc::SIGURG));

            r2.close();
            // Both released — unblocked.
            assert!(!current_thread_blocked().contains(&libc::SIGURG));
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
}
