use super::super::*;

#[test]
fn test_threadpool_process_wait_success() {
    let mut pool = CompletionHub::new();
    let mut child = std::process::Command::new("/bin/true").spawn().unwrap();
    let pid = child.id();
    pool.submit(SubmissionId::from_raw(1), PoolOp::ProcessWait { pid })
        .unwrap();
    let completions = pool.wait_pool(Some(5000)).unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].id, 1);
    // ProcessWait encodes the exit code in data (LE i32), not result_code.
    assert_eq!(completions[0].result_code, 0, "waitpid should succeed");
    let exit_code = i32::from_le_bytes(completions[0].data[..4].try_into().unwrap());
    assert_eq!(exit_code, 0, "expected exit code 0 from /bin/true");
    // Reap from std::process::Child to avoid zombie
    let _ = child.wait();
}

#[test]
fn test_threadpool_process_wait_failure() {
    let mut pool = CompletionHub::new();
    let mut child = std::process::Command::new("/bin/false").spawn().unwrap();
    let pid = child.id();
    pool.submit(SubmissionId::from_raw(2), PoolOp::ProcessWait { pid })
        .unwrap();
    let completions = pool.wait_pool(Some(5000)).unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].id, 2);
    // ProcessWait encodes the exit code in data (LE i32), not result_code.
    // result_code=0 means waitpid succeeded; the process exit code is in data.
    assert_eq!(completions[0].result_code, 0, "waitpid should succeed");
    let exit_code = i32::from_le_bytes(completions[0].data[..4].try_into().unwrap());
    assert_ne!(exit_code, 0, "expected non-zero exit code from /bin/false");
    let _ = child.wait();
}
