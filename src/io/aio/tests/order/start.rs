// audited: 2026-11-12
//! When a waiting write starts, and whether it does: its `:timeout`, its
//! `:deadline`, a port closed while it waits, and the bytes it sends.
//!
//! docs/impl/io/write.md

use super::*;

/// The `:error` kind of a failed completion.
fn error_kind(c: &Completion) -> Option<Value> {
    let err = c.result.as_ref().err()?;
    let fields = err.as_struct()?;
    crate::value::sorted_struct_get(fields, &TableKey::keyword("error")).copied()
}

/// A waiting write's `:timeout` starts when the write starts, not while it
/// waits in line.
///
/// The counter-factual: a write sent at once parks on the full send buffer,
/// and its 100 ms timeout ends it long before the peer reads.
#[test]
fn a_waiting_writes_timeout_starts_when_it_starts() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in backends() {
            let h = TestHeap::new();
            let (port, peer, queued) = blocked_port(&h);
            let a = write(&backend, payload(&h, b'A'), port, Bound::NONE);
            wait_for_worker(&backend);
            let b = write(
                &backend,
                payload(&h, b'B'),
                port,
                Bound::per_op(Duration::from_millis(100)),
            );

            let early = gather_for(&backend, Duration::from_millis(500));
            assert!(
                early.iter().all(|c| c.id != b),
                "{which}: the write waiting in line timed out on its :timeout",
            );
            Completion::discard_all(early);

            let reader = std::thread::spawn(move || read_exactly(peer, queued + 2 * PAYLOAD));
            let got = gather_ids(&backend, &[a, b], which);
            assert_eq!(
                count(find(&got, b)),
                Some(PAYLOAD as i64),
                "{which}: the waiting write must run to its end once it starts",
            );
            Completion::discard_all(got);
            let received = reader.join().expect("the peer's reader");
            assert!(
                received[queued..] == runs(b"AB")[..],
                "{which}: the peer must receive A, then B",
            );
            settle(&backend, peer);
        }
    });
}

/// A waiting write whose `:deadline` passes in line answers `:timeout` and
/// sends nothing, and the backend's wait wakes for it.
///
/// The trap: the write ahead never ends while the peer does not read, so no
/// completion comes to wake the wait. A wait that blocks for its whole bound
/// here answers the deadline seconds late.
#[test]
fn a_deadline_that_passes_in_line_ends_the_write_unsent() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in backends() {
            let h = TestHeap::new();
            let (port, peer, queued) = blocked_port(&h);
            let a = write(&backend, payload(&h, b'A'), port, Bound::NONE);
            wait_for_worker(&backend);
            let deadline = Instant::now() + Duration::from_millis(100);
            let b = write(
                &backend,
                payload(&h, b'B'),
                port,
                Bound::new(None, Some(deadline)),
            );

            let started = Instant::now();
            let mut got = Vec::new();
            while got.iter().all(|c: &Completion| c.id != b)
                && started.elapsed() < Duration::from_secs(5)
            {
                got.extend(backend.wait(Some(Duration::from_secs(5))).unwrap());
            }
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "{which}: the wait answered {:?} after a 100 ms deadline",
                started.elapsed(),
            );
            assert_eq!(
                error_kind(find(&got, b)),
                Some(Value::keyword("timeout")),
                "{which}: a deadline that passes in line must answer :timeout",
            );
            assert!(
                backend.pending_ids().contains(&a),
                "{which}: the write ahead is still in flight",
            );
            Completion::discard_all(got);

            let reader = std::thread::spawn(move || read_exactly(peer, queued + PAYLOAD));
            Completion::discard_all(gather_ids(&backend, &[a], which));
            let received = reader.join().expect("the peer's reader");
            assert!(
                received[queued..] == runs(b"A")[..],
                "{which}: A arrives whole"
            );
            assert!(
                ready_bytes(peer).is_empty(),
                "{which}: the write whose deadline passed in line sent bytes",
            );
            settle(&backend, peer);
        }
    });
}

/// A waiting write whose bytes must be copied sends them as they were when it
/// was submitted, not as they are when it starts.
///
/// The write behind A is an `@bytes`. While it waits in line the program
/// overwrites every byte in place and then grows the store, which moves its
/// bytes to a new extent.
///
/// The counter-factual: a write that takes its copy when it starts copies the
/// store as the program left it, and the peer receives `X` for every byte of
/// B.
#[test]
fn a_waiting_write_sends_its_mutable_bytes_as_submitted() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in backends() {
            let h = TestHeap::new();
            let (port, peer, queued) = blocked_port(&h);
            let a = write(&backend, payload(&h, b'A'), port, Bound::NONE);
            wait_for_worker(&backend);
            let store = h.ctx().bytes_mut(vec![b'B'; PAYLOAD]);
            let b = write(&backend, store, port, Bound::NONE);
            let heap = h.heap();
            crate::value::arena::bytes_splice(heap, store, 0..PAYLOAD, &[b'X'; PAYLOAD]);
            crate::value::arena::bytes_append(heap, store, &[b'Y'; PAYLOAD]);

            let reader = std::thread::spawn(move || read_exactly(peer, queued + 2 * PAYLOAD));
            let got = gather_ids(&backend, &[a, b], which);
            assert_eq!(
                count(find(&got, b)),
                Some(PAYLOAD as i64),
                "{which}: the waiting write must report the bytes it was submitted with",
            );
            Completion::discard_all(got);
            let received = reader.join().expect("the peer's reader");
            assert!(
                received[queued..] == runs(b"AB")[..],
                "{which}: the peer must receive A, then B as it was submitted, but \
                 {} bytes of B were written after the submission",
                received[queued..].iter().filter(|&&x| x == b'X').count(),
            );
            settle(&backend, peer);
        }
    });
}

/// Closing the port fails the write that waits on it, and the write sends
/// nothing.
#[test]
fn closing_the_port_fails_a_waiting_write() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in backends() {
            let h = TestHeap::new();
            let (port, peer, queued) = blocked_port(&h);
            let a = write(&backend, payload(&h, b'A'), port, Bound::NONE);
            wait_for_worker(&backend);
            let b = write(&backend, payload(&h, b'B'), port, Bound::NONE);
            let close = backend
                .submit(
                    &IoRequest::unbounded(IoOp::Close, port),
                    crate::io::pending::Submitter::for_test(),
                )
                .unwrap();

            let got = gather_ids(&backend, &[a, b, close], which);
            assert!(
                find(&got, b).result.is_err(),
                "{which}: a write waiting on a port that closed must fail",
            );
            Completion::discard_all(got);
            let junk = read_exactly(peer, queued);
            assert!(
                junk.iter().all(|&x| x == b'j'),
                "{which}: the junk comes first"
            );
            assert!(
                !ready_bytes(peer).contains(&b'B'),
                "{which}: the write waiting on a closed port sent bytes",
            );
            settle(&backend, peer);
        }
    });
}
