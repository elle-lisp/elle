//! audited: 2026-09-20
//! A descriptor number stays out of the OS's hands while an operation names it,
//! even after the value that owned it is gone.
//!
//! docs/impl/io-descriptor.md

use super::*;
use crate::value::fiber::FiberStatus;

/// A descriptor number stays out of the OS's hands while a submitted operation
/// names it, even when the port that owned it goes with its fiber's regions.
///
/// `port/close` is not the only way a port ends. A fiber can terminate by a
/// route the scheduler did not take — `fiber/abort` injects an error the
/// fiber's own `protect` catches — so it runs to `:dead` with its operation
/// still submitted and releases the regions holding its port on the way out.
/// Nothing cancels, and nothing asks the backend to hold anything.
///
/// The trap: a worker resolves its fd at syscall entry, not at submit time. A
/// number handed back before the worker gets there can be given to a new
/// socket, and the worker reads that socket instead — its bytes going to a
/// completion no fiber is waiting for. The sweep narrows that window and does
/// not close it: between the release and the worker observing its stop, the
/// number belongs to the OS.
///
/// The trap in measuring it: an fd-number count cannot say this. The suite
/// shares a process and runs in parallel, so the number moves under the
/// measurement. What is stable is what the number REFERS to — the port takes a
/// duplicate of the pipe's read end, so the pipe's own end stays behind as a
/// witness of the file description both name.
///
/// Counter-factual: with the port owning its descriptor outright, the region
/// release closes the number and the first assertion reads `None` — or reads
/// whatever else in the process was handed the number in the meantime, which
/// is the defect itself.
#[test]
fn a_port_freed_with_its_fibers_regions_keeps_its_descriptor_number() {
    use std::os::unix::io::FromRawFd;
    crate::value::arena::with_test_region(|| {
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            // A pipe nobody writes: the read parks, so the operation is still
            // in flight when the region goes.
            let pipe = Pipe::new();
            let port_fd = unsafe { libc::dup(pipe.read_fd) };
            assert!(port_fd >= 0, "{which}: dup(2) failed");
            let pipe_identity = file_identity(pipe.read_fd)
                .unwrap_or_else(|| panic!("{which}: fstat on the pipe's read end failed"));

            let heap_ptr = crate::value::arena::leaked_test_heap();
            // SAFETY: the heap is leaked for the process, so this borrow is
            // valid for the whole test.
            let heap = unsafe { &mut *heap_ptr };

            // The asking fiber's own region, holding the port itself — which is
            // what makes the release that ends the fiber the thing that ends
            // the port.
            let region = heap.new_runtime_region();
            let port = crate::value::build::external(
                heap,
                "port",
                Port::new_file(
                    // SAFETY: `dup(2)` handed this number over above and
                    // nothing else holds it.
                    unsafe { std::os::unix::io::OwnedFd::from_raw_fd(port_fd) },
                    Direction::Read,
                    Encoding::Binary,
                    "<pipe>".into(),
                ),
                region,
            );

            let (fiber, handle) =
                crate::value::fiber::test_fiber_in_region(heap, FiberStatus::Paused);
            let id = backend
                .submit(
                    &IoRequest {
                        op: PortOp::ReadAll.into(),
                        port,
                        timeout: None,
                    },
                    crate::io::pending::Submitter::new(heap_ptr, fiber),
                )
                .unwrap();

            // The interesting order: the worker is already parked on the number
            // when the fiber ends under it.
            wait_for_worker(&backend);

            // The fiber ends: its region is released, and the port with it.
            handle.with_mut(|f| f.status = FiberStatus::Error);
            heap.decref_region(region);

            assert_eq!(
                file_identity(port_fd),
                Some(pipe_identity),
                "{which}: descriptor {port_fd} no longer names the pipe the \
                 submitted read is on — the number went back to the OS with the \
                 port's region, so the next socket to take it is the one that \
                 read's worker reads",
            );

            // The operation ends on its own once its fiber has gone, and the
            // number goes back with the entry that held the last share of it.
            let mut delivered = Vec::new();
            for _ in 0..40 {
                delivered.extend(completion_ids(backend.wait(50).unwrap()));
                if !backend.has_pending() && backend.workers() == 0 {
                    break;
                }
            }
            assert_eq!(
                delivered,
                vec![id],
                "{which}: the read must answer once — the scheduler holds this \
                 id against the fiber that asked and lets go on a completion",
            );
            assert_ne!(
                file_identity(port_fd),
                Some(pipe_identity),
                "{which}: descriptor {port_fd} still names the pipe after the \
                 operation holding it was retired — a share that outlives its \
                 operation costs one descriptor per operation",
            );
        }
    });
}

