use super::{drain_pending_blocked, posix_trace, saved_dispositions, with_watched_set, SigEvent};
use std::cell::RefCell;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};

pub(crate) struct SignalReceiver {
    inner: RefCell<SignalReceiverInner>,
}

struct SignalReceiverInner {
    /// The kqueue. `None` once the receiver is closed; the `OwnedFd`
    /// closes the descriptor on drop, so there is no manual `close`
    /// call and no risk of a double-close.
    kq: Option<OwnedFd>,
    signals: Vec<libc::c_int>,
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
        with_watched_set(|set| {
            for &s in &signals {
                let entry = set.refcount.entry(s).or_insert(0);
                if *entry == 0 {
                    unsafe { libc::sigaddset(&mut mask, s) };
                    newly_watched.push(s);
                }
                *entry += 1;
            }
        });
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
                        s, err
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
        // SAFETY: fresh kqueue fd we own. Holding it as an OwnedFd means
        // the early return below (kevent failure) closes it for us.
        let kq = unsafe { OwnedFd::from_raw_fd(kq) };
        unsafe { libc::fcntl(kq.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
        posix_trace(format_args!("macos: kqueue() opened kq={}", kq.as_raw_fd()));

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
                kq.as_raw_fd(),
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
            // `kq` (still an OwnedFd) drops here on return, closing it.
            rollback(&signals);
            return Err(format!("os/sig-watch: kevent register: {}", err));
        }
        posix_trace(format_args!(
            "macos: kevent EV_ADD EVFILT_SIGNAL registered {:?} on kq={}",
            signals,
            kq.as_raw_fd()
        ));

        Ok(SignalReceiver {
            inner: RefCell::new(SignalReceiverInner {
                kq: Some(kq),
                signals,
            }),
        })
    }

    pub fn raw_fd(&self) -> Result<RawFd, String> {
        let inner = self.inner.borrow();
        match &inner.kq {
            Some(kq) => Ok(kq.as_raw_fd()),
            None => Err("os/sig-next: receiver is closed".into()),
        }
    }

    #[allow(dead_code)]
    pub fn signals(&self) -> Vec<libc::c_int> {
        self.inner.borrow().signals.clone()
    }

    pub fn close(&self) {
        let mut inner = self.inner.borrow_mut();
        if inner.kq.is_none() {
            posix_trace(format_args!(
                "macos: SignalReceiver::close already closed (idempotent)"
            ));
            return;
        }
        posix_trace(format_args!(
            "macos: SignalReceiver::close kq={:?} signals={:?}",
            inner.kq.as_ref().map(|kq| kq.as_raw_fd()),
            inner.signals
        ));
        // Dropping the OwnedFd closes the descriptor.
        inner.kq = None;
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
        // Taking the OwnedFd drops (closes) it; roll back the signal
        // refcounts only if we hadn't already been closed explicitly.
        if inner.kq.take().is_some() {
            rollback(&inner.signals);
        }
    }
}

impl std::fmt::Debug for SignalReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.borrow();
        write!(
            f,
            "SignalReceiver(kq={:?}, signals={:?}, closed={})",
            inner.kq.as_ref().map(|kq| kq.as_raw_fd()),
            inner.signals,
            inner.kq.is_none()
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
    with_watched_set(|set| {
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
    });
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
