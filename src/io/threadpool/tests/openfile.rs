use super::super::*;
use super::file_path;

#[test]
fn test_threadpool_open_existing_file_returns_valid_fd() {
    let path = file_path("open-success");
    std::fs::write(&path, "test").unwrap();

    let mut pool = CompletionHub::new();
    let c_path = std::ffi::CString::new(path.as_str()).unwrap();
    pool.submit(
        SubmissionId::from_raw(10),
        PoolOp::Open {
            path: c_path,
            flags: libc::O_RDONLY | libc::O_CLOEXEC,
            mode: 0o666,
        },
        Bounds::prompt(),
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

    std::fs::remove_file(&path).ok();
}

#[test]
fn test_threadpool_open_nonexistent_path_returns_negative_errno() {
    // A file under a directory that no test creates, so the open must fail.
    let path = format!("{}/nofile", file_path("no-such-dir"));

    let mut pool = CompletionHub::new();
    let c_path = std::ffi::CString::new(path.as_str()).unwrap();
    pool.submit(
        SubmissionId::from_raw(11),
        PoolOp::Open {
            path: c_path,
            flags: libc::O_RDONLY | libc::O_CLOEXEC,
            mode: 0o666,
        },
        Bounds::prompt(),
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

/// A fifo that unlinks with itself.
struct Fifo(String);

impl Fifo {
    fn new(tag: &str) -> Fifo {
        let path = file_path(tag);
        let c_path = std::ffi::CString::new(path.as_str()).unwrap();
        assert_eq!(
            unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) },
            0,
            "mkfifo failed"
        );
        Fifo(path)
    }

    fn c_path(&self) -> std::ffi::CString {
        std::ffi::CString::new(self.0.as_str()).unwrap()
    }
}

impl Drop for Fifo {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
}

/// Opening a fifo for writing waits for a reader — and the caller's `:timeout`
/// is what ends that wait.
///
/// `port/open` documents the deadline (`(port/open "fifo" :read :timeout
/// 5000)`), and this is the direction where the wait actually happens: POSIX
/// blocks a write-side open until a reader opens the other end, which it need
/// never do.
#[test]
fn opening_a_fifo_for_writing_reports_the_callers_timeout() {
    use std::time::{Duration, Instant};

    let fifo = Fifo::new("fifo-write");
    let mut pool = CompletionHub::new();

    let started = Instant::now();
    pool.submit(
        SubmissionId::from_raw(12),
        PoolOp::Open {
            path: fifo.c_path(),
            flags: libc::O_WRONLY | libc::O_CLOEXEC,
            mode: 0o666,
        },
        Bounds::new(Some(Duration::from_millis(200)), None),
    )
    .unwrap();

    let completions = pool
        .wait_pool(Some(10_000))
        .expect("the open must report back rather than park in the kernel");
    assert_eq!(completions.len(), 1);
    assert_eq!(
        completions[0].result_code,
        -libc::ETIMEDOUT,
        "a fifo nobody reads must report the caller's timeout"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the open ran {:?} against a 200 ms timeout",
        started.elapsed()
    );
}

/// Opening a fifo for writing succeeds once a reader arrives, so the paced
/// retry is a wait rather than a fast failure.
#[test]
fn opening_a_fifo_for_writing_succeeds_when_a_reader_arrives() {
    use std::time::Duration;

    let fifo = Fifo::new("fifo-late-reader");
    let mut pool = CompletionHub::new();

    pool.submit(
        SubmissionId::from_raw(13),
        PoolOp::Open {
            path: fifo.c_path(),
            flags: libc::O_WRONLY | libc::O_CLOEXEC,
            mode: 0o666,
        },
        Bounds::new(Some(Duration::from_secs(10)), None),
    )
    .unwrap();

    // Late on purpose: the open must already be retrying when the reader shows
    // up, which is the case a fast `ENXIO` failure would get wrong.
    std::thread::sleep(Duration::from_millis(100));
    let reader = std::fs::File::open(&fifo.0).expect("open the read end");

    let completions = pool.wait_pool(Some(10_000)).unwrap();
    assert_eq!(completions.len(), 1);
    let fd = completions[0].result_code;
    assert!(
        fd >= 0,
        "the open must succeed once a reader arrives, got {}",
        fd
    );
    unsafe { libc::close(fd) };
    drop(reader);
}

/// The descriptor handed back carries the caller's flags, not the worker's.
///
/// `O_NONBLOCK` is how the open reports instead of parking, and it belongs to
/// the open file description — so leaving it set would make every later read on
/// this port return `EAGAIN` where the caller asked for a blocking file.
/// `OpBound` is what sets non-blocking mode per operation from then on.
#[test]
fn an_opened_descriptor_is_left_in_the_mode_the_caller_asked_for() {
    let path = file_path("open-flags");
    std::fs::write(&path, "test").unwrap();

    let mut pool = CompletionHub::new();
    pool.submit(
        SubmissionId::from_raw(14),
        PoolOp::Open {
            path: std::ffi::CString::new(path.as_str()).unwrap(),
            flags: libc::O_RDONLY | libc::O_CLOEXEC,
            mode: 0o666,
        },
        Bounds::prompt(),
    )
    .unwrap();

    let completions = pool.wait_pool(Some(5000)).unwrap();
    let fd = completions[0].result_code;
    assert!(fd >= 0, "expected valid fd, got {}", fd);
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(flags >= 0, "F_GETFL failed");
    assert_eq!(
        flags & libc::O_NONBLOCK,
        0,
        "the open left the descriptor non-blocking, which the caller did not ask for"
    );

    unsafe { libc::close(fd) };
    std::fs::remove_file(&path).ok();
}
