// audited: 2026-09-05
// src/io/AGENTS.md
//! A pool read counts the remainder its port is already holding, in bytes and
//! in grapheme clusters.

use super::*;

/// A connected stream pair. The returned descriptors are the test's to close;
/// `Port` takes the first, the test writes the reply into the second.
fn stream_socket_pair() -> (libc::c_int, libc::c_int) {
    let mut fds = [0 as libc::c_int; 2];
    assert_eq!(
        unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) },
        0,
        "socketpair(2) failed"
    );
    (fds[0], fds[1])
}

/// A `ReadExact` must count the remainder the port is already holding.
///
/// A length-prefixed reply arrives as one burst: the `ReadLine` that takes the
/// header reads a whole chunk, so the body's first bytes come back with it and
/// the completion stashes them as the port's remainder (`submit.rs` § "A
/// previous read on this port took more from the kernel than it answered
/// with"). The `ReadExact` that follows asks for the whole body, and the wire
/// is short of that count by exactly what the port is holding.
///
/// The ring answers this in `uring/drain.rs`: its "enough yet?" test is
/// `state.buffer.len() + filled + got < count`, so the held bytes count toward
/// the total and it stops when the wire has delivered the rest. The pool's
/// runner counts only what it read from the descriptor, so it waits for the
/// full count from a peer that has already said everything it has to say — and
/// a redis `GET` of a value past one chunk hangs forever
/// (`tests/elle/redis-short-read.lisp`, which is gated on a live Redis and so
/// never runs on the macOS CI box, the only one that uses this backend).
///
/// Built on `new_thread_pool` for the reason the rest of this family gives: the
/// ring is the default on a Linux host, and this property would otherwise go
/// unchecked everywhere it can regress.
#[test]
fn a_pool_read_exact_counts_the_remainder_the_port_already_holds() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        use std::os::unix::io::FromRawFd;

        let (ours, peer) = stream_socket_pair();

        // Header, body, terminator — one burst, so the header's read
        // over-reaches into the body. The body is past one 4096-byte
        // `read_until` chunk, so the `ReadExact` still has to reach the wire.
        let body_len = 40_000usize;
        let mut payload = format!("${}\r\n", body_len).into_bytes();
        payload.extend(std::iter::repeat_n(b'x', body_len));
        payload.extend_from_slice(b"\r\n");

        // From a thread: a socketpair's send buffer is smaller than this
        // payload on some platforms (macOS defaults well under it), and a
        // writer that blocks would deadlock the test rather than fail it.
        let writer = std::thread::spawn(move || {
            let mut sent = 0usize;
            while sent < payload.len() {
                let n = unsafe {
                    libc::write(
                        peer,
                        payload[sent..].as_ptr() as *const libc::c_void,
                        payload.len() - sent,
                    )
                };
                assert!(n > 0, "the peer's write failed");
                sent += n as usize;
            }
        });

        let port = h.ctx().external(
            "port",
            Port::new_unix_stream(
                unsafe { std::os::unix::io::OwnedFd::from_raw_fd(ours) },
                "socketpair".to_string(),
            ),
        );
        let backend = AsyncBackend::new_thread_pool().unwrap();

        let line_id = backend
            .submit(
                &IoRequest {
                    op: PortOp::ReadLine {
                        buffer: h.ctx().bytes(vec![0u8; 65536]),
                    }
                    .into(),
                    port,
                    timeout: None,
                },
                crate::io::pending::Submitter::for_test(),
            )
            .unwrap();
        let mut line_done = false;
        for _ in 0..40 {
            for c in backend.wait(50).unwrap() {
                if c.id == line_id {
                    line_done = true;
                }
            }
            if line_done {
                break;
            }
        }
        assert!(line_done, "the header read never completed");

        let count = body_len + 2;
        let exact_id = backend
            .submit(
                &IoRequest {
                    op: PortOp::ReadExact {
                        count,
                        buffer: h.ctx().bytes(vec![0u8; count]),
                    }
                    .into(),
                    port,
                    timeout: None,
                },
                crate::io::pending::Submitter::for_test(),
            )
            .unwrap();

        // Bounded, because the property under test is that this terminates.
        let mut exact = None;
        for _ in 0..80 {
            for c in backend.wait(50).unwrap() {
                if c.id == exact_id {
                    exact = Some(c);
                }
            }
            if exact.is_some() {
                break;
            }
        }
        let exact = exact.expect(
            "the body read never completed — it waited for the whole count from \
             a peer that had already sent everything, because the bytes the \
             header's read left in the port went uncounted",
        );
        assert!(
            exact.result.is_ok(),
            "the body read must answer with the body, not an error",
        );

        writer.join().unwrap();
        unsafe {
            libc::close(peer);
        }
    });
}

