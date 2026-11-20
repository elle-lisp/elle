// audited: 2026-11-20
//! A file walk through the backend: a cancel and the sweep of a gone fiber reach it on either platform, and a teardown does not wait for it.
//!
//! docs/impl/io/cancel.md

use super::*;
use crate::io::request::FileOp;
use crate::scratch::ScratchDir;
use crate::value::fiber::FiberStatus;

/// How many files the deleted tree holds. Removing them takes milliseconds,
/// and the stop under test is written microseconds after the submission.
const FILES: usize = 20_000;

/// A directory of [`FILES`] empty files.
fn big_tree(dir: &ScratchDir, name: &str) -> String {
    let top = dir.join_str(name);
    std::fs::create_dir(&top).unwrap();
    for i in 0..FILES {
        std::fs::write(format!("{top}/f{i}"), "").unwrap();
    }
    top
}

fn delete(path: &str) -> IoRequest {
    let op = FileOp::DeleteDir {
        path: path.to_string(),
        recursive: true,
    };
    IoRequest::unbounded(IoOp::File(op), Value::NIL)
}

/// Reap until nothing is pending, and answer what was delivered.
fn reap(backend: &AsyncBackend) -> Vec<Completion> {
    let mut delivered = Vec::new();
    for _ in 0..200 {
        if !backend.has_pending() {
            break;
        }
        delivered.extend(backend.wait(TICK).unwrap());
    }
    assert!(!backend.has_pending(), "the walk never reported back");
    delivered
}

/// The trap: a `File` operation runs on a pool worker even where the ring runs
/// the rest, so the ring's `IORING_OP_ASYNC_CANCEL` names an id the ring never
/// saw. The counter-factual asks only the ring, and the whole tree goes.
#[test]
fn a_cancel_reaches_a_walk_on_either_platform() {
    crate::value::arena::with_test_region(|| {
        let dir = ScratchDir::new("walk-cancel");
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            let top = big_tree(&dir, which);
            let id = backend
                .submit(&delete(&top), crate::io::pending::Submitter::for_test())
                .unwrap();
            backend.cancel(id).unwrap();
            let delivered = reap(&backend);
            assert!(
                delivered.is_empty(),
                "{which}: a cancelled walk answers nobody"
            );
            assert!(
                std::path::Path::new(&top).is_dir(),
                "{which}: the cancel stopped the delete before it reached the end"
            );
        }
    });
}

/// A walk whose fiber is gone is asked to stop as a parked read is, and answers
/// with the error that says so.
#[test]
fn a_walk_whose_fiber_is_gone_is_stopped_on_either_platform() {
    crate::value::arena::with_test_region(|| {
        let dir = ScratchDir::new("walk-gone");
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            let top = big_tree(&dir, which);
            let heap_ptr = crate::value::arena::leaked_test_heap();
            // SAFETY: the heap is leaked for the process.
            let heap = unsafe { &mut *heap_ptr };
            let (fiber, _handle) =
                crate::value::fiber::test_fiber_in_region(heap, FiberStatus::Error);
            let id = backend
                .submit(
                    &delete(&top),
                    crate::io::pending::Submitter::new(heap_ptr, fiber),
                )
                .unwrap();
            let delivered = reap(&backend);
            assert_eq!(completion_ids(delivered), vec![id], "{which}: it answers");
            assert!(
                std::path::Path::new(&top).is_dir(),
                "{which}: the sweep stopped the delete before it reached the end"
            );
        }
    });
}

/// A backend coming to rest waits for the pool operations that address a
/// region, and for no walk. A step on a mount that has stopped answering would
/// hold the teardown up for as long as the mount does.
///
/// The set is read before any completion is drained, so the walk's entry and
/// its stop pipe are both still there, however soon the walk finished.
#[test]
fn a_teardown_waits_for_no_file_walk() {
    crate::value::arena::with_test_region(|| {
        let dir = ScratchDir::new("walk-rest");
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            let list = IoRequest::unbounded(
                IoOp::File(FileOp::List {
                    path: dir.as_str().to_string(),
                }),
                Value::NIL,
            );
            let walk = backend
                .submit(&list, crate::io::pending::Submitter::for_test())
                .unwrap();
            let sleep = IoRequest::unbounded(
                IoOp::Sleep {
                    duration: Duration::from_secs(60),
                },
                Value::NIL,
            );
            let timer = backend
                .submit(&sleep, crate::io::pending::Submitter::for_test())
                .unwrap();
            let awaited = backend.inner.borrow().awaited_at_rest();
            assert!(!awaited.contains(&walk), "{which}: the walk is not awaited");
            // The ring runs a timer in the kernel, with no stop pipe to wait on.
            #[cfg(target_os = "linux")]
            let pool = !backend.is_uring();
            #[cfg(not(target_os = "linux"))]
            let pool = true;
            if pool {
                assert!(awaited.contains(&timer), "{which}: the timer is");
            }
            backend.quiesce();
        }
    });
}
