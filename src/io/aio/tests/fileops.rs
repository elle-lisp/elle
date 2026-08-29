use super::*;

#[test]
fn test_async_seek_returns_immediate_completion() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        let path = write_temp_file("hello world");
        let port = open_rw_port(&path);

        let req = IoRequest {
            op: IoOp::Seek {
                offset: 6,
                whence: libc::SEEK_SET,
            },
            port,
            timeout: None,
        };
        let id = backend
            .submit(&req, crate::value::arena::leaked_test_heap())
            .unwrap();

        // Seek is immediate — no wait needed
        let completions = backend.poll();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(completions[0].result.is_ok());
        assert_eq!(completions[0].result.as_ref().unwrap().as_int(), Some(6));

        std::fs::remove_file(&path).ok();
    });
}

#[test]
fn test_async_tell_returns_immediate_completion() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        let path = write_temp_file("hello");
        let port = open_rw_port(&path);

        let req = IoRequest {
            op: IoOp::Tell,
            port,
            timeout: None,
        };
        let id = backend
            .submit(&req, crate::value::arena::leaked_test_heap())
            .unwrap();

        let completions = backend.poll();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(completions[0].result.is_ok());
        assert_eq!(completions[0].result.as_ref().unwrap().as_int(), Some(0));

        std::fs::remove_file(&path).ok();
    });
}

#[test]
fn test_async_seek_non_file_port_errors() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let backend = AsyncBackend::new().unwrap();
        let stdin_port = h.ctx().external("port", Port::stdin());

        let req = IoRequest {
            op: IoOp::Seek {
                offset: 0,
                whence: libc::SEEK_SET,
            },
            port: stdin_port,
            timeout: None,
        };
        // stdin has PortKind::Stdin — seek must fail immediately
        let id = backend
            .submit(&req, crate::value::arena::leaked_test_heap())
            .unwrap();
        let completions = backend.poll();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(completions[0].result.is_err());
    });
}

#[test]
fn test_async_submit_spawn_echo() {
    crate::value::arena::with_test_region(|| {
        use crate::io::request::{SpawnRequest, StdioDisposition};
        let backend = AsyncBackend::new().unwrap();
        let req = IoRequest {
            op: IoOp::Spawn(SpawnRequest {
                program: "/bin/echo".to_string(),
                args: vec!["hello-async".to_string()],
                env: None,
                cwd: None,
                stdin: StdioDisposition::Null,
                stdout: StdioDisposition::Pipe,
                stderr: StdioDisposition::Null,
            }),
            port: Value::NIL,
            timeout: None,
        };
        let id = backend
            .submit(&req, crate::value::arena::leaked_test_heap())
            .unwrap();
        let completions = backend.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        let val = completions[0].result.as_ref().expect("spawn failed");
        let fields = val.as_struct().expect("expected struct");
        assert!(
            sorted_struct_get(fields, &TableKey::Keyword("pid".into()))
                .unwrap()
                .as_int()
                .unwrap()
                > 0
        );
    });
}

