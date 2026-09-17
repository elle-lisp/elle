//! audited: 2026-09-17
//! The submissions that name an OS object the request carries or creates: a
//! watcher, a signal receiver, a file, a child, a background task.
//!
//! src/io/AGENTS.md

use super::*;

impl AsyncBackend {
    /// Submit a watch-next operation. Reads from the inotify fd.
    pub(super) fn submit_watch_next(&self, watcher_val: &Value) -> Result<SubmissionId, String> {
        use crate::io::watch::FsWatcher;

        let watcher = watcher_val
            .as_external::<FsWatcher>()
            .ok_or("watch-next: expected a watcher handle")?;
        let fd = watcher.raw_fd()?;

        self.submit_op(
            4096,
            |d| match &mut *d.platform {
                #[cfg(target_os = "linux")]
                PlatformBackend::Uring(ring) => crate::io::uring::submit_uring_watch_next(
                    ring,
                    d.id,
                    fd,
                    d.buffer_pool,
                    d.buffer,
                ),
                PlatformBackend::ThreadPool => {
                    // A watcher on a directory nothing touches waits forever,
                    // so the read carries a stop pipe. `fs/watch` names no
                    // deadline, so the stop is the whole bound.
                    let bounds = d.hub.bounds(d.id, None);
                    d.hub.submit(d.id, PoolOp::WatchRead { fd }, bounds)
                }
            },
            |buffer, ()| PendingOp::WatchNext {
                watcher: *watcher_val,
                buffer_handle: buffer,
            },
        )
    }

