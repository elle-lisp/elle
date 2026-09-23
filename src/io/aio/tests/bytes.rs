//! audited: 2026-09-23
//! A write read from its payload, a remainder given back to its port, and a
//! teardown that waits for a worker.
//!
//! docs/impl/io-bytes.md

use super::*;
use std::os::unix::io::{FromRawFd, OwnedFd};

/// Write junk into `fd` until the kernel refuses more, and report how much it
/// took. The descriptor is left blocking, as it was: io_uring answers a write
/// on a non-blocking descriptor with `EAGAIN` rather than waiting for room.
fn fill_send_buffer(fd: RawFd) -> usize {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(flags >= 0, "F_GETFL failed");
    set_nonblocking(fd);
    let junk = vec![b'j'; 64 * 1024];
    let mut queued = 0usize;
    loop {
        let n = unsafe { libc::write(fd, junk.as_ptr() as *const libc::c_void, junk.len()) };
        if n <= 0 {
            break;
        }
        queued += n as usize;
    }
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFL, flags) }, 0);
    queued
}

/// Write every byte of `bytes` to `fd`.
fn write_all(fd: RawFd, bytes: &[u8]) {
    let mut sent = 0usize;
    while sent < bytes.len() {
        let n = unsafe {
            libc::write(
                fd,
                bytes[sent..].as_ptr() as *const libc::c_void,
                bytes.len() - sent,
            )
        };
        assert!(n > 0, "the peer's write failed");
        sent += n as usize;
    }
}

/// Read exactly `n` bytes from a blocking `fd`.
fn read_exactly(fd: RawFd, n: usize) -> Vec<u8> {
    let mut got = vec![0u8; n];
    let mut filled = 0usize;
    while filled < n {
        let r = unsafe {
            libc::read(
                fd,
                got[filled..].as_mut_ptr() as *mut libc::c_void,
                n - filled,
            )
        };
        assert!(r > 0, "the peer stopped short: {filled} of {n} bytes");
        filled += r as usize;
    }
    got
}

/// Wait for the completion filed under `id`, discarding any other.
fn completion_for(backend: &AsyncBackend, id: SubmissionId) -> Completion {
    for _ in 0..200 {
        let mut found = None;
        for c in backend.wait(50).unwrap() {
            if c.id == id && found.is_none() {
                found = Some(c);
            } else {
                c.discard();
            }
        }
        if let Some(c) = found {
            return c;
        }
    }
    panic!("the operation filed under {id} never completed");
}

/// Submit `op` on `port` with `timeout`, on behalf of no fiber.
fn submit(
    backend: &AsyncBackend,
    op: PortOp,
    port: Value,
    timeout: Option<std::time::Duration>,
) -> SubmissionId {
    backend
        .submit(
            &IoRequest {
                op: op.into(),
                port,
                timeout,
            },
            crate::io::pending::Submitter::for_test(),
        )
        .unwrap()
}

/// The bytes a completed read answered with.
fn answered_bytes(c: Completion, what: &str) -> Vec<u8> {
    let bytes = match &c.result {
        Ok(v) => v
            .as_bytes()
            .map(|b| b.to_vec())
            .or_else(|| v.with_string(|s| s.as_bytes().to_vec()))
            .unwrap_or_else(|| panic!("{what}: answered with {}", v.type_name())),
        Err(e) => panic!("{what}: failed with {e}"),
    };
    c.discard();
    bytes
}

/// A write hands the kernel its payload's address rather than a copy.
///
/// The peer's send buffer is full before the write is submitted, so neither
/// backend can move a byte of it yet: the ring parks the write in the kernel,
/// and the pool worker parks in its poll. The payload is changed while it
/// waits, and only then does the peer read.
///
/// The counter-factual: a write that copied its payload at submission sends
/// the bytes as they were then, every `A`. One that reads the payload where it
/// lies sends the `B`s it finds there when the peer makes room.
#[test]
fn a_write_hands_the_kernel_the_payload_where_it_lies() {
    const PAYLOAD: usize = 64 * 1024;
    crate::value::arena::with_test_region(|| {
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            let h = crate::primitives::ctx::TestHeap::new();
            let (ours, peer) = stream_socket_pair();
            let queued = fill_send_buffer(ours);
            let payload = h.ctx().bytes(vec![b'A'; PAYLOAD]);
            let port = h.ctx().external(
                "port",
                Port::new_unix_stream(
                    unsafe { OwnedFd::from_raw_fd(ours) },
                    "socketpair".to_string(),
                ),
            );

            let id = submit(&backend, PortOp::Write { data: payload }, port, None);
            // The pool's worker must be parked before the payload changes; the
            // ring parked the write inside the submit call.
            wait_for_worker(&backend);
            // SAFETY: nothing reads the payload but the parked write, and the
            // heap it lives on outlives this loop iteration.
            unsafe {
                let (bytes, len) = crate::io::request::writeable_buffer_ptr(&payload);
                std::ptr::write_bytes(bytes, b'B', len);
            }

            let reader = std::thread::spawn(move || read_exactly(peer, queued + PAYLOAD));
            let done = completion_for(&backend, id);
            assert_eq!(
                done.result.as_ref().ok().and_then(|v| v.as_int()),
                Some(PAYLOAD as i64),
                "{which}: the write must report its whole payload",
            );
            done.discard();
            let got = reader.join().expect("the peer's reader");
            unsafe { libc::close(peer) };

            let sent = &got[queued..];
            assert!(
                sent.iter().all(|&b| b == b'B'),
                "{which}: the peer received {} bytes of the payload as it was at \
                 submission — the write copied it rather than handing the kernel \
                 its address",
                sent.iter().filter(|&&b| b == b'A').count(),
            );
        }
    });
}