/// A watcher's descriptor number stays out of the OS's hands while a submitted
/// read names it, even when the watcher goes with its fiber's regions.
///
/// The port case above is the same invariant on the thing that owns a
/// descriptor most often. This is the other owner: `WatchNext` reads the inotify
/// (Linux) or kqueue (macOS) descriptor an `FsWatcher` owns, and the external
/// hands out no share of it the way a `Port` does. What keeps the number is the
/// entry's hold on the region the watcher lives in — a live external has not
/// dropped its `OwnedFd`.
///
/// The trap in measuring it: as in the port case, an fd-number count cannot say
/// this, because the suite shares a process and runs in parallel. A duplicate of
/// the watcher's descriptor, taken before the region is released, is the witness
/// of the file description both numbers name.
///
/// Counter-factual: with the operand hold removed, the release frees the region,
/// the `FsWatcher` drops its `OwnedFd`, and the first assertion reads `None` —
/// or reads whatever else in the process was handed the number meanwhile, which
/// is the defect itself.
#[test]
fn a_watcher_freed_with_its_fibers_regions_keeps_its_descriptor_number() {
    crate::value::arena::with_test_region(|| {
        let dir = temp_path("watch-hold");
        std::fs::create_dir(&dir).unwrap();

        let backend = AsyncBackend::new_thread_pool().unwrap();
        let heap_ptr = crate::value::arena::leaked_test_heap();
        // SAFETY: the heap is leaked for the process, so this borrow is valid
        // for the whole test.
        let heap = unsafe { &mut *heap_ptr };

        let watcher = crate::io::watch::FsWatcher::new().unwrap();
        watcher.add(&dir, false).unwrap();
        let watch_fd = watcher.raw_fd().expect("an open watcher has a descriptor");
        let watch_identity =
            file_identity(watch_fd).expect("fstat on the watcher's descriptor failed");
        // The witness: a second number for the same file description, so the
        // identity survives the watcher letting go of its own.
        let witness = unsafe { libc::dup(watch_fd) };
        assert!(witness >= 0, "dup(2) failed");

        // The asking fiber's own region, holding the watcher itself.
        let (fiber, handle) = crate::value::fiber::test_fiber_in_region(heap, FiberStatus::Paused);
        let region = heap.new_runtime_region();
        let watcher_val = crate::value::build::external(heap, "fs-watcher", watcher, region);

        let id = backend
            .submit(
                &IoRequest {
                    op: IoOp::WatchNext,
                    port: watcher_val,
                    timeout: None,
                },
                crate::io::pending::Submitter::new(heap_ptr, fiber),
            )
            .unwrap();

        // The interesting order: the worker is already parked on the number
        // when the fiber ends under it.
        wait_for_worker(&backend);

        // The fiber ends: its region is released, and the watcher with it.
        handle.with_mut(|f| f.status = FiberStatus::Error);
        heap.decref_region(region);

        assert_eq!(
            file_identity(watch_fd),
            Some(watch_identity),
            "descriptor {watch_fd} no longer names the watcher the submitted \
             read is on — the number went back to the OS with the watcher's \
             region, so the next file to take it is the one that read's worker \
             reads",
        );

        // The operation ends on its own once its fiber has gone, and the number
        // goes back with the hold that kept the watcher alive.
        let mut delivered = Vec::new();
        for _ in 0..40 {
            delivered.extend(completion_ids(backend.wait(50).unwrap()));
            if !backend.has_pending() && backend.workers() == 0 {
                break;
            }
        }
        assert_eq!(
            delivered,
            vec![id],
            "the watch must answer once — the scheduler holds this id against \
             the fiber that asked and lets go on a completion",
        );
        assert_ne!(
            file_identity(watch_fd),
            Some(watch_identity),
            "descriptor {watch_fd} still names the watcher after the operation \
             holding it was retired — a hold that outlives its operation costs \
             one descriptor per operation",
        );

        unsafe { libc::close(witness) };
        std::fs::remove_dir(&dir).ok();
    });
}
