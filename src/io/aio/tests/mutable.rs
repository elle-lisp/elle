// audited: 2026-11-11
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

            let id = submit(&backend, PortOp::write(payload), port, None);
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

/// A gathered write whose parts can change while it waits sends them as they
/// were at submission, every part of it, whatever the program does meanwhile.
///
/// Two shapes. An array holding an `@bytes` beside an immutable part: the
/// program overwrites and grows the `@bytes`, and overwrites the other part in
/// place. An `@array` of immutable parts, which another fiber could replace:
/// the program overwrites each part in place. The write parks on a full send
/// buffer first, as `a_mutable_payload_is_written_as_it_was_at_submission`
/// describes.
///
/// The counter-factual: a write that copied only the mutable part and sent the
/// other from where it lies sends a `B` for every byte of the immutable part. A
/// write that copied nothing sends the `@bytes`' overwritten bytes too.
#[test]
fn a_gathered_write_of_mutable_parts_sends_them_as_submitted() {
    const PART: usize = 32 * 1024;
    crate::value::arena::with_test_region(|| {
        for (in_mutable_array, backend, which) in [
            (
                false,
                AsyncBackend::new().unwrap(),
                "an @bytes part, the platform default",
            ),
            (
                true,
                AsyncBackend::new().unwrap(),
                "an @array, the platform default",
            ),
            (
                false,
                AsyncBackend::new_thread_pool().unwrap(),
                "an @bytes part, the thread pool",
            ),
            (
                true,
                AsyncBackend::new_thread_pool().unwrap(),
                "an @array, the thread pool",
            ),
        ] {
            let h = crate::primitives::ctx::TestHeap::new();
            let (ours, peer) = stream_socket_pair();
            let queued = fill_send_buffer(ours);
            let fixed = h.ctx().bytes(vec![b'A'; PART]);
            let (payload, changing) = if in_mutable_array {
                let other = h.ctx().string("A".repeat(PART));
                (h.ctx().array_mut(vec![fixed, other]), other)
            } else {
                let store = h.ctx().bytes_mut(vec![b'A'; PART]);
                (h.ctx().array(vec![store, fixed]), store)
            };
            let port = h.ctx().external(
                "port",
                Port::new_unix_stream(
                    unsafe { OwnedFd::from_raw_fd(ours) },
                    "socketpair".to_string(),
                ),
            );

            let id = submit(&backend, PortOp::write_many(payload), port, None);
            wait_for_worker(&backend);
            // SAFETY: nothing reads the parts but the parked write, and the
            // heap they live on outlives this loop iteration.
            unsafe { super::gather::overwrite(&fixed, b'B') };
            if in_mutable_array {
                unsafe { super::gather::overwrite(&changing, b'B') };
            } else {
                let heap = h.heap();
                crate::value::arena::bytes_splice(heap, changing, 0..PART, &[b'B'; PART]);
                crate::value::arena::bytes_append(heap, changing, &[b'C'; PART]);
            }

            let total = 2 * PART;
            let reader = std::thread::spawn(move || read_exactly(peer, queued + total));
            let done = completion_for(&backend, id);
            assert_eq!(
                done.result.as_ref().ok().and_then(|v| v.as_int()),
                Some(total as i64),
                "{which}: the write must report the parts it was submitted with",
            );
            done.discard();
            let got = reader.join().expect("the peer's reader");
            unsafe { libc::close(peer) };

            let sent = &got[queued..];
            assert!(
                sent.iter().all(|&b| b == b'A'),
                "{which}: the peer received {} bytes the program wrote after the \
                 submission — the write read a part where it lay",
                sent.iter().filter(|&&b| b != b'A').count(),
            );
        }
    });
}
