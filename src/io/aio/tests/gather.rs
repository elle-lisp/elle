// audited: 2026-11-11
//! A gathered write: each part handed to the kernel where it lies, and every part sent in order.
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

/// Shrink a socket's send buffer, so every write to it is short.
fn small_send_buffer(fd: RawFd) {
    let size: libc::c_int = 4096;
    let rc = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &size as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    assert_eq!(rc, 0, "setsockopt(SO_SNDBUF) failed");
}

/// A gathered write sends every part, in order, however the kernel cuts it.
///
/// A small send buffer cuts the write inside a part again and again, and 3000
/// parts are more than one `writev` carries, so the operation goes out as
/// several writes. Each part is numbered in its bytes, and one in five is
/// empty. The parts come in an array and then in a list, on each backend.
///
/// The counter-factual: a resubmission that restarts the part it stopped in,
/// or skips to the next one, sends the right parts in the wrong number; one
/// that stops at IOV_MAX parts reports a short count.
#[test]
fn a_gathered_write_sends_its_parts_in_order_across_short_writes() {
    crate::value::arena::with_test_region(|| {
        for (as_list, backend, which) in [
            (
                false,
                AsyncBackend::new().unwrap(),
                "an array, the platform default",
            ),
            (
                true,
                AsyncBackend::new().unwrap(),
                "a list, the platform default",
            ),
            (
                false,
                AsyncBackend::new_thread_pool().unwrap(),
                "an array, the thread pool",
            ),
            (
                true,
                AsyncBackend::new_thread_pool().unwrap(),
                "a list, the thread pool",
            ),
        ] {
            let h = crate::primitives::ctx::TestHeap::new();
            let (ours, peer) = stream_socket_pair();
            small_send_buffer(ours);
            let texts: Vec<String> = (0..3000)
                .map(|i| match i % 5 {
                    0 => String::new(),
                    _ => format!("<{i}:{}>", "x".repeat(i % 97)),
                })
                .collect();
            let parts: Vec<Value> = texts
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    if i % 2 == 0 {
                        h.ctx().string(t)
                    } else {
                        h.ctx().bytes(t.as_bytes().to_vec())
                    }
                })
                .collect();
            let payload = if as_list {
                h.ctx().list(parts)
            } else {
                h.ctx().array(parts)
            };
            let expected: Vec<u8> = texts.concat().into_bytes();
            let port = socket_port(&h, ours);

            let total = expected.len();
            let reader = std::thread::spawn(move || read_exactly(peer, total));
            let id = submit(&backend, PortOp::write_many(payload), port, None);
            let done = completion_for(&backend, id);
            assert_eq!(
                done.result.as_ref().ok().and_then(|v| v.as_int()),
                Some(total as i64),
                "{which}: the write must report every byte of its parts",
            );
            done.discard();
            let got = reader.join().expect("the peer's reader");
            unsafe { libc::close(peer) };
            let first_wrong = got.iter().zip(&expected).position(|(a, b)| a != b);
            assert_eq!(
                first_wrong, None,
                "{which}: the peer's bytes leave the parts' order at this offset",
            );
        }
    });
}
