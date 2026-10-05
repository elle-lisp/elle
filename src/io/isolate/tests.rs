// audited: 2026-10-04
//! What `run` promises a body: no lock held, no other thread, a deadline, and a report that names its test.
//!
//! docs/analysis/testing.md

use super::*;
use std::sync::Mutex;

/// Held by a thread of the test process while the body runs.
static HELD: Mutex<()> = Mutex::new(());

/// A lock another thread of the test process holds is free in the body.
///
/// The trap: a fork copies a held lock as held, and the thread that would
/// release it does not exist in the child. The standard library's SIGSEGV
/// handler takes such a lock whenever a thread starts or ends, so a forked
/// child that faulted could spin until its deadline.
///
/// The counter-factual: fork the test process itself, and the body finds
/// `HELD` locked by a thread it does not have.
#[test]
fn a_lock_held_in_the_test_process_is_free_in_the_body() {
    // This code runs again in the copy `run` starts, so hold the lock only in
    // the test process.
    let holder = std::env::var_os(COPY).is_none().then(|| {
        let (held_tx, held_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            let _guard = HELD.lock().expect("HELD is never poisoned");
            held_tx.send(()).expect("the test waits for the lock");
            let _ = release_rx.recv();
        });
        held_rx.recv().expect("the holder takes the lock");
        (thread, release_tx)
    });

    let report = run(Duration::from_secs(30), || match HELD.try_lock() {
        Ok(_) => 0,
        Err(_) => 1,
    });

    if let Some((thread, release)) = holder {
        drop(release);
        thread.join().expect("the holder thread");
    }
    assert_eq!(
        report.end,
        Outcome::Exited(0),
        "the body found a lock held that only the test process holds"
    );
}

/// The body is the only thread of its process, so a signal the process sends
/// itself can reach no thread but the body's.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn the_body_is_the_only_thread_of_its_process() {
    let report = run(Duration::from_secs(30), || {
        match std::fs::read_dir("/proc/self/task").map(Iterator::count) {
            Ok(1) => 0,
            Ok(_) => 1,
            Err(_) => 2,
        }
    });
    assert_eq!(
        report.end,
        Outcome::Exited(0),
        "1: the body's process has other threads; 2: /proc/self/task did not read"
    );
}

#[test]
fn a_body_past_its_deadline_is_reported_hung() {
    let report = run(Duration::from_secs(1), || {
        std::thread::sleep(Duration::from_secs(60));
        0
    });
    assert_eq!(report.end, Outcome::Hung);
}

#[test]
fn the_signal_that_ends_the_body_is_reported() {
    let report = run(Duration::from_secs(30), || {
        unsafe { libc::raise(libc::SIGKILL) };
        0
    });
    assert_eq!(report.end, Outcome::Signaled(libc::SIGKILL));
}

#[test]
fn a_stopped_body_is_continued_and_counted() {
    let report = run(Duration::from_secs(30), || {
        unsafe { libc::raise(libc::SIGSTOP) };
        0
    });
    assert_eq!(
        report,
        Report {
            end: Outcome::Exited(0),
            stops: 1
        }
    );
}

/// `run` reads the test's name from the thread libtest runs it on. A name that
/// matches no test starts a copy that runs nothing.
///
/// The counter-factual: trust the copy's exit status alone, and a copy that ran
/// no test exits 0, which reads as a body that passed.
#[test]
fn a_copy_that_runs_no_test_fails_the_caller() {
    let joined = std::thread::Builder::new()
        .name("io::isolate::tests::no_test_has_this_name".into())
        .spawn(|| run(Duration::from_secs(30), || 0))
        .expect("spawn the misnamed thread")
        .join();
    let payload = joined.expect_err("run must fail when its copy runs no test");
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        message.contains("ran no test"),
        "the failure names the missing test: {message:?}"
    );
}
