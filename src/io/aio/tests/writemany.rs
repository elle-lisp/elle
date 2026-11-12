// audited: 2026-11-12
//! A gathered write hands the kernel each part where it lies, not a joined copy.
//!
//! docs/impl/io-bytes.md

use super::bytes::{completion_for, fill_send_buffer, read_exactly, submit};
use super::*;
use std::os::unix::io::{FromRawFd, OwnedFd};

/// The region bytes of an immutable string or bytes part.
fn region_bytes(part: &Value) -> &[u8] {
    part.as_bytes()
        .or_else(|| part.as_str().map(str::as_bytes))
        .expect("a part is an immutable string or bytes")
}

/// Write `byte` over every byte of an immutable string or bytes part, where
/// it lies.
///
/// # Safety
///
/// Nothing reads the part but a parked write, and its heap outlives the call.
pub(super) unsafe fn overwrite(part: &Value, byte: u8) {
    let bytes = region_bytes(part);
    std::ptr::write_bytes(bytes.as_ptr() as *mut u8, byte, bytes.len());
}

/// A port over our end of a stream socket pair.
fn socket_port(h: &crate::primitives::ctx::TestHeap, fd: RawFd) -> Value {
    h.ctx().external(
        "port",
        Port::new_unix_stream(
            unsafe { OwnedFd::from_raw_fd(fd) },
            "socketpair".to_string(),
        ),
    )
}

/// A gathered write hands the kernel each part's own address, not a copy of
/// the parts joined.
///
/// The peer's send buffer is full before the write is submitted, so it parks
/// as `a_write_hands_the_kernel_the_payload_where_it_lies` describes. Every
/// part, a string among them, is overwritten while it waits.
///
/// The counter-factual: a write that joined its parts at submission sends
/// them as they were then, every `A`. One that reads each part where it lies
/// sends the `B`s it finds there when the peer makes room.
#[test]
fn a_gathered_write_hands_the_kernel_each_part_where_it_lies() {
    const PART: usize = 32 * 1024;
    crate::value::arena::with_test_region(|| {
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            let h = crate::primitives::ctx::TestHeap::new();
            let (ours, peer) = stream_socket_pair();
            let queued = fill_send_buffer(ours);
            let parts = [
                h.ctx().bytes(vec![b'A'; PART]),
                h.ctx().string("A".repeat(PART)),
                h.ctx().bytes(vec![b'A'; PART]),
            ];
            let payload = h.ctx().array(parts.to_vec());
            let port = socket_port(&h, ours);

            let id = submit(&backend, PortOp::write_many(payload), port, None);
            wait_for_worker(&backend);
            for part in &parts {
                // SAFETY: nothing reads the parts but the parked write, and
                // the heap they live on outlives this loop iteration.
                unsafe { overwrite(part, b'B') };
            }

            let total = 3 * PART;
            let reader = std::thread::spawn(move || read_exactly(peer, queued + total));
            let done = completion_for(&backend, id);
            assert_eq!(
                done.result.as_ref().ok().and_then(|v| v.as_int()),
                Some(total as i64),
                "{which}: the write must report every byte of its parts",
            );
            done.discard();
            let got = reader.join().expect("the peer's reader");
            unsafe { libc::close(peer) };

            let sent = &got[queued..];
            assert!(
                sent.iter().all(|&b| b == b'B'),
                "{which}: the peer received {} bytes of the parts as they were \
                 at submission — the write copied them rather than handing the \
                 kernel each part's address",
                sent.iter().filter(|&&b| b == b'A').count(),
            );
        }
    });
}
