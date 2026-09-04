//! `subprocess/wait` through the async backend.

use super::*;
use crate::io::request::{reaped_child, zombie_child};

/// Submit a wait on `handle` through `backend`.
fn submit_wait(backend: &AsyncBackend, handle: Value) -> Result<SubmissionId, String> {
    backend.submit(
        &IoRequest {
            op: IoOp::ProcessWait,
            port: handle,
            timeout: None,
        },
        crate::io::pending::Submitter::for_test(),
    )
}

/// The `:message` of an io error struct.
fn error_message(err: &Value) -> String {
    let fields = err.as_struct().expect("an io error is a struct");
    sorted_struct_get(fields, &TableKey::keyword("message"))
        .and_then(|v| v.with_string(|s| s.to_string()))
        .expect("an io error carries a :message")
}

/// Test IORING_OP_WAITID via async backend.
/// Requires Linux kernel 6.7+. The test skips gracefully on older kernels
/// by checking for -EINVAL completion.
#[test]
#[cfg(target_os = "linux")]
fn test_async_submit_process_wait_uring() {
    crate::value::arena::with_test_region(|| {
        let child = std::process::Command::new("/bin/true").spawn().unwrap();
        let pid = child.id();
        let h = crate::primitives::ctx::TestHeap::new();
        let handle_val = h.ctx().external("process", ProcessHandle::new(pid, child));

        let backend = AsyncBackend::new().unwrap();
        match submit_wait(&backend, handle_val) {
            Err(e) if e.contains("thread-pool") => {
                // Thread-pool backend: ProcessWait not supported. Skip.
            }
            Err(e) => panic!("submit failed unexpectedly: {}", e),
            Ok(id) => {
                let completions = backend.wait(5000).unwrap();
                assert_eq!(completions.len(), 1);
                assert_eq!(completions[0].id, id);
                match &completions[0].result {
                    Err(e) => {
                        // -EINVAL means IORING_OP_WAITID not supported on this kernel. Skip.
                        let msg = format!("{:?}", e);
                        if msg.contains("22")
                            || msg.contains("EINVAL")
                            || msg.contains("waitid failed")
                        {
                            return; // kernel < 6.7
                        }
                        panic!("ProcessWait failed: {:?}", e);
                    }
                    Ok(val) => {
                        assert_eq!(val.as_int(), Some(0), "expected exit 0");
                    }
                }
            }
        }
    });
}

/// A failed process wait names the syscall the platform actually called.
///
/// One completion arm serves both backends, and they call different things:
/// `IORING_OP_WAITID` on the ring, `waitpid(2)` in the pool worker
/// (`src/io/threadpool/child.rs`). The trap: a report that names `waitid` on a
/// platform that has no `waitid` call in the path sends its reader looking for
/// code that is not there.
///
/// The failure is arranged with a child this process never spawned, so the
/// worker's `waitpid` finds no child of its own and returns `ECHILD` — the same
/// shape any lost child produces. A child this process DID spawn and reap would
/// answer from the record instead, which is the whole point of § "A reap is
/// never wasted"; the pid below belongs to nobody, so there is nothing to hold.
#[test]
fn a_failed_pool_process_wait_names_waitpid() {
    crate::value::arena::with_test_region(|| {
        // Already reaped by `reaped_child`, so the pool worker's own `waitpid`
        // has no child left — and the stand-in's `ProcessHandle` carries an
        // empty record, because nothing in this process reaped through one.
        let pid = reaped_child().id();

        let h = crate::primitives::ctx::TestHeap::new();
        let handle_val = h
            .ctx()
            .external("process", ProcessHandle::new(pid, reaped_child()));

        let backend = AsyncBackend::new_thread_pool().unwrap();
        let id = submit_wait(&backend, handle_val).unwrap();

        let completions = backend.wait(5000).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        let err = completions[0]
            .result
            .as_ref()
            .expect_err("a wait for a child that is gone must fail");
        let msg = error_message(err);
        assert!(
            msg.contains("waitpid failed"),
            "the pool's process wait must name waitpid, the call it makes; got {msg:?}",
        );
    });
}

/// A wait a cancel discarded still leaves the child's exit status for the next
/// waiter.
///
/// This is the shape `ev/timeout` around a `subprocess/wait` produces, and it
/// cancels on every call: the wait reaps the child in the instant between the
/// deadline and the worker noticing it, the cancel means its completion reaches
/// nobody, and the next `subprocess/wait` on the same handle is entitled to the
/// status the first one abandoned.
///
/// Both halves are forced rather than waited for. The child is a zombie before
/// the submission, so the reap happens on the operation's first ask, and
/// nothing drains between the submit and the cancel, so the mark lands while
/// the entry is still in flight.
///
/// The counter-factual: with the retired entry dropping the status instead of
/// keeping it, the second wait finds no child and fails with `waitpid failed:
/// errno 10 (No child processes)`.
#[test]
fn a_cancelled_wait_that_reaped_the_child_answers_the_next_wait() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let child = zombie_child();
        let handle = h
            .ctx()
            .external("process", ProcessHandle::new(child.id(), child));

        let backend = AsyncBackend::new_thread_pool().unwrap();
        let cancelled = submit_wait(&backend, handle).unwrap();
        backend.cancel(cancelled).unwrap();

        // A cancelled operation delivers nothing, which is what makes the
        // status the record's to keep rather than the completion's to carry.
        for _ in 0..40 {
            assert!(
                backend.wait(50).unwrap().is_empty(),
                "a cancelled wait must deliver no completion",
            );
            if !backend.has_pending() {
                break;
            }
        }
        assert!(!backend.has_pending(), "the cancelled wait never retired");

        let again = submit_wait(&backend, handle).unwrap();
        let completions = backend.wait(5000).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, again);
        let value = match &completions[0].result {
            Ok(v) => *v,
            Err(e) => panic!(
                "the status the cancelled wait reaped was lost: {}",
                error_message(e)
            ),
        };
        assert_eq!(
            value.as_int(),
            Some(1),
            "the second wait must report the status `false` exited with",
        );
    });
}

