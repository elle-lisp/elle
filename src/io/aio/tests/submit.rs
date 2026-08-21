//! The submission frame — what every operation the backend issues shares.
//!
//! A submission mints an id, hands the operation to io_uring or to a
//! thread-pool worker under that id, and files a pending entry keyed by it.
//! The arriving completion carries the id back and is resolved through that
//! entry. An entry filed under any other id is a completion nobody claims and
//! a fiber that never wakes — a hang, not an error — so every operation shape
//! is checked here rather than trusted to the shared frame.
//!
//! `PendingOp::Port` (stream I/O, accept, datagram, shutdown) takes its own
//! route through `submit`; `backend.rs` and `net.rs` pin that one.

use super::*;
use crate::io::request::{SpawnRequest, StdioDisposition, TaskFn};

/// Submit `req` and assert the backend filed exactly one new pending entry,
/// keyed by the id `submit` returned.
fn submit_pending(backend: &AsyncBackend, req: &IoRequest, label: &str) -> SubmissionId {
    let before = backend.pending_ids();
    let id = backend
        .submit(req, crate::value::arena::leaked_test_heap())
        .unwrap_or_else(|e| panic!("{label}: submit failed: {e}"));
    let after = backend.pending_ids();
    assert_eq!(
        after.len(),
        before.len() + 1,
        "{label}: one submission files one pending entry"
    );
    assert!(
        !before.contains(&id) && after.contains(&id),
        "{label}: the pending entry is keyed by the id submit returned"
    );
    id
}

/// Block for `id`'s completion and assert it is the one that arrives, and that
/// it consumed the pending entry the submission filed.
fn expect_completion(backend: &AsyncBackend, id: SubmissionId, label: &str) -> Completion {
    let mut completions = backend
        .wait(-1)
        .unwrap_or_else(|e| panic!("{label}: wait failed: {e}"));
    assert_eq!(
        completions.len(),
        1,
        "{label}: one submission yields one completion"
    );
    let completion = completions.pop().unwrap();
    assert_eq!(
        completion.id, id,
        "{label}: the completion carries the submitted id"
    );
    assert!(
        backend.pending_ids().is_empty(),
        "{label}: the completion consumed its pending entry"
    );
    completion
}

#[test]
fn a_sleep_completes_under_the_id_it_was_submitted_with() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        let req = IoRequest {
            op: IoOp::Sleep {
                duration: Duration::from_millis(1),
            },
            port: Value::NIL,
            timeout: None,
        };
        let id = submit_pending(&backend, &req, "sleep");
        expect_completion(&backend, id, "sleep");
    });
}

#[test]
fn a_resolve_completes_under_the_id_it_was_submitted_with() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        // getaddrinfo(3) has no io_uring form, so this dispatches to the
        // thread pool on every platform.
        let req = IoRequest {
            op: IoOp::Resolve {
                hostname: "localhost".to_string(),
            },
            port: Value::NIL,
            timeout: None,
        };
        let id = submit_pending(&backend, &req, "resolve");
        expect_completion(&backend, id, "resolve");
    });
}

#[test]
fn a_task_completes_under_the_id_it_was_submitted_with() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        // An arbitrary closure has no io_uring form either.
        let req = IoRequest {
            op: IoOp::Task(TaskFn::new(Box::new(|| (0, b"done".to_vec())))),
            port: Value::NIL,
            timeout: None,
        };
        let id = submit_pending(&backend, &req, "task");
        expect_completion(&backend, id, "task");
    });
}

#[test]
fn a_readiness_poll_completes_under_the_id_it_was_submitted_with() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        // stderr accepts writes, so POLLOUT is ready the moment it is armed.
        let req = IoRequest {
            op: IoOp::PollFd {
                fd: 2,
                events: libc::POLLOUT as u32,
            },
            port: Value::NIL,
            timeout: None,
        };
        let id = submit_pending(&backend, &req, "poll-fd");
        expect_completion(&backend, id, "poll-fd");
    });
}