/// Test IORING_OP_WAITID via async backend.
/// Requires Linux kernel 6.7+. The test skips gracefully on older kernels
/// by checking for -EINVAL completion.
#[test]
#[cfg(target_os = "linux")]
fn test_async_submit_process_wait_uring() {
    crate::value::arena::with_test_region(|| {
        use crate::io::request::{IoOp, IoRequest, ProcessHandle};

        let child = std::process::Command::new("/bin/true").spawn().unwrap();
        let pid = child.id();
        let h = crate::primitives::ctx::TestHeap::new();
        let handle = ProcessHandle::new(pid, child);
        let handle_val = h.ctx().external("process", handle);

        let backend = AsyncBackend::new().unwrap();
        let req = IoRequest {
            op: IoOp::ProcessWait,
            port: handle_val,
            timeout: None,
        };
        let id = backend.submit(&req, crate::value::arena::leaked_test_heap());

        match id {
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
/// The failure is arranged by reaping the child first, so the worker's
/// `waitpid` finds no child and returns `ECHILD` — the same shape any lost
/// child produces.
#[test]
fn a_failed_pool_process_wait_names_waitpid() {
    crate::value::arena::with_test_region(|| {
        // Resolved through `PATH`, not hardcoded: no absolute path is right
        // everywhere. macOS ships no `/bin/true`, and a busybox image ships no
        // `/usr/bin/true`. Either way the spawn would fail with `ENOENT` before
        // this test reaches what it pins.
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        // Reaped here, so the pool worker's own `waitpid` has no child left.
        child.wait().unwrap();

        let h = crate::primitives::ctx::TestHeap::new();
        let handle_val = h
            .ctx()
            .external("process", ProcessHandle::new(pid, child_stub()));

        let backend = AsyncBackend::new_thread_pool().unwrap();
        let id = backend
            .submit(
                &IoRequest {
                    op: IoOp::ProcessWait,
                    port: handle_val,
                    timeout: None,
                },
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        let completions = backend.wait(5000).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        let err = completions[0]
            .result
            .as_ref()
            .expect_err("a wait for a child that is gone must fail");
        let fields = err.as_struct().expect("an io error is a struct");
        let msg = sorted_struct_get(fields, &TableKey::Keyword("message".into()))
            .and_then(|v| v.with_string(|s| s.to_string()))
            .expect("an io error carries a :message");
        assert!(
            msg.contains("waitpid failed"),
            "the pool's process wait must name waitpid, the call it makes; got {msg:?}",
        );
    });
}

/// A reaped placeholder child for a `ProcessHandle` whose real child is already
/// gone. `ProcessHandle::new` demands a `Child`, and its `Drop` calls
/// `try_wait`, so the stand-in is a process that has already exited.
#[cfg(test)]
fn child_stub() -> std::process::Child {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    child.wait().unwrap();
    child
}

// ── IoOp::Open integration tests ─────────────────────────────────────────

#[test]
fn test_async_open_regular_file_returns_port() {
    crate::value::arena::with_test_region(|| {
        let path = temp_path("async-open");
        std::fs::write(&path, "async open test").unwrap();

        let h = crate::primitives::ctx::TestHeap::new();
        let port_val = h.ctx().external(
            "port",
            Port::new_unopened(
                PortKind::File,
                Direction::Read,
                Encoding::Text,
                path.clone(),
            ),
        );
        let backend = AsyncBackend::new().unwrap();
        let req = IoRequest {
            op: IoOp::Open {
                path: path.clone(),
                flags: libc::O_RDONLY | libc::O_CLOEXEC,
                mode: 0o666,
                direction: Direction::Read,
                encoding: Encoding::Text,
            },
            port: port_val,
            timeout: None,
        };
        let id = backend
            .submit(&req, crate::value::arena::leaked_test_heap())
            .unwrap();
        let completions = backend.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(
            completions[0].result.is_ok(),
            "open should succeed for existing file: {:?}",
            completions[0].result
        );
        // Result must be a port value
        let val = completions[0].result.as_ref().unwrap();
        assert_eq!(
            val.external_type_name(),
            Some("port"),
            "open result must be a port"
        );

        std::fs::remove_file(&path).ok();
    });
}

/// Regression: a backend dropped with an io_uring op still in flight must
/// not leave the kernel holding a write pointer into a buffer it is about to
/// free. A `read-all` on a pipe whose write end is held open and empty blocks
/// in the kernel — it stays in flight. `quiesce_pending` cancels and drains
/// such ops on teardown, so an op's `BufferPool` slot is never freed while the
/// kernel still owns it (which would let the eventual write corrupt the heap:
/// `malloc(): unsorted double linked list corrupted`). Here we prove the
/// mechanism deterministically: `quiesce()` cancels the in-flight read and
/// reaps it, so `has_pending()` goes false.
///
/// Counter-factual: with `quiesce_pending` stubbed to a no-op, the cancel is
/// never issued, the blocked read stays pending, and the final assertion
/// fails — exactly the unfixed state.
#[test]
#[cfg(target_os = "linux")]
fn test_drop_with_inflight_read_cancels_and_drains() {
    use std::os::unix::io::FromRawFd;
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        // Only the io_uring path has the async kernel-write-into-buffer hazard;
        // the thread-pool fallback can't reproduce it (and can't cancel a
        // blocked worker read), so there is nothing to assert there.
        if !backend.is_uring() {
            return;
        }

        // pipe2: [read, write]. The write end stays open with no data, so a
        // read on the read end blocks indefinitely — the op stays in flight.
        let mut fds = [0 as libc::c_int; 2];
        assert_eq!(
            unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) },
            0,
            "pipe2 failed"
        );
        let (read_fd, write_fd) = (fds[0], fds[1]);
        let h = crate::primitives::ctx::TestHeap::new();
        let read_owned = unsafe { std::os::unix::io::OwnedFd::from_raw_fd(read_fd) };
        let port = h.ctx().external(
            "port",
            Port::new_file(
                read_owned,
                Direction::Read,
                Encoding::Binary,
                "<pipe>".into(),
            ),
        );

        let req = IoRequest {
            op: PortOp::ReadAll.into(),
            port,
            timeout: None,
        };
        backend
            .submit(&req, crate::value::arena::leaked_test_heap())
            .unwrap();
        assert!(
            backend.has_pending(),
            "read on an empty pipe should be in flight"
        );

        // Cancel + drain to quiescence before any buffer/ring frees.
        backend.quiesce();
        assert!(
            !backend.has_pending(),
            "quiesce must cancel and reap the in-flight op"
        );

        // The write end isn't owned by the port; close it explicitly. (The
        // read end is owned by the Port and closed when the value drops.)
        unsafe { libc::close(write_fd) };
    });
}

#[test]
fn test_async_open_nonexistent_path_errors() {
    crate::value::arena::with_test_region(|| {
        let path = "/nonexistent/elle-test-async-open-dir/nofile";
        let backend = AsyncBackend::new().unwrap();
        let req = IoRequest {
            op: IoOp::Open {
                path: path.to_string(),
                flags: libc::O_RDONLY | libc::O_CLOEXEC,
                mode: 0o666,
                direction: crate::port::Direction::Read,
                encoding: crate::port::Encoding::Text,
            },
            port: Value::NIL,
            timeout: None,
        };
        let id = backend
            .submit(&req, crate::value::arena::leaked_test_heap())
            .unwrap();
        let completions = backend.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(
            completions[0].result.is_err(),
            "open must error for nonexistent path"
        );
    });
}

#[test]
fn test_async_open_with_timeout_succeeds_on_regular_file() {
    crate::value::arena::with_test_region(|| {
        let path = temp_path("async-open-timeout");
        std::fs::write(&path, "timeout test").unwrap();

        let h = crate::primitives::ctx::TestHeap::new();
        let port_val = h.ctx().external(
            "port",
            Port::new_unopened(
                PortKind::File,
                Direction::Read,
                Encoding::Text,
                path.clone(),
            ),
        );
        let backend = AsyncBackend::new().unwrap();
        let req = IoRequest {
            op: IoOp::Open {
                path: path.clone(),
                flags: libc::O_RDONLY | libc::O_CLOEXEC,
                mode: 0o666,
                direction: Direction::Read,
                encoding: Encoding::Text,
            },
            port: port_val,
            timeout: Some(std::time::Duration::from_millis(5000)),
        };
        let id = backend
            .submit(&req, crate::value::arena::leaked_test_heap())
            .unwrap();
        let completions = backend.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        // Regular file opens instantly — should succeed before the 5s timeout.
        assert!(
            completions[0].result.is_ok(),
            "open with generous timeout must succeed for regular file: {:?}",
            completions[0].result
        );

        std::fs::remove_file(&path).ok();
    });
}
