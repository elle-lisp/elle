//! The pool operations that park with no peer, and what ends them.
//!
//! Each of these waits for an event that may never arrive: a child that never
//! exits, a descriptor nothing writes, a directory nothing touches, a fifo
//! whose other end nobody opens. A worker inside the blocking form of any of
//! them is unreachable — `io/cancel` cannot retract a syscall already running —
//! so the operation would outlive the fiber that asked for it and cost one OS
//! thread for the life of the process.
//!
//! `assert_cancel_retires` is the shared assertion: the worker comes back and
//! the `pending` entry goes, within a bounded number of waits. Its twins for
//! the socket calls live in `net.rs`.
//!
//! `io_uring` runs most of these in the kernel, so the cancellation tests build
//! a thread-pool backend explicitly rather than taking the platform default.
//! The last test is the exception: it holds both backends to one answer.

use super::*;
use std::time::Duration;

/// A pipe whose ends close with it.
struct Pipe {
    read_fd: RawFd,
    write_fd: RawFd,
}

impl Pipe {
    fn new() -> Pipe {
        let mut fds = [0 as libc::c_int; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe(2) failed");
        Pipe {
            read_fd: fds[0],
            write_fd: fds[1],
        }
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.read_fd);
            libc::close(self.write_fd);
        }
    }
}

/// A fifo that unlinks with itself.
struct Fifo(String);

impl Fifo {
    fn new(tag: &str) -> Fifo {
        let path = temp_path(tag);
        let c_path = std::ffi::CString::new(path.as_str()).unwrap();
        assert_eq!(
            unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) },
            0,
            "mkfifo failed"
        );
        Fifo(path)
    }
}

impl Drop for Fifo {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
}

/// A cancelled subprocess wait must END on the thread-pool backend.
///
/// `subprocess/wait` on a child that never exits is the shape: `waitpid(pid,
/// .., 0)` holds the worker for the child's whole life. `ev/timeout` around a
/// wait is how a supervisor gives a stuck child a deadline, and the cancel it
/// issues has to reach the worker.
#[test]
fn a_cancelled_pool_process_wait_ends_rather_than_being_abandoned() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        // `sleep 30` outlives the wait, so the wait ends because it was
        // cancelled rather than because the child happened to exit.
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        let handle = h.ctx().external("process", ProcessHandle::new(pid, child));

        let backend = AsyncBackend::new_thread_pool().unwrap();
        let id = backend
            .submit(
                &IoRequest {
                    op: IoOp::ProcessWait,
                    port: handle,
                    timeout: None,
                },
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        assert_cancel_retires(&backend, id, "process wait");

        // Reap the child here rather than leaving it to the handle's Drop,
        // which only tries once and would leave a zombie behind.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
            let mut status = 0;
            libc::waitpid(pid as libc::pid_t, &mut status, 0);
        }
    });
}

