use super::super::*;
use crate::io::request::{zombie_child, ExitRecord};

#[test]
fn test_threadpool_process_wait_success() {
    let mut pool = CompletionHub::new();
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    pool.submit(
        SubmissionId::from_raw(1),
        PoolOp::ProcessWait {
            pid,
            exit: ExitRecord::new(),
        },
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
        PoolOp::ProcessWait {
            pid,
            exit: ExitRecord::new(),
        },
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
    pool.submit(
        id,
        PoolOp::ProcessWait {
            pid,
            exit: ExitRecord::new(),
        },
        bounds,
    )
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
    pool.submit(
        id,
        PoolOp::ProcessWait {
            pid,
            exit: ExitRecord::new(),
        },
        bounds,
    )
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

/// A wait that is stopped after it has already reaped the child keeps the
/// status the reap took.
///
/// The trap: `waitpid` CONSUMES what it reports. The worker asks the kernel
/// before it looks at the stop pipe, so a stop written while the child is
/// reapable meets a worker that has already taken the status — and the
/// completion carrying it is then discarded, because a cancelled submission
/// delivers nothing. The child is gone, and without a record the status goes
/// with it: the next `subprocess/wait` on that child gets `ECHILD`.
///
/// Both halves of the race are forced rather than waited for. The child is
/// already a zombie, so the first ask reaps it, and the stop is written before
/// the operation is submitted, so the worker runs cancelled from its first
/// instruction.
///
/// The counter-factual: with the reap reporting the code without keeping it,
/// the record reads `None` and a later waiter has nothing to answer from.
#[test]
fn a_stopped_wait_that_reaped_the_child_keeps_its_status() {
    let mut pool = CompletionHub::new();
    // The `Child` is dropped here rather than kept: the operation below is the
    // reaper, and std's `Drop` for a `Child` reaps nothing either way.
    let pid = zombie_child().id();
    let exit = ExitRecord::new();

    let id = SubmissionId::from_raw(5);
    let bounds = pool.bounds(id, None);
    // The stop is in the pipe before the worker exists, which is the far side
    // of the window the wait's pace can only narrow.
    pool.stop(id);
    pool.submit(
        id,
        PoolOp::ProcessWait {
            pid,
            exit: exit.clone(),
        },
        bounds,
    )
    .unwrap();

    let completions = pool.wait_pool(Some(5000)).unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(
        completions[0].result_code, 0,
        "a reapable child is reaped on the first ask, whatever the stop says",
    );
    assert_eq!(
        exit.status(),
        Some(1),
        "the reap took `false`'s status from the kernel, so the record is the \
         only place a later wait can read it",
    );

    pool.forget_stop(id);
}

/// A second wait on a child somebody else reaped answers from the record
/// rather than reporting `ECHILD`.
///
/// The trap: two waits on one child is legal, and the loser's `waitpid` sees a
/// child that is gone. Reading the record after the syscall would not settle
/// it — the winner writes the record after its own `waitpid` returns, so the
/// loser can look in the gap and find nothing. The reap runs under the record's
/// lock so there is no gap to look in.
#[test]
fn a_wait_on_an_already_reaped_child_answers_from_the_record() {
    let mut pool = CompletionHub::new();
    let pid = zombie_child().id();
    let exit = ExitRecord::new();

    // The first wait reaps and records.
    let first = SubmissionId::from_raw(6);
    pool.submit(
        first,
        PoolOp::ProcessWait {
            pid,
            exit: exit.clone(),
        },
        Bounds::prompt(),
    )
    .unwrap();
    assert_eq!(pool.wait_pool(Some(5000)).unwrap().len(), 1);

    // The second finds no child at all.
    let second = SubmissionId::from_raw(7);
    pool.submit(
        second,
        PoolOp::ProcessWait {
            pid,
            exit: exit.clone(),
        },
        Bounds::prompt(),
    )
    .unwrap();
    let completions = pool.wait_pool(Some(5000)).unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(
        completions[0].result_code, 0,
        "a status this process is holding is an answer, not a failed waitpid",
    );
    let code = i32::from_le_bytes(completions[0].data[..4].try_into().unwrap());
    assert_eq!(code, 1, "both waits report the status `false` exited with");
}