/// The same, on a TEXT port, where the count is in grapheme clusters.
///
/// The binary twin above pins that the remainder is counted at all. This one
/// pins that it is counted in the port's own unit: a text `ReadExact` asks for
/// clusters, the remainder holds some of them, and multibyte clusters make the
/// byte count and the cluster count different numbers. A runner that counted
/// only what it read — or counted the remainder in bytes — would ask the peer
/// for clusters it has already sent.
///
/// The ring decides this over `state.buffer` joined to the fiber's bytes
/// (`uring/drain.rs`, the `text_exact` arm), for the reason this runner takes
/// the remainder as bytes rather than as a length: a cluster can straddle the
/// boundary between the two, so neither side can be counted alone.
#[test]
fn a_pool_text_read_exact_counts_the_remainder_in_clusters() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        use std::os::unix::io::FromRawFd;

        let (ours, peer) = stream_socket_pair();

        // Two bytes per cluster, so a byte count and a cluster count can never
        // be mistaken for each other.
        let clusters = 20_000usize;
        let mut payload = b"hdr\n".to_vec();
        for _ in 0..clusters {
            payload.extend_from_slice("é".as_bytes());
        }
        let writer = std::thread::spawn(move || {
            let mut sent = 0usize;
            while sent < payload.len() {
                let n = unsafe {
                    libc::write(
                        peer,
                        payload[sent..].as_ptr() as *const libc::c_void,
                        payload.len() - sent,
                    )
                };
                assert!(n > 0, "the peer's write failed");
                sent += n as usize;
            }
        });

        // A pipe port, because the stream constructors fix the encoding at
        // Binary and the encoding is the whole point here. What the runner
        // reads is a descriptor either way; the counting unit is what differs.
        let port = h.ctx().external(
            "port",
            Port::new_pipe(
                unsafe { std::os::unix::io::OwnedFd::from_raw_fd(ours) },
                Direction::ReadWrite,
                Encoding::Text,
                "socketpair".to_string(),
            ),
        );
        let backend = AsyncBackend::new_thread_pool().unwrap();

        let line_id = backend
            .submit(
                &IoRequest {
                    op: PortOp::ReadLine {
                        buffer: h.ctx().bytes(vec![0u8; 65536]),
                    }
                    .into(),
                    port,
                    timeout: None,
                },
                crate::io::pending::Submitter::for_test(),
            )
            .unwrap();
        let mut line_done = false;
        for _ in 0..40 {
            for c in backend.wait(50).unwrap() {
                if c.id == line_id {
                    line_done = true;
                }
            }
            if line_done {
                break;
            }
        }
        assert!(line_done, "the header read never completed");

        // A text read reserves four bytes per cluster, as `port/read-exact`
        // does — the clusters here are two, so the reservation is ample.
        let exact_id = backend
            .submit(
                &IoRequest {
                    op: PortOp::ReadExact {
                        count: clusters,
                        buffer: h.ctx().bytes(vec![0u8; clusters * 4]),
                    }
                    .into(),
                    port,
                    timeout: None,
                },
                crate::io::pending::Submitter::for_test(),
            )
            .unwrap();

        let mut exact = None;
        for _ in 0..80 {
            for c in backend.wait(50).unwrap() {
                if c.id == exact_id {
                    exact = Some(c);
                }
            }
            if exact.is_some() {
                break;
            }
        }
        let exact = exact.expect(
            "the cluster read never completed — the clusters the header's read \
             left in the port went uncounted",
        );
        assert!(
            exact.result.is_ok(),
            "the cluster read must answer with the text, not an error",
        );

        writer.join().unwrap();
        unsafe {
            libc::close(peer);
        }
    });
}