/// A wait on a child whose status this process is already holding issues no
/// operation at all.
///
/// The answer alone would not pin this: an operation that did go out would find
/// the same record and report the same status. What is pinned is that nothing
/// is submitted — no pending entry, no worker, no `waitpid` — for a child there
/// is nothing left to reap.
#[test]
fn a_wait_on_a_held_status_files_no_operation() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let child = zombie_child();
        let handle = h
            .ctx()
            .external("process", ProcessHandle::new(child.id(), child));

        let backend = AsyncBackend::new_thread_pool().unwrap();
        submit_wait(&backend, handle).unwrap();
        assert_eq!(backend.wait(5000).unwrap().len(), 1, "the first wait reaps");

        let held = submit_wait(&backend, handle).unwrap();
        assert!(
            backend.pending_ids().is_empty(),
            "a wait answered from the record files no entry for a completion \
             that will never arrive",
        );
        assert_eq!(backend.workers(), 0, "and takes no worker out");

        let completions = backend.poll();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, held);
        assert_eq!(
            completions[0].result.as_ref().map(|v| v.as_int()),
            Ok(Some(1)),
            "the held status is the answer",
        );
    });
}

/// A wait that finds no child is answered from the record rather than reported
/// as a failure.
///
/// Two waits on one child can be in flight at once, and the kernel gives the
/// status to one of them. The loser's `ECHILD` is what this arm turns back into
/// the status: the winner reaped first, so its completion is cooked first and
/// the record is already there.
///
/// Built at the entry rather than run: the window is a scheduling accident on
/// the ring, and nothing in a test can make two `waitid`s collide on demand.
/// The counter-factual is the arm below it — without the record the same
/// completion comes back as "waitpid failed: errno 10".
#[test]
fn a_wait_that_finds_no_child_answers_from_the_record() {
    crate::value::arena::with_test_region(|| {
        let mut pending = crate::io::pending::PendingTable::new();
        let mut fd_states = std::collections::HashMap::new();
        let mut pool = crate::io::pool::BufferPool::new();
        let heap = crate::value::arena::leaked_test_heap();

        // The state the winner's completion leaves behind: a status held for a
        // child the kernel no longer has.
        let exit = crate::io::request::ExitRecord::new();
        exit.keep(7);

        let id = SubmissionId::from_raw(1);
        pending.insert(
            id,
            crate::io::pending::PendingOp::ProcessWait {
                buffer_handle: pool.alloc(0),
                handle_val: Value::NIL,
                // The pool shape: the worker calls `waitpid` itself, so the
                // kernel filled no `siginfo_t` for this entry.
                siginfo: std::ptr::null_mut(),
                exit,
            },
            crate::io::pending::Submitter::detached(heap),
        );

        let completion = crate::io::aio::convert::pool_to_completion(
            crate::io::threadpool::PoolCompletion {
                id: id.as_u64(),
                kind: crate::io::pending::OpKind::ProcessWait,
                result_code: -libc::ECHILD,
                data: Vec::new(),
            },
            &mut pending,
            &mut fd_states,
            &mut pool,
            heap,
            crate::config::get().unicode_generation(),
        )
        .expect("a live submission is cooked");

        let value = completion
            .result
            .expect("a status this process holds is an answer, not a failure");
        assert_eq!(value.as_int(), Some(7));
    });
}

/// The same guarantee on the ring, where the kernel reaps rather than a worker.
///
/// The status arrives in the `siginfo_t` the CQE fills, and a cancelled entry
/// is retired rather than cooked — so the retire is what has to read it out.
/// Skipped where there is no ring, and on a kernel without
/// `IORING_OP_WAITID`: that one reaps nothing, so both waits report the
/// `EINVAL` it answers with.
#[test]
#[cfg(target_os = "linux")]
fn a_cancelled_uring_wait_that_reaped_the_child_answers_the_next_wait() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        if !backend.is_uring() {
            return;
        }
        let h = crate::primitives::ctx::TestHeap::new();
        let child = zombie_child();
        let handle = h
            .ctx()
            .external("process", ProcessHandle::new(child.id(), child));

        let cancelled = submit_wait(&backend, handle).unwrap();
        backend.cancel(cancelled).unwrap();
        for _ in 0..40 {
            assert!(
                backend.wait(50).unwrap().is_empty(),
                "a cancelled wait must deliver no completion",
            );
            if !backend.has_pending() {
                break;
            }
        }
        assert!(!backend.has_pending(), "the cancelled wait never retired");

        let again = submit_wait(&backend, handle).unwrap();
        let completions = backend.wait(5000).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, again);
        let value = match &completions[0].result {
            Ok(v) => *v,
            Err(e) => {
                let msg = error_message(e);
                if msg.contains("errno 22") {
                    return; // kernel < 6.7: nothing was ever reaped
                }
                panic!("the status the cancelled wait reaped was lost: {msg}");
            }
        };
        assert_eq!(
            value.as_int(),
            Some(1),
            "the second wait must report the status `false` exited with",
        );
    });
}
