// audited: 2026-10-04
//! The signal masks a thread carries, the receivers that watch a signal, and the dispositions installed at startup.
//!
//! docs/posix-signals.md

use super::*;
use crate::io::isolate::{run, Outcome, Report};
use std::time::Duration;

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
fn mask_all_signals_blocks_async_but_not_fault_signals() {
    let h = std::thread::spawn(|| {
        mask_all_signals_on_this_thread();
        current_thread_blocked()
    });
    let blocked = h.join().unwrap();
    // sigkill and sigstop can't be blocked even via SIG_SETMASK with a
    // full set — the kernel silently strips them. Every asynchronous
    // signal should be present.
    assert!(blocked.contains(&libc::SIGTERM), "SIGTERM blocked");
    assert!(blocked.contains(&libc::SIGUSR1), "SIGUSR1 blocked");
    assert!(blocked.contains(&libc::SIGUSR2), "SIGUSR2 blocked");
    assert!(blocked.contains(&libc::SIGINT), "SIGINT blocked");
    // The fault set must stay deliverable: a synchronous fault is bound
    // to the faulting thread, so a mask cannot reroute it. Blocked, a
    // fault force-kills on Linux but wedges the thread on macOS — the
    // kernel re-executes the faulting instruction forever. See
    // docs/posix-signals.md § "Mask policy".
    for (sig, name) in [
        (libc::SIGSEGV, "SIGSEGV"),
        (libc::SIGBUS, "SIGBUS"),
        (libc::SIGILL, "SIGILL"),
        (libc::SIGFPE, "SIGFPE"),
        (libc::SIGTRAP, "SIGTRAP"),
        (libc::SIGSYS, "SIGSYS"),
        (libc::SIGABRT, "SIGABRT"),
    ] {
        assert!(!blocked.contains(&sig), "{name} must stay deliverable");
    }
}

/// A synchronous fault on a thread carrying the worker mask must kill
/// the process promptly. With the fault set wrongly inside the mask,
/// Linux still force-delivers the default (kill), but macOS leaves the
/// signal pending and re-executes the faulting load forever — the
/// body then hangs and `run`'s deadline fails the test.
#[test]
fn fault_on_a_masked_thread_kills_the_process() {
    let report = run(Duration::from_secs(5), || {
        mask_all_signals_on_this_thread();
        let page = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                4096,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if page == libc::MAP_FAILED {
            return 73;
        }
        let v = unsafe { std::ptr::read_volatile(page as *const u8) };
        // Unreachable: the read faults. A distinguishable code in case
        // the kernel somehow satisfied a PROT_NONE read.
        74 + v as i32
    });
    assert!(
        matches!(report.end, Outcome::Signaled(sig) if sig == libc::SIGSEGV || sig == libc::SIGBUS),
        "the body must die from the fault, by SIGSEGV or SIGBUS: {report:?}"
    );
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
    let r = SignalReceiver::new(
        vec![libc::SIGWINCH],
        std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
    )
    .expect("receiver");
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
/// than SIGWINCH (which the parse_events test above also opens
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

        let r1 = SignalReceiver::new(
            vec![libc::SIGURG],
            std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        )
        .unwrap();
        assert!(current_thread_blocked().contains(&libc::SIGURG));
        assert!(currently_watched().contains(&libc::SIGURG));

        let r2 = SignalReceiver::new(
            vec![libc::SIGURG],
            std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        )
        .unwrap();
        assert!(current_thread_blocked().contains(&libc::SIGURG));

        r1.close();
        // Still blocked because r2 holds the refcount > 0.
        assert!(current_thread_blocked().contains(&libc::SIGURG));
        assert!(currently_watched().contains(&libc::SIGURG));

        r2.close();
        // Refcount transitioned 1 → 0, but SIGURG is in ABSORB_SET
        // so rollback skips the unblock — see `rollback` in the
        // platform file. The mask bit stays set; `currently_watched` flips
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
    let r = SignalReceiver::new(
        vec![libc::SIGKILL],
        std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
    );
    assert!(r.is_err());
}

#[test]
fn cannot_watch_sigstop() {
    let r = SignalReceiver::new(
        vec![libc::SIGSTOP],
        std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
    );
    assert!(r.is_err());
}

// ── Eager-trap (init_process_signals) regression tests ──────────────
//
// Each body runs through `run`, as the only thread of its process. In the
// test process, peer threads with other masks would take a signal before
// `init_process_signals` could install the disposition under test.
//
// Each body calls `init_process_signals()`, then raises the signal. A
// terminate-class handler ends the process with `_exit(128 + signum)`. A
// death by the signal itself means the kernel default fired instead of the
// handler, which is the regression these pin.

/// The body's process ended through the handler's `_exit(128 + signum)`, not by
/// the signal's own default.
fn assert_handler_exit(report: Report, signum: libc::c_int) {
    assert_eq!(
        report.end,
        Outcome::Exited(128 + signum),
        "signal {signum}'s handler must _exit(128 + {signum}); 91 means the \
         body ran on past the raise"
    );
}

