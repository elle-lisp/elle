// audited: 2026-11-20
//! Opening a fifo's read side on a pool worker: the open waits for a writer, and its bound or a stop ends the wait.
//!
//! docs/io/timeout.md
//! docs/impl/io/cancel.md
//!
//! The counter-factual: a worker that hands back the non-blocking open of the
//! read side answers before any writer comes. Every read on that descriptor
//! then meets a fifo with no writer, and `read(2)` answers 0 for that, so the
//! port reads an end of file instead of what the writer sends.

use super::super::*;
use super::Fifo;
use crate::io::request::Bound;
use std::io::{Read, Write};
use std::os::unix::io::FromRawFd;
use std::time::{Duration, Instant};

/// An open of `fifo`'s read side, as `port/open` asks for it.
fn read_side(fifo: &Fifo) -> PoolOp {
    PoolOp::Open {
        path: fifo.c_path(),
        flags: libc::O_RDONLY | libc::O_CLOEXEC,
        mode: 0o666,
    }
}

/// Whether anything holds `fifo` open for reading. A non-blocking open of the
/// write end succeeds only then, and answers `ENXIO` otherwise. That open is a
/// writer coming and going, so ask only once no open of the fifo is waiting.
fn has_reader(fifo: &Fifo) -> bool {
    let fd = unsafe {
        libc::open(
            fifo.c_path().as_ptr(),
            libc::O_WRONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd >= 0 {
        unsafe { libc::close(fd) };
        return true;
    }
    let errno = std::io::Error::last_os_error().raw_os_error();
    assert_eq!(errno, Some(libc::ENXIO), "the probe of the fifo failed");
    false
}

#[test]
fn a_read_side_open_of_a_fifo_waits_for_its_writer() {
    let fifo = Fifo::new("fifo-read-writer");
    let mut pool = CompletionHub::new();
    pool.submit(
        SubmissionId::from_raw(30),
        read_side(&fifo),
        Bounds::new(Bound::per_op(Duration::from_secs(10)), None),
    )
    .unwrap();

    assert!(
        pool.wait_pool(Some(200)).unwrap().is_empty(),
        "the open answered before any writer came"
    );

    // A reader is waiting, so this blocking open returns at once.
    let mut writer = std::fs::OpenOptions::new()
        .write(true)
        .open(&fifo.0)
        .expect("open the write end");
    let completions = pool.wait_pool(Some(10_000)).unwrap();
    assert_eq!(completions.len(), 1, "the writer's open ends the wait");
    let fd = completions[0].result_code;
    assert!(fd >= 0, "the open answers a descriptor, got {}", fd);
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert_eq!(
        flags & libc::O_NONBLOCK,
        0,
        "the descriptor is in the mode the caller asked for"
    );

    writer.write_all(b"abc").unwrap();
    drop(writer);
    // SAFETY: the completion handed this descriptor to the test alone.
    let mut reader = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut got = Vec::new();
    reader.read_to_end(&mut got).unwrap();
    assert_eq!(got, b"abc", "the read answers what the writer sent");
}

#[test]
fn a_read_side_open_of_a_fifo_nobody_writes_reports_the_callers_timeout() {
    let fifo = Fifo::new("fifo-read-timeout");
    let mut pool = CompletionHub::new();
    let started = Instant::now();
    pool.submit(
        SubmissionId::from_raw(31),
        read_side(&fifo),
        Bounds::new(Bound::per_op(Duration::from_millis(200)), None),
    )
    .unwrap();

    let completions = pool
        .wait_pool(Some(10_000))
        .expect("the open must report back rather than park in the kernel");
    assert_eq!(completions.len(), 1);
    assert_eq!(
        completions[0].result_code,
        -libc::ETIMEDOUT,
        "a fifo nobody writes must report the caller's timeout"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(200),
        "the open answered after {:?}, before its 200 ms timeout",
        started.elapsed()
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the open ran {:?} against a 200 ms timeout",
        started.elapsed()
    );
    assert!(!has_reader(&fifo), "the open leaves no reader behind");
}

#[test]
fn a_stop_ends_a_read_side_open_of_a_fifo() {
    let fifo = Fifo::new("fifo-read-stop");
    let mut pool = CompletionHub::new();
    let id = SubmissionId::from_raw(32);
    let bounds = pool.bounds(id, Bound::NONE);
    assert!(bounds.can_stop(), "the open carries a stop pipe");
    pool.submit(id, read_side(&fifo), bounds).unwrap();

    assert!(
        pool.wait_pool(Some(200)).unwrap().is_empty(),
        "the open answered before any writer came"
    );
    let stopped = Instant::now();
    pool.stop(id);
    let completions = pool
        .wait_pool(Some(10_000))
        .expect("the stop must reach the open");
    assert_eq!(completions.len(), 1);
    assert_eq!(
        completions[0].result_code,
        -libc::ECANCELED,
        "a stopped open reports the cancel"
    );
    assert!(
        stopped.elapsed() < Duration::from_secs(5),
        "the open ran {:?} past its stop",
        stopped.elapsed()
    );
    pool.forget_stop(id);
    assert!(!has_reader(&fifo), "the open leaves no reader behind");
}
