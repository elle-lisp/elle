use super::super::*;

#[test]
fn test_threadpool_open_existing_file_returns_valid_fd() {
    let path = "/dev/shm/elle-test-threadpool-open-success";
    std::fs::write(path, "test").unwrap();

    let mut pool = CompletionHub::new();
    let c_path = std::ffi::CString::new(path).unwrap();
    pool.submit(
        SubmissionId::from_raw(10),
        PoolOp::Open {
            path: c_path,
            flags: libc::O_RDONLY | libc::O_CLOEXEC,
            mode: 0o666,
        },
    )
    .unwrap();

    let completions = pool.wait_pool(Some(5000)).unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].id, 10);
    // result_code must be a valid fd (>= 0)
    let fd = completions[0].result_code;
    assert!(fd >= 0, "expected valid fd, got {}", fd);
    // Close the fd to avoid leaking it
    unsafe { libc::close(fd) };

    std::fs::remove_file(path).ok();
}

#[test]
fn test_threadpool_open_nonexistent_path_returns_negative_errno() {
    let path = "/dev/shm/elle-test-threadpool-open-nonexistent-dir/nofile";

    let mut pool = CompletionHub::new();
    let c_path = std::ffi::CString::new(path).unwrap();
    pool.submit(
        SubmissionId::from_raw(11),
        PoolOp::Open {
            path: c_path,
            flags: libc::O_RDONLY | libc::O_CLOEXEC,
            mode: 0o666,
        },
    )
    .unwrap();

    let completions = pool.wait_pool(Some(5000)).unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].id, 11);
    // result_code must be negative (errno)
    assert!(
        completions[0].result_code < 0,
        "expected negative errno for nonexistent path, got {}",
        completions[0].result_code
    );
}