/// SIGTERM must run the built-in sigaction handler and produce a
/// clean exit with code 143 (= `128 + SIGTERM`), not a death by the
/// signal. Counter-factual: comment out the SIGTERM branch in
/// `install_terminate_handlers` and this test fails because the
/// kernel default (Term) fires instead.
#[test]
fn sigterm_terminates_via_handler_with_code_143() {
    let report = run(Duration::from_secs(5), || {
        init_process_signals();
        unsafe { libc::raise(libc::SIGTERM) };
        // The handler should have called _exit before we get here.
        // If we reach this line the handler is broken — return a
        // distinguishable code so the assertion message makes it clear.
        std::thread::sleep(std::time::Duration::from_millis(200));
        91
    });
    assert_handler_exit(report, libc::SIGTERM);
}

/// Same as SIGTERM but for SIGINT / SIGQUIT / SIGHUP — they all
/// share the terminate-class dispatch.
#[test]
fn sigint_terminates_via_handler_with_code_130() {
    let report = run(Duration::from_secs(5), || {
        init_process_signals();
        unsafe { libc::raise(libc::SIGINT) };
        std::thread::sleep(std::time::Duration::from_millis(200));
        91
    });
    assert_handler_exit(report, libc::SIGINT);
}

#[test]
fn sigquit_terminates_via_handler_with_code_131() {
    let report = run(Duration::from_secs(5), || {
        init_process_signals();
        unsafe { libc::raise(libc::SIGQUIT) };
        std::thread::sleep(std::time::Duration::from_millis(200));
        91
    });
    assert_handler_exit(report, libc::SIGQUIT);
}

#[test]
fn sighup_terminates_via_handler_with_code_129() {
    let report = run(Duration::from_secs(5), || {
        init_process_signals();
        unsafe { libc::raise(libc::SIGHUP) };
        std::thread::sleep(std::time::Duration::from_millis(200));
        91
    });
    assert_handler_exit(report, libc::SIGHUP);
}

/// SIGTSTP stops the process, and the SIGCONT `run` sends resumes it to run
/// on and exit 0.
///
/// The kernel default for SIGTSTP also stops the process, so this pins the
/// stop and the resumption, not the handler that raises SIGSTOP in place of
/// the default.
#[test]
fn sigtstp_pauses_and_sigcont_resumes() {
    let report = run(Duration::from_secs(5), || {
        init_process_signals();
        unsafe { libc::raise(libc::SIGTSTP) };
        0
    });
    assert_eq!(
        report,
        Report {
            end: Outcome::Exited(0),
            stops: 1
        },
        "the body must stop once and exit 0 once continued"
    );
}

/// SIGPIPE must be installed as SIG_IGN at startup. Write on a
/// closed pipe should return -1/EPIPE rather than terminating
/// the process.
///
/// Counter-factual: removing the SIG_IGN install would cause the
/// body to die by SIGPIPE.
#[test]
fn sigpipe_is_ignored_at_startup() {
    let report = run(Duration::from_secs(5), || {
        init_process_signals();
        let mut fds: [libc::c_int; 2] = [0; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return 71;
        }
        // Close the read end first.
        unsafe { libc::close(fds[0]) };
        let buf = [0u8; 4];
        let ret = unsafe { libc::write(fds[1], buf.as_ptr() as *const libc::c_void, buf.len()) };
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        unsafe { libc::close(fds[1]) };
        if ret == -1 && errno == libc::EPIPE {
            0
        } else {
            72
        }
    });
    assert_eq!(
        report.end,
        Outcome::Exited(0),
        "SIGPIPE must be ignored; 71 means the pipe did not open, 72 that the \
         write did not fail with EPIPE"
    );
}

/// When nothing watches SIGUSR1, the eager-trap policy is "absorb":
/// the signal is blocked at startup, the kernel queues it but no
/// thread reads it, and the process keeps running. Counter-factual:
/// without the absorb-set block, the kernel default for SIGUSR1
/// (Term) fires and the body dies by the signal.
#[test]
fn sigusr1_absorbed_when_unwatched() {
    let report = run(Duration::from_secs(5), || {
        init_process_signals();
        unsafe { libc::raise(libc::SIGUSR1) };
        // Give the kernel a beat to deliver if it were going to.
        std::thread::sleep(std::time::Duration::from_millis(100));
        0
    });
    assert_eq!(report.end, Outcome::Exited(0), "SIGUSR1 must be absorbed");
}

/// Watcher overrides built-in: a live `SignalReceiver` on SIGTERM
/// must keep the process alive even after SIGTERM is raised — the
/// signal goes to the receiver's signalfd instead of the sigaction
/// handler. Counter-factual: without the watcher-override
/// mechanism (i.e. if the sigaction handler fires regardless), the
/// body exits with code 143 before it can read the receiver.
///
/// Linux-only: the body reads the receiver's fd as a buffer of
/// `signalfd_siginfo`, which `libc` defines on Linux alone.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn watcher_overrides_builtin_for_sigterm() {
    let report = run(Duration::from_secs(5), || {
        init_process_signals();
        let r = match SignalReceiver::new(
            vec![libc::SIGTERM],
            std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        ) {
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
    assert_eq!(
        report.end,
        Outcome::Exited(0),
        "the watcher must take SIGTERM from the built-in handler; 143 means \
         the handler fired, and 81-85 name the step that failed"
    );
}