#[test]
fn an_open_completes_under_the_id_it_was_submitted_with() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let backend = AsyncBackend::new().unwrap();
        let path = write_temp_file("open me");
        // The unopened port the completion fills in, exactly as `port/open`
        // pre-allocates it at the call site.
        let port = h.ctx().external(
            "port",
            Port::new_unopened(
                PortKind::File,
                Direction::Read,
                Encoding::Text,
                path.clone(),
            ),
        );
        let req = IoRequest {
            op: IoOp::Open {
                path: path.clone(),
                flags: libc::O_RDONLY,
                mode: 0o666,
                direction: Direction::Read,
                encoding: Encoding::Text,
            },
            port,
            timeout: None,
        };
        let id = submit_pending(&backend, &req, "open");
        let completion = expect_completion(&backend, id, "open");
        assert!(completion.result.is_ok(), "open: the file opened");

        std::fs::remove_file(&path).ok();
    });
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
#[test]
fn a_watch_read_completes_under_the_id_it_was_submitted_with() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let backend = AsyncBackend::new().unwrap();
        let dir = temp_path("watch");
        std::fs::create_dir(&dir).unwrap();

        let watcher = crate::io::watch::FsWatcher::new().unwrap();
        watcher.add(&dir, false).unwrap();
        // Queue an event before submitting, so the read has something to
        // return and the test never waits on the filesystem.
        std::fs::write(std::path::Path::new(&dir).join("a.txt"), b"x").unwrap();

        let req = IoRequest {
            op: IoOp::WatchNext,
            port: h.ctx().external("fs-watcher", watcher),
            timeout: None,
        };
        let id = submit_pending(&backend, &req, "watch-next");
        expect_completion(&backend, id, "watch-next");

        std::fs::remove_dir_all(&dir).ok();
    });
}

#[test]
fn a_spawn_completes_inside_the_submit_call() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        let req = IoRequest {
            op: IoOp::Spawn(SpawnRequest {
                program: "true".to_string(),
                args: Vec::new(),
                env: None,
                cwd: None,
                stdin: StdioDisposition::Null,
                stdout: StdioDisposition::Null,
                stderr: StdioDisposition::Null,
            }),
            port: Value::NIL,
            timeout: None,
        };
        let id = backend
            .submit(&req, crate::value::arena::leaked_test_heap())
            .unwrap();
        assert!(
            backend.pending_ids().is_empty(),
            "spawn: the child is started in the submit call, so nothing is in flight"
        );

        // The completion is already queued, so the non-blocking poll finds it.
        let completions = backend.poll();
        assert_eq!(
            completions.len(),
            1,
            "spawn: one completion, queued already"
        );
        assert_eq!(
            completions[0].id, id,
            "spawn: the completion carries the submitted id"
        );
    });
}

#[test]
fn a_process_wait_completes_under_the_id_it_was_submitted_with() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        // `sleep` rather than `true`: a child that has already exited takes the
        // cached-exit-code path, which files no pending entry at all.
        let spawn = IoRequest {
            op: IoOp::Spawn(SpawnRequest {
                program: "sleep".to_string(),
                args: vec!["0.2".to_string()],
                env: None,
                cwd: None,
                stdin: StdioDisposition::Null,
                stdout: StdioDisposition::Null,
                stderr: StdioDisposition::Null,
            }),
            port: Value::NIL,
            timeout: None,
        };
        backend
            .submit(&spawn, crate::value::arena::leaked_test_heap())
            .unwrap();
        let spawned = backend.poll().pop().unwrap().result.unwrap();
        let handle = sorted_struct_get(
            spawned.as_struct().unwrap(),
            &TableKey::Keyword("process".into()),
        )
        .expect("spawn result carries a :process handle");

        let req = IoRequest {
            op: IoOp::ProcessWait,
            port: *handle,
            timeout: None,
        };
        let id = submit_pending(&backend, &req, "process-wait");
        expect_completion(&backend, id, "process-wait");
    });
}