/// A read that ends without answering leaves its port's remainder where the
/// next read finds it, first in stream order.
///
/// Two endings, both on each backend. A cancel is the one a program takes most
/// often — a lost `ev/timeout` — and the next read is submitted before the
/// cancelled one's completion arrives, so the remainder must be back in the port
/// at the cancel. A read that times out fails, and a failure takes nothing from
/// the stream.
///
/// The trap: this passes whatever the backend does with the remainder while
/// the read is in flight, unless the remainder leaves the port for that time.
/// A read that takes it and gives it back only when its completion arrives
/// passes the timeout case and fails the cancel: the next read goes to the
/// kernel, and the bytes the port held come after bytes the peer sent later.
#[test]
fn a_read_that_ends_unanswered_gives_the_port_its_remainder_back() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            let h = crate::primitives::ctx::TestHeap::new();
            let (ours, peer) = stream_socket_pair();
            let port = h.ctx().external(
                "port",
                Port::new_unix_stream(
                    unsafe { OwnedFd::from_raw_fd(ours) },
                    "socketpair".to_string(),
                ),
            );
            let line = |what: &str| {
                let id = submit(
                    &backend,
                    PortOp::ReadLine {
                        buffer: h.ctx().bytes(vec![0u8; 65536]),
                    },
                    port,
                    None,
                );
                answered_bytes(completion_for(&backend, id), what)
            };
            let read_ten = |what: &str| {
                let id = submit(
                    &backend,
                    PortOp::Read {
                        count: 10,
                        buffer: h.ctx().bytes(vec![0u8; 10]),
                    },
                    port,
                    None,
                );
                answered_bytes(completion_for(&backend, id), what)
            };
            let read_twenty = |timeout| {
                submit(
                    &backend,
                    PortOp::ReadExact {
                        count: 20,
                        buffer: h.ctx().bytes(vec![0u8; 20]),
                    },
                    port,
                    timeout,
                )
            };

            // A cancel. The line's read takes the ten bytes after it too, and
            // the port holds them; twenty cannot be answered from ten, so the
            // read-exact waits on a peer that sends nothing more.
            write_all(peer, b"one\n0123456789");
            assert_eq!(line("the first line"), b"one");
            let id = read_twenty(None);
            wait_for_worker(&backend);
            backend.cancel(id).unwrap();
            assert_eq!(
                read_ten("the read after a cancel"),
                b"0123456789",
                "{which}: the read after a cancel must answer with the bytes the \
                 port held before it",
            );

            // A timeout.
            write_all(peer, b"two\nabcdefghij");
            assert_eq!(line("the second line"), b"two");
            let id = read_twenty(Some(std::time::Duration::from_millis(100)));
            let timed_out = completion_for(&backend, id);
            assert!(
                timed_out.result.is_err(),
                "{which}: twenty bytes from a peer that sent ten must time out",
            );
            timed_out.discard();
            assert_eq!(
                read_ten("the read after a timeout"),
                b"abcdefghij",
                "{which}: the read after a timeout must answer with the bytes the \
                 port held before it",
            );

            // Let the cancelled read's completion arrive before the backend goes.
            for _ in 0..40 {
                Completion::discard_all(backend.wait(50).unwrap());
                if !backend.has_pending() && backend.workers() == 0 {
                    break;
                }
            }
            unsafe { libc::close(peer) };
        }
    });
}

/// A pool backend coming to rest waits for a worker that is reading into a
/// region, so the heap does not free the buffer under it.
///
/// The ring drains its kernel operations at teardown for this reason
/// (docs/io.md § "Backend teardown"). A pool worker that reads into the
/// caller's buffer is in the same position, and the teardown must stop it and
/// take its completion before it returns.
///
/// The counter-factual: a teardown that drains only the ring returns with the
/// worker still parked on the silent pipe below, holding the buffer's address.
#[test]
fn a_pool_teardown_waits_for_the_workers_that_address_a_region() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let pipe = Pipe::new();
        let read_end = unsafe { libc::dup(pipe.read_fd) };
        assert!(read_end >= 0, "dup(2) failed");
        let port = h.ctx().external(
            "port",
            Port::new_pipe(
                unsafe { OwnedFd::from_raw_fd(read_end) },
                Direction::Read,
                Encoding::Binary,
                "pipe".to_string(),
            ),
        );
        let backend = AsyncBackend::new_thread_pool().unwrap();
        submit(
            &backend,
            PortOp::Read {
                count: 64,
                buffer: h.ctx().bytes(vec![0u8; 64]),
            },
            port,
            None,
        );
        assert!(
            wait_for_worker(&backend),
            "the pool never took the read out to a worker"
        );

        backend.quiesce();

        assert_eq!(
            backend.workers(),
            0,
            "teardown returned with a worker still reading into a buffer the \
             heap is about to free",
        );
    });
}
