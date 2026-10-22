// audited: 2026-10-22
//! A write of a mutable payload, which leaves from a copy taken at submission.
//!
//! docs/impl/io-bytes.md

use super::bytes::{completion_for, fill_send_buffer, read_exactly, submit};
use super::*;
use std::os::unix::io::{FromRawFd, OwnedFd};

/// A write of an `@bytes` sends the bytes it was submitted with, whatever the
/// program does to the store while the write waits.
///
/// The peer's send buffer is full before the write is submitted, so the write
/// parks as `a_write_hands_the_kernel_the_payload_where_it_lies` describes.
/// While it waits, the program overwrites every byte in place and then grows the
/// store past its capacity, which moves the bytes to a new extent.
///
/// The counter-factual: a write that handed the kernel the store's address
/// sends the overwritten bytes, which the extent it was handed holds. The
/// growth leaves that extent dead but readable, so it would not fault.
#[test]
fn a_mutable_payload_is_written_as_it_was_at_submission() {
    const PAYLOAD: usize = 64 * 1024;
    crate::value::arena::with_test_region(|| {
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            let h = crate::primitives::ctx::TestHeap::new();
            let (ours, peer) = stream_socket_pair();
            let queued = fill_send_buffer(ours);
            let payload = h.ctx().bytes_mut(vec![b'A'; PAYLOAD]);
            let port = h.ctx().external(
                "port",
                Port::new_unix_stream(
                    unsafe { OwnedFd::from_raw_fd(ours) },
                    "socketpair".to_string(),
                ),
            );

            let id = submit(&backend, PortOp::Write { data: payload }, port, None);
            wait_for_worker(&backend);
            let heap = h.heap();
            crate::value::arena::bytes_splice(heap, payload, 0..PAYLOAD, &[b'B'; PAYLOAD]);
            crate::value::arena::bytes_append(heap, payload, &[b'C'; PAYLOAD]);

            let reader = std::thread::spawn(move || read_exactly(peer, queued + PAYLOAD));
            let done = completion_for(&backend, id);
            assert_eq!(
                done.result.as_ref().ok().and_then(|v| v.as_int()),
                Some(PAYLOAD as i64),
                "{which}: the write must report the payload it was submitted with",
            );
            done.discard();
            let got = reader.join().expect("the peer's reader");
            unsafe { libc::close(peer) };

            let sent = &got[queued..];
            assert!(
                sent.iter().all(|&b| b == b'A'),
                "{which}: the peer received {} bytes the program wrote after the \
                 submission — the write read the store where it lay",
                sent.iter().filter(|&&b| b != b'A').count(),
            );
        }
    });
}
