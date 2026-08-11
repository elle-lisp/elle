//! The per-operation bound, on a descriptor that carries no socket options.
//!
//! A pipe is the case that separates a bound belonging to the operation from
//! one belonging to the descriptor: it rejects `SO_RCVTIMEO`/`SO_SNDTIMEO`, and
//! a reader that never reads fills it exactly as a peer that never reads fills
//! a socket. So a `:timeout` that rides the socket options bounds nothing here,
//! and the worker parks in the kernel for as long as the pipe stays full.
//!
//! The end-to-end pins are `tests/elle/port-write-timeout.lisp` case 3 and
//! `tests/elle/port-read-timeout.lisp` case 6, which run on both backends.
//! These cover the same mechanism in the fast inner loop.

use super::super::*;
use crate::io::SubmissionId;
use std::time::{Duration, Instant};

/// A pipe whose ends close with it.
struct Pipe {
    read_fd: RawFd,
    write_fd: RawFd,
}

impl Pipe {
    fn new() -> Self {
        let mut fds = [0 as libc::c_int; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe(2) failed");
        Pipe {
            read_fd: fds[0],
            write_fd: fds[1],
        }
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.read_fd);
            libc::close(self.write_fd);
        }
    }
}

/// True when `fd` is in non-blocking mode.
fn is_nonblocking(fd: RawFd) -> bool {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(flags >= 0, "F_GETFL failed");
    flags & libc::O_NONBLOCK != 0
}

#[test]
fn write_to_a_pipe_nobody_reads_returns_at_its_deadline() {
    let pipe = Pipe::new();
    let mut hub = CompletionHub::new();
    // Larger than any pipe buffer, so the write cannot finish however much
    // room the kernel gives it, and nobody ever reads the other end.
    let payload = vec![b'x'; 4 << 20];

    let started = Instant::now();
    hub.submit(
        SubmissionId::from_raw(1),
        PoolOp::Write {
            fd: pipe.write_fd,
            data: payload,
            timeout: Some(Duration::from_millis(200)),
            stop: None,
        },
    )
    .unwrap();

    // Generous against the 200 ms deadline: the assertion is that the
    // operation ends on its own terms rather than parking forever.
    let rc = hub
        .recv_blocking(Some(Duration::from_secs(10)))
        .expect("the write must report back rather than park in the kernel");
    match rc {
        RawCompletion::Pool(pc) => assert_eq!(
            pc.result_code,
            -libc::ETIMEDOUT,
            "a full pipe must report the caller's timeout"
        ),
        RawCompletion::Stdin(_) => panic!("expected a Pool completion"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the write ran {:?} against a 200 ms timeout",
        started.elapsed()
    );
}

#[test]
fn read_from_a_pipe_nobody_writes_returns_at_its_deadline() {
    let pipe = Pipe::new();
    let mut hub = CompletionHub::new();

    let started = Instant::now();
    hub.submit(
        SubmissionId::from_raw(2),
        PoolOp::Read {
            fd: pipe.read_fd,
            size: 1024,
            timeout: Some(Duration::from_millis(200)),
            stop: None,
        },
    )
    .unwrap();

    let rc = hub
        .recv_blocking(Some(Duration::from_secs(10)))
        .expect("the read must report back rather than park in the kernel");
    match rc {
        RawCompletion::Pool(pc) => assert_eq!(
            pc.result_code,
            -libc::ETIMEDOUT,
            "a silent pipe must report the caller's timeout"
        ),
        RawCompletion::Stdin(_) => panic!("expected a Pool completion"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the read ran {:?} against a 200 ms timeout",
        started.elapsed()
    );
}

#[test]
fn the_last_bound_on_a_descriptor_restores_its_blocking_mode() {
    let pipe = Pipe::new();
    assert!(
        !is_nonblocking(pipe.write_fd),
        "a fresh pipe end is blocking"
    );

    let first = OpBound::new(pipe.write_fd, Some(Duration::from_millis(50)), None);
    assert!(
        is_nonblocking(pipe.write_fd),
        "a timed operation takes the descriptor non-blocking"
    );

    {
        let _second = OpBound::new(pipe.write_fd, Some(Duration::from_millis(50)), None);
        assert!(is_nonblocking(pipe.write_fd), "the second holder joins it");
    }
    // The duplex case: one operation finishing must leave the flag alone
    // while another still runs, or the survivor blocks in the kernel with no
    // bound at all.
    assert!(
        is_nonblocking(pipe.write_fd),
        "a holder that leaves while another remains keeps the flag"
    );

    drop(first);
    assert!(
        !is_nonblocking(pipe.write_fd),
        "the last holder restores what it found"
    );
}

#[test]
fn a_descriptor_that_was_already_non_blocking_stays_that_way() {
    let pipe = Pipe::new();
    let flags = unsafe { libc::fcntl(pipe.read_fd, libc::F_GETFL) };
    unsafe { libc::fcntl(pipe.read_fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };

    {
        let _bound = OpBound::new(pipe.read_fd, Some(Duration::from_millis(50)), None);
        assert!(is_nonblocking(pipe.read_fd));
    }
    assert!(
        is_nonblocking(pipe.read_fd),
        "the bound restores the mode the descriptor arrived in, not a default"
    );
}

#[test]
fn a_pause_ends_at_once_when_the_operation_is_stopped() {
    let stop = open_stop_pipe().expect("a stop pipe");
    let bound = OpBound::new(-1, None, Some(stop.read_fd));
    let byte = 1u8;
    assert_eq!(
        unsafe { libc::write(stop.write_fd, &byte as *const u8 as *const libc::c_void, 1) },
        1,
    );

    let started = Instant::now();
    assert!(
        matches!(bound.pause(Duration::from_secs(5)), Wake::Stopped),
        "a paced retry must see the stop rather than wait out its slice"
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the pause ran {:?} with a stop already written",
        started.elapsed()
    );

    drop(bound); // closes the read end it owns
    unsafe { libc::close(stop.write_fd) };
}

#[test]
fn a_pause_with_nothing_to_stop_it_waits_out_its_slice() {
    let bound = OpBound::new(-1, None, None);

    let started = Instant::now();
    assert!(matches!(
        bound.pause(Duration::from_millis(100)),
        Wake::TimedOut
    ));
    // A pause that returns at once is no pace at all: the connect it belongs
    // to would spin on `EAGAIN` for its whole deadline.
    assert!(
        started.elapsed() >= Duration::from_millis(50),
        "the pause returned after {:?} of a 100 ms slice",
        started.elapsed()
    );
}

#[test]
fn an_untimed_operation_leaves_the_descriptor_alone() {
    let pipe = Pipe::new();

    {
        let _bound = OpBound::new(pipe.read_fd, None, None);
        assert!(
            !is_nonblocking(pipe.read_fd),
            "an operation with no deadline asks to wait indefinitely, so it \
             has no reason to touch the descriptor"
        );
    }
    assert!(!is_nonblocking(pipe.read_fd));
}
