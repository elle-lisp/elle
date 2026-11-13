// audited: 2026-11-12
//! Writes to one port, one at a time: the line a write waits in, and the
//! fixtures every test of it shares.
//!
//! docs/impl/io/write.md

use super::bytes::{fill_send_buffer, read_exactly};
use super::*;
use crate::io::request::Bound;
use crate::primitives::ctx::{Alloc, TestHeap};
use crate::value::fiber::FiberStatus;
use std::os::unix::io::{FromRawFd, OwnedFd};
use std::time::Instant;

/// Each write's payload: large enough that the send buffer the peer drains
/// cannot take one in a single syscall.
const PAYLOAD: usize = 64 * 1024;

/// Both platforms, whatever this host would pick.
fn backends() -> [(AsyncBackend, &'static str); 2] {
    [
        (AsyncBackend::new().unwrap(), "the platform default"),
        (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
    ]
}

/// A port on a stream socket whose send buffer is already full, the peer's
/// descriptor, and how many bytes of junk fill the buffer. A write on the port
/// parks until the peer reads.
fn blocked_port(h: &TestHeap) -> (Value, RawFd, usize) {
    let (ours, peer) = stream_socket_pair();
    let queued = fill_send_buffer(ours);
    let port = h.ctx().external(
        "port",
        Port::new_unix_stream(
            unsafe { OwnedFd::from_raw_fd(ours) },
            "socketpair".to_string(),
        ),
    );
    (port, peer, queued)
}

/// Submit the write `op` on `port` under `bound`, on behalf of no fiber.
fn submit_write(backend: &AsyncBackend, op: PortOp, port: Value, bound: Bound) -> SubmissionId {
    backend
        .submit(
            &IoRequest::unbounded(op.into(), port).within(bound),
            crate::io::pending::Submitter::for_test(),
        )
        .unwrap()
}

/// Submit a write of `data` on `port` under `bound`, on behalf of no fiber.
fn write(backend: &AsyncBackend, data: Value, port: Value, bound: Bound) -> SubmissionId {
    submit_write(backend, PortOp::write(data), port, bound)
}

/// A payload of `PAYLOAD` copies of `byte`.
fn payload(h: &TestHeap, byte: u8) -> Value {
    h.ctx().bytes(vec![byte; PAYLOAD])
}

/// Every completion that arrives within `span`.
fn gather_for(backend: &AsyncBackend, span: Duration) -> Vec<Completion> {
    let start = Instant::now();
    let mut got = Vec::new();
    while start.elapsed() < span {
        got.extend(backend.wait(TICK).unwrap());
    }
    got
}

/// Wait until a completion has arrived for each of `ids`, and answer every
/// completion that did. Panics past `PATIENCE`.
fn gather_ids(backend: &AsyncBackend, ids: &[SubmissionId], which: &str) -> Vec<Completion> {
    let start = Instant::now();
    let mut got: Vec<Completion> = Vec::new();
    while !ids.iter().all(|id| got.iter().any(|c| c.id == *id)) {
        assert!(
            start.elapsed() < PATIENCE.unwrap(),
            "{which}: ids {ids:?} did not all complete; arrived: {:?}",
            got.iter().map(|c| c.id).collect::<Vec<_>>(),
        );
        got.extend(backend.wait(TICK).unwrap());
    }
    got
}

/// The completion filed under `id` among `got`.
fn find(got: &[Completion], id: SubmissionId) -> &Completion {
    got.iter()
        .find(|c| c.id == id)
        .expect("a completion for the id")
}

/// The count a completed write answered with, or `None` for an error.
fn count(c: &Completion) -> Option<i64> {
    c.result.as_ref().ok().and_then(|v| v.as_int())
}

/// What the peer can read now without waiting: the bytes, or none at all.
fn ready_bytes(peer: RawFd) -> Vec<u8> {
    set_nonblocking(peer);
    let mut got = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = unsafe { libc::read(peer, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n <= 0 {
            return got;
        }
        got.extend_from_slice(&buf[..n as usize]);
    }
}

/// `PAYLOAD` copies of each byte in `order`, one run after another.
fn runs(order: &[u8]) -> Vec<u8> {
    order.iter().flat_map(|&b| vec![b; PAYLOAD]).collect()
}

/// Let every operation still in flight end, and the peer go.
fn settle(backend: &AsyncBackend, peer: RawFd) {
    for _ in 0..40 {
        Completion::discard_all(backend.wait(TICK).unwrap());
        if !backend.has_pending() && backend.workers() == 0 {
            break;
        }
    }
    unsafe { libc::close(peer) };
}

/// A write submitted while another write on its port is in flight waits in
/// line, out of flight: its id is not among the operations the backend has
/// out.
///
/// The counter-factual: a backend that sends every write at once files the
/// second and third writes in flight beside the first, and the peer then
/// receives runs of each payload in turn.
#[test]
fn a_write_behind_another_on_its_port_waits_out_of_flight() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in backends() {
            let h = TestHeap::new();
            let (port, peer, queued) = blocked_port(&h);
            let a = write(&backend, payload(&h, b'A'), port, Bound::NONE);
            wait_for_worker(&backend);
            let b = write(&backend, payload(&h, b'B'), port, Bound::NONE);
            let c = write(&backend, payload(&h, b'C'), port, Bound::NONE);

            let ids = backend.pending_ids();
            assert!(ids.contains(&a), "{which}: the first write is in flight");
            assert!(
                !ids.contains(&b) && !ids.contains(&c),
                "{which}: a write behind another on its port must wait out of \
                 flight, but {ids:?} has the later writes {b} and {c} out",
            );

            let reader = std::thread::spawn(move || read_exactly(peer, queued + 3 * PAYLOAD));
            Completion::discard_all(gather_ids(&backend, &[a, b, c], which));
            reader.join().expect("the peer's reader");
            settle(&backend, peer);
        }
    });
}

