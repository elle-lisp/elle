// audited: 2026-10-20
//! A line longer than the buffer `read-line` reserves is one answer, on either backend.
//!
//! docs/impl/io-bytes.md

use super::bytes::{answered_bytes, completion_for, submit, write_all};
use super::*;
use std::os::unix::io::{FromRawFd, OwnedFd};

/// A line longer than the buffer a `read-line` reserves is one answer on
/// either backend, and the read after it answers the next line.
///
/// The trap: a read that answers with the full buffer when it fills with no
/// newline hands back a piece nothing marks as one, because `read-line` drops
/// the newline from every answer. A line of exactly the buffer's length then
/// leaves its newline for the next read, which answers an empty line the
/// stream does not hold.
///
/// The counter-factual: a line under the buffer passes however the backend
/// treats a full one, so the lines here are the buffer's length exactly, one
/// byte over it, and three times it.
#[test]
fn a_line_longer_than_its_buffer_is_one_answer_on_either_backend() {
    const BUFFER: usize = 65536;
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
                        buffer: h.ctx().bytes(vec![0u8; BUFFER]),
                    },
                    port,
                    None,
                );
                answered_bytes(completion_for(&backend, id), what)
            };

            for len in [BUFFER, BUFFER + 1, 3 * BUFFER] {
                // Digits by position, so a wrong byte shows as well as a wrong
                // length.
                let long: Vec<u8> = (0..len).map(|i| b'0' + (i % 10) as u8).collect();
                let mut payload = long.clone();
                payload.extend_from_slice(b"\nyy\n");
                // From a thread: the payload is past a socketpair's send buffer
                // on some platforms, and a writer that blocks here would
                // deadlock the test rather than fail it.
                let writer = std::thread::spawn(move || write_all(peer, &payload));

                let got = line("the long line");
                assert_eq!(
                    got.len(),
                    len,
                    "{which}: a {len}-byte line must be one answer, not a {}-byte piece",
                    got.len(),
                );
                assert!(
                    got == long,
                    "{which}: the {len}-byte line must come back byte for byte"
                );
                assert_eq!(
                    line("the line after"),
                    b"yy",
                    "{which}: the read after a {len}-byte line must answer the next \
                     line, not a piece of the first or an empty line",
                );
                writer.join().expect("the peer's writer");
            }
            unsafe { libc::close(peer) };
        }
    });
}
