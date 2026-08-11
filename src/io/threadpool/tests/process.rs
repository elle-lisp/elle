use super::super::*;

#[test]
fn test_threadpool_process_wait_success() {
    let mut pool = CompletionHub::new();
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    pool.submit(
        SubmissionId::from_raw(1),
        PoolOp::ProcessWait { pid },
        Bounds::prompt(),
    )
    .unwrap();
    let completions = pool.wait_pool(Some(5000)).unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].id, 1);
    // ProcessWait encodes the exit code in data (LE i32), not result_code.
    assert_eq!(completions[0].result_code, 0, "waitpid should succeed");
    let exit_code = i32::from_le_bytes(completions[0].data[..4].try_into().unwrap());
    assert_eq!(exit_code, 0, "expected exit code 0 from `true`");
    // Reap from std::process::Child to avoid zombie
    let _ = child.wait();
}

#[test]
fn test_threadpool_process_wait_failure() {
    let mut pool = CompletionHub::new();
    let mut child = std::process::Command::new("false").spawn().unwrap();
    let pid = child.id();
    pool.submit(
        SubmissionId::from_raw(2),
        PoolOp::ProcessWait { pid },
        Bounds::prompt(),
    )
    .unwrap();
    let completions = pool.wait_pool(Some(5000)).unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].id, 2);
    // ProcessWait encodes the exit code in data (LE i32), not result_code.
    // result_code=0 means waitpid succeeded; the process exit code is in data.
    assert_eq!(completions[0].result_code, 0, "waitpid should succeed");
    let exit_code = i32::from_le_bytes(completions[0].data[..4].try_into().unwrap());
    assert_ne!(exit_code, 0, "expected non-zero exit code from `false`");
    let _ = child.wait();
}

/// A wait on a child that has not exited must end when the operation is
/// stopped, rather than when the child eventually exits.
///
/// `waitpid(pid, .., 0)` holds the worker for the child's whole life, where
/// neither `io/cancel` nor a deadline can reach it. `sleep 30` outlives every
/// bound here, so a wait that ends promptly ended because it was stopped.
#[test]
fn a_stopped_process_wait_ends_rather_than_waiting_for_the_child() {
    use std::time::{Duration, Instant};

    let mut pool = CompletionHub::new();
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = child.id();

    let id = SubmissionId::from_raw(3);
    // No deadline, exactly as `submit_process_wait` builds it: the stop pipe is
    // the whole bound.
    let bounds = pool.bounds(id, None);
    pool.submit(id, PoolOp::ProcessWait { pid }, bounds)
        .unwrap();

    // Let the worker reach its wait first, so the stop arrives at a worker
    // already waiting — the order a cancel meets in production.
    std::thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    pool.stop(id);

    let completions = pool.wait_pool(Some(5000)).unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(
        completions[0].result_code,
        -libc::ECANCELED,
        "a stopped wait must report the cancellation, not the child's exit"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the wait ran {:?} after being stopped, so it was waiting on the child",
        started.elapsed()
    );

    pool.forget_stop(id);
    let _ = child.kill();
    let _ = child.wait();
}

/// A child that exits while the wait is out is still reported, and promptly.
///
/// The wait pauses between asks, so this is what says the pace stays short
/// enough that `subprocess/wait` on an ordinary command is not held back by it.
#[test]
fn a_process_wait_reports_a_short_lived_child_without_delay() {
    use std::time::{Duration, Instant};

    let mut pool = CompletionHub::new();
    // Started before the wait, so the exit lands while the worker is out
    // rather than before it ever asked.
    let mut child = std::process::Command::new("sleep")
        .arg("0.1")
        .spawn()
        .unwrap();
    let pid = child.id();

    let started = Instant::now();
    let id = SubmissionId::from_raw(4);
    let bounds = pool.bounds(id, None);
    pool.submit(id, PoolOp::ProcessWait { pid }, bounds)
        .unwrap();

    let completions = pool.wait_pool(Some(5000)).unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].result_code, 0, "waitpid should succeed");
    let exit_code = i32::from_le_bytes(completions[0].data[..4].try_into().unwrap());
    assert_eq!(exit_code, 0);
    assert!(
        started.elapsed() < Duration::from_millis(600),
        "a child that exited at 100 ms was reported {:?} in",
        started.elapsed()
    );

    pool.forget_stop(id);
    let _ = child.wait();
}