    /// Submit a sig-next operation. Reads from the signalfd / kqueue fd.
    /// The buffer holds several batched `signalfd_siginfo` structs (Linux) or
    /// several kevent result pairs (macOS).
    pub(super) fn submit_sig_next(&self, receiver_val: &Value) -> Result<SubmissionId, String> {
        use crate::io::sigfd::{posix_trace, SignalReceiver};

        let receiver = receiver_val
            .as_external::<SignalReceiver>()
            .ok_or("sig-next: expected a signal receiver handle")?;
        let fd = receiver.raw_fd()?;
        // The receiver's own instance trace cell — used for the diagnostic below
        // and carried into the threadpool `PoolOp` so the worker's blocking-read
        // `posix_trace` gates per-instance.
        let trace = receiver.trace();
        posix_trace(&trace, format_args!("submit_sig_next fd={}", fd));

        // signalfd_siginfo is 128 bytes on Linux; round up generously.
        self.submit_op(
            1024,
            |d| {
                match &mut *d.platform {
                    #[cfg(target_os = "linux")]
                    PlatformBackend::Uring(ring) => {
                        // One `IORING_OP_READ` on the signalfd, completing
                        // through the kernel's poll pipeline with no worker
                        // thread of ours. The arm below is reached only under
                        // `--no-uring`.
                        crate::io::uring::submit_uring_sig_next(
                            ring,
                            d.id,
                            fd,
                            d.buffer_pool,
                            d.buffer,
                        )?;
                    }
                    PlatformBackend::ThreadPool => {
                        // A signal that never arrives waits forever, so the
                        // read carries a stop pipe. `os/sig-watch` names no
                        // deadline, so the stop is the whole bound. Each arm
                        // opens its own, because a platform with no signal read
                        // at all must not open a pipe it will never hand over.
                        #[cfg(any(target_os = "linux", target_os = "android"))]
                        {
                            let bounds = d.hub.bounds(d.id, None);
                            d.hub
                                .submit(d.id, PoolOp::SigfdRead { fd, trace }, bounds)?;
                        }
                        #[cfg(target_os = "macos")]
                        {
                            let bounds = d.hub.bounds(d.id, None);
                            d.hub.submit(
                                d.id,
                                PoolOp::KqSigRead {
                                    fd,
                                    // The worker pthread_sigmask-unblocks these
                                    // so kqueue's EVFILT_SIGNAL has a thread the
                                    // kernel can pick as the delivery target —
                                    // see `event::kq_sig_read`.
                                    signals: receiver.signals(),
                                    trace,
                                },
                                bounds,
                            )?;
                        }
                        #[cfg(not(any(
                            target_os = "linux",
                            target_os = "android",
                            target_os = "macos"
                        )))]
                        {
                            let _ = (d.id, fd);
                            return Err("sig-next: not supported on this platform".into());
                        }
                    }
                }
                Ok(())
            },
            |buffer, ()| PendingOp::SigNext {
                receiver: *receiver_val,
                buffer_handle: buffer,
            },
        )
    }

    /// Submit a file open operation. Open creates the port its completion
    /// fills, so `port` arrives pre-allocated rather than in `request.port`.
    ///
    /// This is a thread-pool operation on every platform, io_uring included.
    /// An `open(2)` on a fifo waits for the other end, and a wait is only
    /// answerable where the worker can hold it: `IORING_OP_OPENAT` blocks an
    /// io-wq thread that a linked timeout marks cancelled but cannot retract,
    /// so the kernel keeps the thread and the caller's `:timeout` buys nothing.
    /// One implementation is also one answer: the fifo behavior docs/io.md
    /// describes is the same whichever platform is underneath.
    pub(super) fn submit_open(
        &self,
        path: &str,
        flags: i32,
        mode: u32,
        timeout: Option<Duration>,
        port: Value,
    ) -> Result<SubmissionId, String> {
        let c_path = std::ffi::CString::new(path)
            .map_err(|_| format!("port/open: path contains null byte: {}", path))?;

        self.submit_op(
            0,
            |d| {
                let bounds = d.hub.bounds(d.id, timeout);
                d.hub.submit(
                    d.id,
                    PoolOp::Open {
                        path: c_path,
                        flags,
                        mode,
                    },
                    bounds,
                )
            },
            |buffer, ()| PendingOp::Open {
                path: path.to_string(),
                buffer_handle: buffer,
                port,
            },
        )
    }

    /// Start a subprocess. The child is spawned in this call, so the result is
    /// ready before it returns: no CQE will arrive, and there is no pending
    /// entry and no buffer for a kernel write to land in.
    pub(super) fn submit_spawn(
        &self,
        req: &SpawnRequest,
        origin_heap: *mut crate::value::fiberheap::FiberHeap,
    ) -> Result<SubmissionId, String> {
        // Build the spawn result on the requesting fiber's heap (`origin_heap`),
        // so there are no cross-heap references: the requesting fiber receives
        // this value via fiber/resume and the heap that built it is the heap that
        // manages its lifetime. Built before the backend is borrowed, so the
        // spawn cannot re-enter it.
        let result = req.spawn_to_subprocess(origin_heap);

        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();
        inner.completions.push_back(Completion::new(id, result));
        Ok(id)
    }

    /// Run an arbitrary closure on a background thread. A closure has no
    /// io_uring equivalent, so a Task always goes to the thread pool — on
    /// every platform.
    ///
    /// Nothing here can bound the closure: it is opaque Rust that runs until it
    /// returns. A cancel discards its result without giving the worker thread
    /// back, so a `Task` that must be interruptible has to arrange that itself.
    pub(super) fn submit_task(&self, task_fn: &TaskFn) -> Result<SubmissionId, String> {
        let closure = task_fn
            .take()
            .ok_or_else(|| "io/submit: task closure already consumed".to_string())?;

        self.submit_op(
            0,
            |d| {
                d.hub
                    .submit(d.id, PoolOp::Task(closure), Bounds::uninterruptible())
            },
            |buffer, ()| PendingOp::Task {
                buffer_handle: buffer,
            },
        )
    }

    /// Wait for a subprocess to exit.
    ///
    /// The dispatch decides the `siginfo_t` the completion reads. io_uring's
    /// `IORING_OP_WAITID` needs one for the kernel to fill, while the pool
    /// worker reaps with `waitpid(2)` and reports the code, so its entry holds
    /// null.
    pub(super) fn submit_process_wait(&self, handle_val: &Value) -> Result<SubmissionId, String> {
        let handle = handle_val
            .as_external::<ProcessHandle>()
            .ok_or_else(|| "io/submit: ProcessWait requires a subprocess".to_string())?;

        // Fast path: this process is already holding the child's status, so
        // there is nothing left to reap. Push an immediate completion and file
        // no pending entry. See src/io/AGENTS.md § "A reap is never wasted" —
        // the status is here whether the wait that took it was read or
        // cancelled.
        if let Some(code) = handle.exit().status() {
            let mut inner = self.inner.borrow_mut();
            let id = inner.mint_id();
            inner
                .completions
                .push_back(Completion::ok(id, Value::int(code as i64)));
            return Ok(id);
        }

        let pid = handle.pid();
        let exit = handle.exit().clone();
        // The dispatch takes its own clone rather than borrowing `exit`, which
        // the pending entry below moves.
        let pool_exit = exit.clone();
        self.submit_op(
            0,
            |d| match &mut *d.platform {
                #[cfg(target_os = "linux")]
                PlatformBackend::Uring(ring) => {
                    // The kernel fills this on child exit, so it must outlive the
                    // SQE — `PendingOp::ProcessWait` owns it until the CQE arrives,
                    // and completion processing reclaims it.
                    // SAFETY: zeroed() is valid for siginfo_t (all-zero is a valid
                    // initialized state).
                    let siginfo: *mut libc::siginfo_t =
                        Box::into_raw(unsafe { Box::new(std::mem::zeroed()) });
                    match crate::io::uring::submit_uring_process_wait(ring, d.id, pid, siginfo) {
                        Ok(()) => Ok(siginfo),
                        Err(e) => {
                            // SAFETY: we own siginfo, allocated just above, and no
                            // pending entry will be filed to hand it on.
                            unsafe { drop(Box::from_raw(siginfo)) };
                            Err(e)
                        }
                    }
                }
                PlatformBackend::ThreadPool => {
                    // A child that never exits waits forever, so the wait
                    // carries a stop pipe. `subprocess/wait` names no deadline,
                    // so the stop is the whole bound.
                    let bounds = d.hub.bounds(d.id, None);
                    d.hub.submit(
                        d.id,
                        PoolOp::ProcessWait {
                            pid,
                            exit: pool_exit,
                        },
                        bounds,
                    )?;
                    Ok(std::ptr::null_mut())
                }
            },
            |buffer, siginfo| PendingOp::ProcessWait {
                buffer_handle: buffer,
                handle_val: *handle_val,
                siginfo,
                exit,
            },
        )
    }
}