/// Three writes to one port reach the peer whole, one after another, in the
/// order they were submitted.
///
/// The counter-factual: on the pool two workers are each inside `write(2)` on
/// the descriptor, and on the ring a short write's tail follows another
/// write's SQE. Either way the peer receives runs of A, B and C mixed.
#[test]
fn writes_to_one_port_reach_the_peer_whole_and_in_order() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in backends() {
            let h = TestHeap::new();
            let (port, peer, queued) = blocked_port(&h);
            let a = write(&backend, payload(&h, b'A'), port, Bound::NONE);
            wait_for_worker(&backend);
            let b = write(&backend, payload(&h, b'B'), port, Bound::NONE);
            let c = write(&backend, payload(&h, b'C'), port, Bound::NONE);

            let reader = std::thread::spawn(move || read_exactly(peer, queued + 3 * PAYLOAD));
            let got = gather_ids(&backend, &[a, b, c], which);
            for id in [a, b, c] {
                assert_eq!(
                    count(find(&got, id)),
                    Some(PAYLOAD as i64),
                    "{which}: write {id} must report its whole payload",
                );
            }
            Completion::discard_all(got);
            let received = reader.join().expect("the peer's reader");
            assert!(
                received[queued..] == runs(b"ABC")[..],
                "{which}: the peer must receive A, then B, then C, each whole",
            );
            settle(&backend, peer);
        }
    });
}

/// A gathered write waits in the same line as a write of one payload, and
/// reaches the peer whole, its parts in order, between the writes around it.
///
/// The counter-factual: a backend that lines up only a write of one payload
/// sends the gathered write beside the one ahead, and the peer receives its
/// parts mixed into A.
#[test]
fn a_gathered_write_waits_in_the_same_line() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in backends() {
            let h = TestHeap::new();
            let (port, peer, queued) = blocked_port(&h);
            let a = write(&backend, payload(&h, b'A'), port, Bound::NONE);
            wait_for_worker(&backend);
            let parts = h.ctx().array(vec![
                h.ctx().bytes(vec![b'B'; PAYLOAD / 2]),
                h.ctx().string("b".repeat(PAYLOAD / 2)),
            ]);
            let b = submit_write(&backend, PortOp::write_many(parts), port, Bound::NONE);
            let c = write(&backend, payload(&h, b'C'), port, Bound::NONE);
            assert!(
                !backend.pending_ids().contains(&b),
                "{which}: a gathered write behind another on its port must wait \
                 out of flight",
            );

            let reader = std::thread::spawn(move || read_exactly(peer, queued + 3 * PAYLOAD));
            let got = gather_ids(&backend, &[a, b, c], which);
            assert_eq!(
                count(find(&got, b)),
                Some(PAYLOAD as i64),
                "{which}: the gathered write must report every byte of its parts",
            );
            Completion::discard_all(got);
            let received = reader.join().expect("the peer's reader");
            let mut expected = runs(b"A");
            expected.extend(vec![b'B'; PAYLOAD / 2]);
            expected.extend(vec![b'b'; PAYLOAD / 2]);
            expected.extend(runs(b"C"));
            assert!(
                received[queued..] == expected[..],
                "{which}: the peer must receive A, the gathered parts in order, \
                 then C",
            );
            settle(&backend, peer);
        }
    });
}

mod leave;
mod start;