/// A cancelled readiness wait must END on the thread-pool backend.
///
/// `ev/poll-fd` with no `:timeout` waits for as long as the descriptor stays
/// quiet — `wayland/event-loop` and `glib-wait` park there on every iteration.
/// Without a stop pipe the worker sits in `poll(2)` with `-1` and nothing but
/// the descriptor itself can end it.
#[test]
fn a_cancelled_pool_poll_fd_ends_rather_than_being_abandoned() {
    crate::value::arena::with_test_region(|| {
        // A pipe nobody writes: the read end never becomes readable.
        let pipe = Pipe::new();
        let backend = AsyncBackend::new_thread_pool().unwrap();
        let id = backend
            .submit(
                &IoRequest {
                    op: IoOp::PollFd {
                        fd: pipe.read_fd,
                        events: libc::POLLIN as u32,
                    },
                    port: Value::NIL,
                    timeout: None,
                },
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        assert_cancel_retires(&backend, id, "poll-fd");
    });
}

/// A cancelled `ev/poll-fd` must leave the descriptor's flags alone.
///
/// The descriptor belongs to whoever passed it in — a display connection, a
/// GLib event source. The bound watches it and never reads it, so taking it
/// non-blocking would change a file description this runtime does not own.
#[test]
fn a_pool_poll_fd_does_not_touch_the_descriptor_it_watches() {
    crate::value::arena::with_test_region(|| {
        let pipe = Pipe::new();
        let before = unsafe { libc::fcntl(pipe.read_fd, libc::F_GETFL) };
        assert!(before >= 0, "F_GETFL failed");
        assert_eq!(
            before & libc::O_NONBLOCK,
            0,
            "a fresh pipe end is blocking, which is what makes the check mean \
             something"
        );

        let backend = AsyncBackend::new_thread_pool().unwrap();
        let id = backend
            .submit(
                &IoRequest {
                    op: IoOp::PollFd {
                        fd: pipe.read_fd,
                        events: libc::POLLIN as u32,
                    },
                    port: Value::NIL,
                    timeout: None,
                },
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        // While the worker is out, not after it has cleaned up.
        for _ in 0..200 {
            if backend.workers() > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(50));
        let during = unsafe { libc::fcntl(pipe.read_fd, libc::F_GETFL) };
        assert_eq!(
            during, before,
            "the poll changed the flags on a descriptor it only watches"
        );

        assert_cancel_retires(&backend, id, "poll-fd");
    });
}

/// A cancelled filesystem watch must END on the thread-pool backend.
///
/// A watcher on a directory nothing touches waits forever, and `fs/watch` names
/// no deadline, so the stop pipe is the whole bound.
#[test]
fn a_cancelled_pool_watch_next_ends_rather_than_being_abandoned() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let dir = temp_path("watch-park");
        std::fs::create_dir(&dir).unwrap();

        let watcher = crate::io::watch::FsWatcher::new().unwrap();
        watcher.add(&dir, false).unwrap();
        let watcher_val = h.ctx().external("fs-watcher", watcher);

        let backend = AsyncBackend::new_thread_pool().unwrap();
        let id = backend
            .submit(
                &IoRequest {
                    op: IoOp::WatchNext,
                    port: watcher_val,
                    timeout: None,
                },
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        assert_cancel_retires(&backend, id, "watch-next");
        std::fs::remove_dir(&dir).ok();
    });
}

/// A cancelled open must END on the thread-pool backend.
///
/// `open(2)` on a fifo for writing waits until a reader opens the other end,
/// which it need never do. The worker asks with `O_NONBLOCK` and paces its
/// retry under the bound instead, so the stop is visible between asks.
#[test]
fn a_cancelled_pool_open_of_a_fifo_ends_rather_than_being_abandoned() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let fifo = Fifo::new("open-park");

        let backend = AsyncBackend::new_thread_pool().unwrap();
        let port = h.ctx().external(
            "port",
            Port::new_unopened(
                PortKind::File,
                Direction::Write,
                Encoding::Binary,
                fifo.0.clone(),
            ),
        );
        let id = backend
            .submit(
                &IoRequest {
                    op: IoOp::Open {
                        path: fifo.0.clone(),
                        flags: libc::O_WRONLY | libc::O_CLOEXEC,
                        mode: 0o666,
                        direction: Direction::Write,
                        encoding: Encoding::Binary,
                    },
                    port,
                    timeout: None,
                },
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        assert_cancel_retires(&backend, id, "open");
    });
}

