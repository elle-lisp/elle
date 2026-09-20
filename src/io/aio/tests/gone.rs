//! audited: 2026-09-20
//! What becomes of an operation whose asking fiber has gone: the answer it
//! gets, the operands it still holds, and the sweep that ends it.
//!
//! docs/impl/io-inflight.md

use super::*;
use crate::value::fiber::FiberStatus;

/// A completion is withheld when the fiber that asked for it has gone.
///
/// A cancel is something a caller must remember to issue, and one caller
/// cannot: a fiber that terminates by a path the scheduler did not route runs
/// to `:dead` with its operation still submitted and nothing marking the id. The
/// entry still holds everything a result would be assembled from, so the
/// question is not whether the assembly is safe — it is whether the result has
/// anywhere to go, and it does not.
///
/// The answer is an error, carrying nothing the entry held. An answer rather
/// than silence, unlike a cancelled operation, because nobody dropped this id —
/// the scheduler still pairs it with the fiber that asked, and retires the
/// pairing on a completion.
///
/// Counter-factual: with the fiber check removed from `PendingTable::take`, the
/// write below resolves `Live` and answers with a byte count instead. This is
/// the only place that counter-factual can be run: a program reaching the same
/// state has no fiber left to receive the answer, so nothing there can tell an
/// error from a result.
#[test]
fn a_completion_is_withheld_when_the_fiber_that_asked_is_gone() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            // A regular file, so the write runs to its end on its own. An
            // operation that parks would leave which of the two happened first
            // — the completion or the fiber's end — up to the machine.
            let path = temp_path("orphaned-asker");
            let heap_ptr = crate::value::arena::leaked_test_heap();
            // SAFETY: the heap is leaked for the process, so this borrow is
            // valid for the whole test.
            let heap = unsafe { &mut *heap_ptr };

            let port_region = heap.new_runtime_region();
            let file = std::fs::File::create(&path).expect("create the file");
            let port = crate::value::build::external(
                heap,
                "port",
                Port::new_file(
                    file.into(),
                    Direction::Write,
                    Encoding::Binary,
                    path.clone(),
                ),
                port_region,
            );

            // The asking fiber, parked in its write when the submission is made.
            let (fiber, handle) =
                crate::value::fiber::test_fiber_in_region(heap, FiberStatus::Paused);
            let region = heap.new_runtime_region();
            let data = crate::primitives::ctx::Alloc::with_region(region, heap).string("late\n");
            let id = backend
                .submit(
                    &IoRequest {
                        op: PortOp::Write { data }.into(),
                        port,
                        timeout: None,
                    },
                    crate::io::pending::Submitter::new(heap_ptr, fiber),
                )
                .unwrap();

            // The fiber ends by a route that told nobody, and releases the
            // region its payload lived in on the way out.
            handle.with_mut(|f| f.status = FiberStatus::Error);
            heap.decref_region(region);

            let mut delivered = Vec::new();
            for _ in 0..40 {
                delivered.extend(backend.wait(50).unwrap());
                if !backend.has_pending() && backend.workers() == 0 {
                    break;
                }
            }
            std::fs::remove_file(&path).ok();

            assert_eq!(
                delivered.len(),
                1,
                "{which}: the operation must answer once — the scheduler holds \
                 this id against the fiber that asked and lets go on a completion",
            );
            let completion = delivered.pop().unwrap();
            assert_eq!(completion.id, id, "{which}: the submitted id came back");
            completion.result.as_ref().expect_err(
                "an operation whose fiber has gone must answer with an error: a \
                 value would be assembled for a reader that is not there",
            );
            completion.discard();
            assert!(
                !backend.has_pending(),
                "{which}: the retired operation kept its pending entry",
            );
            assert_eq!(
                backend.workers(),
                0,
                "{which}: the retired operation never gave its worker back",
            );
        }
    });
}

/// A submitted operation's operands outlive the fiber that asked for them.
///
/// The entry holds `Value`s, and a `Value` is a bare pointer that keeps nothing
/// alive. Nothing else counts a reference held by the pending table, so without
/// the entry's own retain the payload below goes when the fiber releases its
/// region, and every later read of it is a read of freed memory.
///
/// The trap in measuring it: reading freed memory is not what a test can assert
/// on. By the time the read happens the slot has been recycled and reads as
/// whatever its new owner wrote, so a regression comes back with a plausible
/// answer rather than a fault. The store's generation counter moves only on a
/// free, so it says exactly what the release did, and that is what this reads.
///
/// Counter-factual: with the retain removed from `PendingTable::insert`, the
/// generation moves on the release below.
#[test]
fn a_submitted_operations_operands_outlive_the_fiber_that_asked() {
    crate::value::arena::with_test_region(|| {
        let path = temp_path("operand-hold");
        let backend = AsyncBackend::new_thread_pool().unwrap();
        let heap_ptr = crate::value::arena::leaked_test_heap();
        // SAFETY: the heap is leaked for the process, so this borrow is valid
        // for the whole test.
        let heap = unsafe { &mut *heap_ptr };

        let port_region = heap.new_runtime_region();
        let file = std::fs::File::create(&path).expect("create the file");
        let port = crate::value::build::external(
            heap,
            "port",
            Port::new_file(
                file.into(),
                Direction::Write,
                Encoding::Binary,
                path.clone(),
            ),
            port_region,
        );

        // The asking fiber's own region, holding the payload it handed over.
        let region = heap.new_runtime_region();
        let born = heap.region_generation(region.get());
        let data = crate::primitives::ctx::Alloc::with_region(region, heap).string("held\n");
        backend
            .submit(
                &IoRequest {
                    op: PortOp::Write { data }.into(),
                    port,
                    timeout: None,
                },
                crate::io::pending::Submitter::detached(heap_ptr),
            )
            .unwrap();

        // The fiber ends: its region is released while the write still names
        // the payload that lived in it.
        heap.decref_region(region);

        assert_eq!(
            heap.region_generation(region.get()),
            born,
            "the payload's region went with the fiber that asked, while the \
             submitted write still names it",
        );

        // Draining disposes of the entry, which is what lets the hold go.
        for _ in 0..40 {
            Completion::discard_all(backend.wait(50).unwrap());
            if !backend.has_pending() && backend.workers() == 0 {
                break;
            }
        }
        std::fs::remove_file(&path).ok();
        assert_ne!(
            heap.region_generation(region.get()),
            born,
            "the completed operation kept holding its operands' region",
        );
    });
}

