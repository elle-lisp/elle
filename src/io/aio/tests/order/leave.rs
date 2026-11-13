// audited: 2026-11-12
//! A waiting write that leaves its line before it starts: a cancel, a fiber
//! that is gone, and backend teardown.
//!
//! docs/impl/io/write.md

use super::*;

/// Submit a write of `data` on `port` on behalf of no fiber, filed against
/// `heap`, the heap the payload lives on. A hold reaches only the regions of
/// the heap its submission names, and every `leaked_test_heap` is a new one.
fn write_on(
    backend: &AsyncBackend,
    heap: *mut crate::value::fiberheap::FiberHeap,
    data: Value,
    port: Value,
) -> SubmissionId {
    backend
        .submit(
            &IoRequest::unbounded(PortOp::write(data).into(), port),
            crate::io::pending::Submitter::detached(heap),
        )
        .unwrap()
}

/// A write cancelled while it waits in line sends nothing and answers nothing,
/// and the write behind it takes its place.
///
/// The trap: a cancelled operation in flight also answers nothing, because its
/// completion is retired. What tells the two apart here is the peer: every byte
/// it receives after the junk belongs to A or C.
#[test]
fn a_cancelled_waiting_write_sends_nothing_and_answers_nothing() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in backends() {
            let h = TestHeap::new();
            let (port, peer, queued) = blocked_port(&h);
            let a = write(&backend, payload(&h, b'A'), port, Bound::NONE);
            wait_for_worker(&backend);
            let b = write(&backend, payload(&h, b'B'), port, Bound::NONE);
            let c = write(&backend, payload(&h, b'C'), port, Bound::NONE);
            backend.cancel(b).unwrap();

            let reader = std::thread::spawn(move || read_exactly(peer, queued + 2 * PAYLOAD));
            let mut got = gather_ids(&backend, &[a, c], which);
            got.extend(gather_for(&backend, Duration::from_millis(200)));
            assert!(
                got.iter().all(|c| c.id != b),
                "{which}: the cancelled waiting write {b} answered",
            );
            Completion::discard_all(got);
            let received = reader.join().expect("the peer's reader");
            assert!(
                received[queued..] == runs(b"AC")[..],
                "{which}: the peer must receive A then C, and nothing of B",
            );
            assert!(
                ready_bytes(peer).is_empty(),
                "{which}: the cancelled waiting write sent bytes after C",
            );
            settle(&backend, peer);
        }
    });
}

/// A write cancelled while it waits in line lets go of its payload at the
/// cancel.
///
/// The counter-factual: a cancelled write in flight keeps its payload until
/// its completion arrives, because the kernel or a worker may still read it
/// (docs/impl/io/bytes.md). A waiting write was never handed to either, so
/// holding its payload past the cancel holds it for nobody.
#[test]
fn a_cancelled_waiting_write_lets_go_of_its_payload() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in backends() {
            let h = TestHeap::new();
            let (port, peer, queued) = blocked_port(&h);
            let a = write(&backend, payload(&h, b'A'), port, Bound::NONE);
            wait_for_worker(&backend);

            let heap_ptr = crate::value::arena::leaked_test_heap();
            // SAFETY: the heap is leaked for the process, so this borrow is
            // valid for the whole test.
            let heap = unsafe { &mut *heap_ptr };
            let region = heap.new_runtime_region();
            let born = heap.region_generation(region.get());
            let data = Alloc::with_region(region, heap).bytes(vec![b'B'; PAYLOAD]);
            let b = write_on(&backend, heap_ptr, data, port);
            backend.cancel(b).unwrap();
            heap.decref_region(region);

            assert_ne!(
                heap.region_generation(region.get()),
                born,
                "{which}: a write cancelled in line still holds its payload",
            );

            let reader = std::thread::spawn(move || read_exactly(peer, queued + PAYLOAD));
            Completion::discard_all(gather_ids(&backend, &[a], which));
            reader.join().expect("the peer's reader");
            settle(&backend, peer);
        }
    });
}

/// A waiting write whose fiber ended by a route the scheduler did not take
/// answers with an error, and sends nothing.
///
/// The scheduler still pairs the id with the fiber, and lets go of the pairing
/// only on a completion (docs/impl/io/inflight.md), so the waiting write must
/// answer rather than fall silent.
#[test]
fn a_waiting_write_whose_fiber_is_gone_answers_with_an_error() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in backends() {
            let h = TestHeap::new();
            let (port, peer, queued) = blocked_port(&h);
            let a = write(&backend, payload(&h, b'A'), port, Bound::NONE);
            wait_for_worker(&backend);

            let heap_ptr = crate::value::arena::leaked_test_heap();
            // SAFETY: the heap is leaked for the process, so this borrow is
            // valid for the whole test.
            let heap = unsafe { &mut *heap_ptr };
            let (fiber, handle) =
                crate::value::fiber::test_fiber_in_region(heap, FiberStatus::Paused);
            let b = backend
                .submit(
                    &IoRequest::unbounded(PortOp::write(payload(&h, b'B')).into(), port),
                    crate::io::pending::Submitter::new(heap_ptr, fiber),
                )
                .unwrap();
            handle.with_mut(|f| f.status = FiberStatus::Error);

            let got = gather_ids(&backend, &[b], which);
            assert!(
                find(&got, b).result.is_err(),
                "{which}: a waiting write whose fiber is gone must answer an error",
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
                "{which}: the write whose fiber was gone sent bytes",
            );
            settle(&backend, peer);
        }
    });
}

/// Backend teardown lets go of a waiting write's payload.
#[test]
fn teardown_lets_go_of_a_waiting_writes_payload() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in backends() {
            let h = TestHeap::new();
            let (port, peer, _queued) = blocked_port(&h);
            write(&backend, payload(&h, b'A'), port, Bound::NONE);
            wait_for_worker(&backend);

            let heap_ptr = crate::value::arena::leaked_test_heap();
            // SAFETY: the heap is leaked for the process, so this borrow is
            // valid for the whole test.
            let heap = unsafe { &mut *heap_ptr };
            let region = heap.new_runtime_region();
            let born = heap.region_generation(region.get());
            let data = Alloc::with_region(region, heap).bytes(vec![b'B'; PAYLOAD]);
            write_on(&backend, heap_ptr, data, port);
            heap.decref_region(region);

            backend.quiesce();
            assert_ne!(
                heap.region_generation(region.get()),
                born,
                "{which}: teardown left a waiting write holding its payload",
            );
            unsafe { libc::close(peer) };
        }
    });
}