/// A cancelled operation delivers no completion — on either backend.
///
/// Every `io/cancel` caller in the scheduler drops its record of the submission
/// before cancelling it (`complete-fiber`, `handle-abort`,
/// `handle-io-forward-cancel`, `do-shutdown`), so no fiber is left to receive a
/// result. Building one anyway is not merely wasted work: the completion path
/// dereferences the operation's port `Value`, and the release that ran when the
/// asking fiber finished may already have freed the region that value lives in.
///
/// The trap: the two backends reach the same cancellation by different routes —
/// the pool worker returns `-ECANCELED` through the hub, the ring posts a
/// `-ECANCELED` CQE — so a discard implemented on one of them says nothing about
/// the other. The counter-factual is the ring cooking the CQE: one completion
/// arrives for an id nobody is waiting on, carrying a value read through the
/// dead port.
#[test]
fn a_cancelled_operation_delivers_no_completion_on_either_backend() {
    use std::os::unix::io::FromRawFd;
    crate::value::arena::with_test_region(|| {
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            // A pipe whose write end stays open and empty: the read parks, and
            // the cancel is the only thing that can end it. The port takes a
            // duplicate so its close and the pipe's are each of one descriptor.
            let pipe = Pipe::new();
            let dup_fd = unsafe { libc::dup(pipe.read_fd) };
            assert!(dup_fd >= 0, "{which}: dup(2) failed");
            let h = crate::primitives::ctx::TestHeap::new();
            let port = h.ctx().external(
                "port",
                Port::new_file(
                    unsafe { std::os::unix::io::OwnedFd::from_raw_fd(dup_fd) },
                    Direction::Read,
                    Encoding::Binary,
                    "<pipe>".into(),
                ),
            );
            let id = backend
                .submit(
                    &IoRequest {
                        op: PortOp::ReadAll.into(),
                        port,
                        timeout: None,
                    },
                    crate::value::arena::leaked_test_heap(),
                )
                .unwrap();

            // The pool cancels a worker that is already waiting; give it the
            // thread before asking. The ring has no worker to wait for.
            for _ in 0..200 {
                if backend.workers() > 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            backend.cancel(id).unwrap();

            // Bounded, because the property is that this terminates: the entry
            // must be retired by the cancelled operation's own completion.
            let mut delivered = Vec::new();
            for _ in 0..40 {
                delivered.extend(backend.wait(50).unwrap().into_iter().map(|c| c.id));
                if !backend.has_pending() && backend.workers() == 0 {
                    break;
                }
            }

            assert!(
                delivered.is_empty(),
                "{which}: a cancelled operation delivered {} completion(s) — \
                 no fiber is waiting for one, and building it reads the port \
                 value the finished fiber's release may already have freed",
                delivered.len(),
            );
            assert!(
                !backend.has_pending(),
                "{which}: the cancelled operation kept its pending entry",
            );
            assert_eq!(
                backend.workers(),
                0,
                "{which}: the cancelled operation never gave its worker back",
            );
        }
    });
}

/// `ev/poll-fd` answers an expired wait with 0 — on either backend.
///
/// That is the primitive's documented contract, and what lets a caller poll in
/// a loop: `wayland/event-loop` polls with a 33 ms bound on every iteration and
/// ignores the result, and `glib-wait` compares the mask against zero. An error
/// instead would abort both loops on their first quiet tick.
///
/// The two backends report the expiry differently — `ETIMEDOUT` from the pool
/// worker's own bound, `ECANCELED` from the ring's linked timeout — so this
/// runs against both to pin that neither difference reaches the caller.
#[test]
fn a_poll_fd_that_reaches_its_deadline_reports_no_events_on_either_backend() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            // A pipe nobody writes: the read end never becomes readable, so the
            // deadline is the only thing that can end the wait.
            let pipe = Pipe::new();
            let id = backend
                .submit(
                    &IoRequest {
                        op: IoOp::PollFd {
                            fd: pipe.read_fd,
                            events: libc::POLLIN as u32,
                        },
                        port: Value::NIL,
                        timeout: Some(Duration::from_millis(200)),
                    },
                    crate::value::arena::leaked_test_heap(),
                )
                .unwrap();

            let mut completions = backend.wait(-1).unwrap();
            assert_eq!(completions.len(), 1, "{which}: one completion");
            let completion = completions.pop().unwrap();
            assert_eq!(completion.id, id, "{which}: the submitted id came back");
            let value = completion
                .result
                .unwrap_or_else(|e| panic!("{which}: a quiet poll-fd must not signal, got {e:?}"));
            assert_eq!(
                value.as_int(),
                Some(0),
                "{which}: an expired poll-fd reports no events"
            );
        }
    });
}
