//! audited: 2026-11-17
//! An open on io_uring: a fifo's write side waits with no pool worker out, and a cancel or its own timer ends it.
//!
//! docs/impl/io/platforms.md
//! docs/impl/io/bounds.md
//! docs/impl/io/cancel.md
//!
//! Every test here builds the platform default and returns early where that
//! is not the ring. The pool's open is pinned in `park.rs` and in the worker
//! tests of `threadpool/tests/openfile.rs`.

use super::fifo::Fifo;
use super::*;
use crate::io::pending::Submitter;
use std::time::Instant;

/// A ring backend, or `None` where the ring will not open.
fn ring() -> Option<AsyncBackend> {
    let backend = AsyncBackend::new().unwrap();
    backend.is_uring().then_some(backend)
}

/// The open `port/open` makes of `path` with `flags`, under `bound`, with the
/// unopened port its completion fills.
fn open(path: &str, flags: i32, direction: Direction, bound: Bound) -> IoRequest {
    let h = crate::primitives::ctx::TestHeap::new();
    let port = h.ctx().external(
        "port",
        Port::new_unopened(
            PortKind::File,
            direction,
            Encoding::Binary,
            path.to_string(),
        ),
    );
    let op = IoOp::Open {
        path: path.to_string(),
        flags: flags | libc::O_CLOEXEC,
        mode: 0o666,
        direction,
        encoding: Encoding::Binary,
    };
    IoRequest::unbounded(op, port).within(bound)
}

/// The first completion to arrive within `patience`, or `None`.
fn next_completion(backend: &AsyncBackend, patience: Duration) -> Option<Completion> {
    let until = Instant::now() + patience;
    while Instant::now() < until {
        let mut got = backend.wait(TICK).unwrap();
        if let Some(c) = got.pop() {
            Completion::discard_all(got);
            return Some(c);
        }
    }
    None
}

/// Whether the descriptor an opened port holds is in non-blocking mode.
fn nonblocking(port: Value) -> bool {
    let flags = port
        .as_external::<Port>()
        .and_then(|p| p.with_fd(|fd| unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) }))
        .expect("an opened port holds a descriptor");
    assert!(flags >= 0, "F_GETFL failed");
    flags & libc::O_NONBLOCK != 0
}

