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
        let ret = unsafe { libc::write(fds[1], buf.as_ptr() as *const libc::c_void, buf.len()) };
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
///
/// Linux-only: the body reads the receiver's fd as a buffer of
/// `signalfd_siginfo`, which `libc` defines on Linux alone.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn watcher_overrides_builtin_for_sigterm() {
    let status = fork_run(5, || {
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