/// An operation that PARKS must end when the fiber that asked for it has gone,
/// with no peer ever acting.
///
/// The test above pins the answer such an operation gets. This one is about
/// the completion arriving at all: an accept on a listener nobody connects to
/// waits for an event outside this process, and the fiber that would have
/// received it is what went away, so nothing in the program is left to make
/// that event happen.
///
/// The trap: give the operation a peer and this passes whatever the runtime
/// does — the connection wakes the parked worker and the completion arrives on
/// its own. Nobody connects here, deliberately.
///
/// Counter-factual: without the sweep the accept keeps its `pending` entry and,
/// on the pool, its worker thread; the bounded loop below then runs to its end
/// and delivers nothing.
#[test]
fn an_operation_that_parks_ends_when_the_fiber_that_asked_is_gone() {
    use std::os::unix::io::FromRawFd;
    crate::value::arena::with_test_region(|| {
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            let heap_ptr = crate::value::arena::leaked_test_heap();
            // SAFETY: the heap is leaked for the process, so this borrow is
            // valid for the whole test.
            let heap = unsafe { &mut *heap_ptr };

            // A BLOCKING listener, for the reason
            // `a_cancelled_pool_accept_ends_rather_than_being_abandoned` gives:
            // with `SOCK_NONBLOCK` the worker's `accept` reports EAGAIN at once
            // and the operation ends on its own, which is not the state under
            // test.
            let listener_fd = unsafe {
                let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
                assert!(fd >= 0, "{which}: socket(2) failed");
                let mut addr: libc::sockaddr_in = std::mem::zeroed();
                addr.sin_family = libc::AF_INET as libc::sa_family_t;
                addr.sin_port = 0;
                addr.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::LOCALHOST).to_be();
                assert_eq!(
                    libc::bind(
                        fd,
                        &addr as *const _ as *const libc::sockaddr,
                        std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t
                    ),
                    0,
                    "{which}: bind(2) failed"
                );
                assert_eq!(libc::listen(fd, 128), 0, "{which}: listen(2) failed");
                fd
            };
            let listener_region = heap.new_runtime_region();
            let listener = crate::value::build::external(
                heap,
                "port",
                Port::new_tcp_listener(
                    unsafe { std::os::unix::io::OwnedFd::from_raw_fd(listener_fd) },
                    "127.0.0.1:0".to_string(),
                ),
                listener_region,
            );

            // The asking fiber, and its own region holding the port the accept
            // would have filled in.
            let (fiber, handle) =
                crate::value::fiber::test_fiber_in_region(heap, FiberStatus::Paused);
            let region = heap.new_runtime_region();
            let accept_port = crate::value::build::external(
                heap,
                "port",
                Port::new_unopened(
                    PortKind::TcpStream,
                    Direction::ReadWrite,
                    Encoding::Binary,
                    String::new(),
                ),
                region,
            );

            let id = backend
                .submit(
                    &IoRequest {
                        op: PortOp::Accept {
                            options: Default::default(),
                            encoding: Encoding::Binary,
                            accept_port,
                        }
                        .into(),
                        port: listener,
                        timeout: None,
                    },
                    crate::io::pending::Submitter::new(heap_ptr, fiber),
                )
                .unwrap();

            // The interesting order: the worker is already parked in its wait
            // when the fiber ends under it.
            wait_for_worker(&backend);

            // The fiber ends by a route that told nobody, releasing the region
            // its accept's operand lived in on the way out.
            handle.with_mut(|f| f.status = FiberStatus::Error);
            heap.decref_region(region);

            let mut delivered = Vec::new();
            for _ in 0..40 {
                delivered.extend(backend.wait(50).unwrap());
                if !backend.has_pending() && backend.workers() == 0 {
                    break;
                }
            }

            assert_eq!(
                delivered.len(),
                1,
                "{which}: an accept nobody will ever connect to must end once \
                 the fiber that asked for it is gone",
            );
            let completion = delivered.pop().unwrap();
            assert_eq!(completion.id, id, "{which}: the submitted id came back");
            completion.result.as_ref().expect_err(
                "an operation whose fiber has gone must answer with an error: a \
                 value would be assembled for a reader that is not there",
            );
            completion.discard();
            assert!(
                !backend.has_pending(),
                "{which}: the ended operation kept its pending entry",
            );
            assert_eq!(
                backend.workers(),
                0,
                "{which}: the ended operation never gave its worker back",
            );
        }
    });
}