/// An open of a fifo for writing waits for a reader with no pool worker out,
/// and a cancel ends it.
///
/// The counter-factual: an open handed to the pool holds a worker for as long
/// as it waits, and asks its fifo again every 10 ms meanwhile.
#[test]
fn a_ring_open_of_a_fifo_nobody_reads_holds_no_worker_and_a_cancel_ends_it() {
    crate::value::arena::with_test_region(|| {
        let Some(backend) = ring() else { return };
        let fifo = Fifo::new("ring-open-cancel");
        let req = open(&fifo.0, libc::O_WRONLY, Direction::Write, Bound::NONE);
        let id = backend.submit(&req, Submitter::for_test()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(backend.workers(), 0, "the open holds a pool worker");
        assert_eq!(backend.pending_ids(), vec![id], "the open still waits");

        backend.cancel(id).unwrap();
        for _ in 0..40 {
            Completion::discard_all(backend.wait(TICK).unwrap());
            if !backend.has_pending() {
                break;
            }
        }
        assert!(!backend.has_pending(), "the cancel never ended the open");
    });
}

/// An open of a fifo nobody reads answers `:timeout` once its bound passes,
/// with no pool worker out while it waits.
///
/// The trap: a linked timeout that fires on an open waiting in an io-wq thread
/// marks the open cancelled and never ends it, and no CQE arrives at all. The
/// open's bound is a timer of its own that asks for the cancel instead.
#[test]
fn a_ring_open_of_a_fifo_nobody_reads_answers_its_timeout() {
    crate::value::arena::with_test_region(|| {
        let Some(backend) = ring() else { return };
        let fifo = Fifo::new("ring-open-timeout");
        let bound = Bound::per_op(Duration::from_millis(200));
        let started = Instant::now();
        let req = open(&fifo.0, libc::O_WRONLY, Direction::Write, bound);
        let id = backend.submit(&req, Submitter::for_test()).unwrap();

        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(backend.workers(), 0, "the open holds a pool worker");

        let c = next_completion(&backend, Duration::from_secs(5))
            .expect("the open must answer at its bound rather than wait on");
        let waited = started.elapsed();
        assert_eq!(c.id, id);
        let kind =
            c.result.as_ref().err().and_then(|e| {
                sorted_struct_get(e.as_struct()?, &TableKey::keyword("error")).copied()
            });
        c.discard();
        assert_eq!(kind, Some(Value::keyword("timeout")), "a :timeout error");
        assert!(
            waited >= Duration::from_millis(200),
            "it ended at {waited:?}"
        );
        assert!(!backend.has_pending(), "and the entry went with the answer");
    });
}

/// An open of a fifo for writing answers once a reader opens the other end,
/// with the descriptor in the blocking mode `port/open` asked for.
#[test]
fn a_ring_open_of_a_fifo_answers_once_a_reader_arrives() {
    crate::value::arena::with_test_region(|| {
        let Some(backend) = ring() else { return };
        let fifo = Fifo::new("ring-open-reader");
        let bound = Bound::per_op(Duration::from_secs(10));
        let req = open(&fifo.0, libc::O_WRONLY, Direction::Write, bound);
        let id = backend.submit(&req, Submitter::for_test()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(backend.workers(), 0, "the open holds a pool worker");
        // The read side blocks until a writer is there, and the open is that
        // writer, so the two meet.
        let reader = std::fs::File::open(&fifo.0).expect("open the read end");

        let c = next_completion(&backend, Duration::from_secs(5)).expect("the open answers");
        assert_eq!(c.id, id);
        let port = *c.result.as_ref().expect("the open succeeds");
        assert!(!nonblocking(port), "the descriptor is blocking");
        c.discard();
        drop(reader);
    });
}

/// An open of a fifo for reading answers at once though nobody writes it, and
/// its descriptor is blocking.
///
/// The trap: a forced-async open waits in io-wq for the other end on the read
/// side too. `port/open` would then wait for a writer the program may be about
/// to start itself, where the pool's open answers at once.
#[test]
fn a_ring_open_of_a_fifo_for_reading_answers_at_once() {
    crate::value::arena::with_test_region(|| {
        let Some(backend) = ring() else { return };
        let fifo = Fifo::new("ring-open-read");
        let req = open(&fifo.0, libc::O_RDONLY, Direction::Read, Bound::NONE);
        let id = backend.submit(&req, Submitter::for_test()).unwrap();

        let c = next_completion(&backend, Duration::from_secs(2))
            .expect("the read side of a fifo opens without a writer");
        assert_eq!(c.id, id);
        let port = *c.result.as_ref().expect("the open succeeds");
        assert!(!nonblocking(port), "the descriptor is blocking");
        c.discard();
    });
}

/// An open that answers before its bound takes its timer out of the ring.
///
/// The counter-factual: a timer left armed expires later with nothing to
/// cancel, and its CQE wakes the scheduler's wait for nothing. The last wait
/// below would then return at the open's bound rather than at its own.
#[test]
fn a_ring_open_that_answers_first_leaves_no_timer_behind() {
    crate::value::arena::with_test_region(|| {
        let Some(backend) = ring() else { return };
        let path = write_temp_file("ring-open-timer");
        let bound = Bound::per_op(Duration::from_millis(300));
        let req = open(&path, libc::O_RDONLY, Direction::Read, bound);
        backend.submit(&req, Submitter::for_test()).unwrap();
        let c = next_completion(&backend, Duration::from_secs(2)).expect("the open answers");
        assert!(c.result.is_ok(), "the open succeeds");
        c.discard();

        // The timer's removal posts two CQEs of its own: reap them.
        for _ in 0..2 {
            Completion::discard_all(backend.wait(TICK).unwrap());
        }
        let started = Instant::now();
        let woke = backend.wait(Some(Duration::from_secs(1))).unwrap();
        let waited = started.elapsed();
        assert!(woke.is_empty(), "nothing was in flight to answer");
        assert!(
            waited >= Duration::from_millis(900),
            "a timer woke the wait after {waited:?}"
        );
        std::fs::remove_file(&path).ok();
    });
}
