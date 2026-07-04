use super::{drain_pending_blocked, posix_trace, with_watched_set, SigEvent};
use std::cell::RefCell;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};

pub(crate) struct SignalReceiver {
    inner: RefCell<SignalReceiverInner>,
}

struct SignalReceiverInner {
    /// The signalfd. `None` once the receiver is closed; the `OwnedFd`
    /// closes the descriptor on drop, so there is no manual `close`
    /// call and no risk of a double-close.
    fd: Option<OwnedFd>,
    signals: Vec<libc::c_int>,
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
        with_watched_set(|set| {
            for &s in &signals {
                let entry = set.refcount.entry(s).or_insert(0);
                if *entry == 0 {
                    unsafe { libc::sigaddset(&mut mask, s) };
                }
                *entry += 1;
            }
        });
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
        let fd = unsafe { libc::signalfd(-1, &sfd_mask, libc::SFD_CLOEXEC | libc::SFD_NONBLOCK) };
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
                // SAFETY: `fd` is a fresh signalfd descriptor owned by us.
                fd: Some(unsafe { OwnedFd::from_raw_fd(fd) }),
                signals,
            }),
        })
    }

    pub fn raw_fd(&self) -> Result<RawFd, String> {
        let inner = self.inner.borrow();
        match &inner.fd {
            Some(fd) => Ok(fd.as_raw_fd()),
            None => Err("os/sig-next: receiver is closed".into()),
        }
    }

    #[allow(dead_code)]
    pub fn signals(&self) -> Vec<libc::c_int> {
        self.inner.borrow().signals.clone()
    }

    pub fn close(&self) {
        let mut inner = self.inner.borrow_mut();
        if inner.fd.is_none() {
            posix_trace(format_args!(
                "linux: SignalReceiver::close already closed (idempotent)"
            ));
            return;
        }
        posix_trace(format_args!(
            "linux: SignalReceiver::close fd={:?} signals={:?}",
            inner.fd.as_ref().map(|f| f.as_raw_fd()),
            inner.signals
        ));
        // Dropping the OwnedFd closes the descriptor.
        inner.fd = None;
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
        // Taking the OwnedFd drops (closes) it; roll back the signal
        // refcounts only if we hadn't already been closed explicitly.
        if inner.fd.take().is_some() {
            rollback(&inner.signals);
        }
    }
}

impl std::fmt::Debug for SignalReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.borrow();
        write!(
            f,
            "SignalReceiver(fd={:?}, signals={:?}, closed={})",
            inner.fd.as_ref().map(|f| f.as_raw_fd()),
            inner.signals,
            inner.fd.is_none()
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
    with_watched_set(|set| {
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
    });
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
