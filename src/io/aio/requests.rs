//! The operations the backend issues, and the frame they share.
//!
//! `submit_op` is that frame: mint the id, reserve the pinned buffer, hand the
//! operation to the platform, file the pending entry the completion resolves
//! through. Each `submit_*` supplies only what is its own — how many bytes the
//! kernel may write, which platform call runs the operation, what its
//! completion must remember.

use super::*;

use crate::io::pool::BufferHandle;
use std::os::unix::io::RawFd;

/// What one submission's platform dispatch may reach: the id the operation is
/// issued under, the pooled buffer reserved for it, and the two places an
/// operation can be handed to — the io_uring ring and the thread-pool hub.
///
/// The `match platform` stays at the call sites rather than moving in here.
/// Every operation calls a different `submit_uring_*` and builds a different
/// [`PoolOp`], and `io_uring::IoUring` is a type that does not exist off Linux,
/// so a helper taking both arms would need a `#[cfg]`'d signature for no gain.
/// [`Dispatch::poll_fd`] is the exception: the two operations that wait on a
/// bare descriptor wait the same way.
struct Dispatch<'a> {
    /// The id the operation is issued under, and the key its pending entry
    /// will be filed by.
    id: SubmissionId,
    /// The pooled buffer reserved for this operation. Only the io_uring arms
    /// use it — a pool worker copies its result back through the hub rather
    /// than writing into pinned memory — so off Linux nothing reads it.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    buffer: BufferHandle,
    platform: &'a mut PlatformBackend,
    hub: &'a mut CompletionHub,
    /// The pool `buffer` indexes into, for the io_uring arms that must fill it
    /// (a path, a sockaddr) before handing the pointer to the kernel.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    buffer_pool: &'a mut BufferPool,
}

impl Dispatch<'_> {
    /// Arm a readiness wait on `fd` for `events`: a `POLL_ADD` on the ring, or
    /// a worker blocking in `poll(2)` on the pool. `ev/poll-fd` and the
    /// `chan/wait-ready` park differ only in what their pending entry keeps.
    fn poll_fd(&mut self, fd: RawFd, events: u32, timeout: Option<Duration>) -> Result<(), String> {
        match &mut *self.platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                crate::io::uring::submit_uring_poll_add(ring, self.id, fd, events, timeout)
            }
            PlatformBackend::ThreadPool => {
                // The wait is open-ended when no timeout was named, and a park
                // `ev/timeout` cannot reach outlives the fiber that wanted it.
                let bounds = self.hub.bounds(self.id, timeout);
                self.hub
                    .submit(self.id, PoolOp::PollFd { fd, events }, bounds)
            }
        }
    }
}

impl AsyncBackend {
    /// Issue one operation: mint its id, reserve `buf_bytes` of pinned buffer,
    /// hand it to the platform, and remember what its completion will need.
    ///
    /// `dispatch` returns whatever the platform decided that the pending entry
    /// must record — the pre-created socket fd for a connect, the `siginfo_t`
    /// allocation for a process wait, `()` for the operations that decide
    /// nothing. `make_pending` turns that, plus the buffer, into the entry.
    ///
    /// The entry is filed under the same id the operation was dispatched with,
    /// which is what lets an arriving completion find it. A completion whose
    /// entry is missing is discarded and its fiber never wakes.
    ///
    /// A dispatch failure returns before any entry exists, and leaves the
    /// buffer reserved: `submit_linked` can fail with the operation's SQE
    /// already pushed onto the submission queue, so the kernel may still read
    /// that buffer on the next `ring.submit()`.
    fn submit_op<D>(
        &self,
        buf_bytes: usize,
        dispatch: impl FnOnce(&mut Dispatch) -> Result<D, String>,
        make_pending: impl FnOnce(BufferHandle, D) -> PendingOp,
    ) -> Result<SubmissionId, String> {
        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();
        let buffer = inner.buffer_pool.alloc(buf_bytes);

        let submitter = inner.submitter;
        let AsyncBackendInner {
            ref mut platform,
            ref mut hub,
            ref mut pending,
            ref mut buffer_pool,
            ..
        } = *inner;

        let decided = dispatch(&mut Dispatch {
            id,
            buffer,
            platform,
            hub,
            buffer_pool,
        })?;
        pending.insert(id, make_pending(buffer, decided), submitter);
        Ok(id)
    }

    /// Submit a Connect operation. Connect creates the port its completion
    /// fills, so `port` arrives pre-allocated rather than in `request.port`.
    ///
    /// The dispatch decides the connect fd: io_uring pre-creates the socket and
    /// reports it here, while the pool worker creates one and reports it on
    /// completion.
    #[allow(unused_variables)]
    pub(super) fn submit_connect(
        &self,
        addr: &ConnectAddr,
        timeout: Option<Duration>,
        port: Value,
    ) -> Result<SubmissionId, String> {
        self.submit_op(
            0,
            |d| match &mut *d.platform {
                #[cfg(target_os = "linux")]
                PlatformBackend::Uring(ring) => {
                    // Connect always carries a parsed IP, so io_uring hands it to the
                    // kernel directly — there is no hostname branch to fall back from.
                    // A submission failure (queue full, socket()) is a hard error, not
                    // a silent demotion to the thread pool.
                    crate::io::uring::submit_uring_connect(
                        ring,
                        d.id,
                        addr,
                        timeout,
                        d.buffer_pool,
                        d.buffer,
                    )
                    .map(Some)
                }
                PlatformBackend::ThreadPool => {
                    // A connect waits on a peer that need never answer — a
                    // dropped handshake, a listener whose backlog is full — so
                    // it carries the caller's deadline and a stop pipe, like
                    // every other open-ended pool operation.
                    let bounds = d.hub.bounds(d.id, timeout);
                    let pool_op = match addr {
                        ConnectAddr::Tcp {
                            addr: ip,
                            port,
                            options,
                            ..
                        } => PoolOp::ConnectTcp {
                            addr: std::net::SocketAddr::new(*ip, *port),
                            options: options.clone(),
                        },
                        ConnectAddr::Unix { path, options, .. } => PoolOp::ConnectUnix {
                            path: path.clone(),
                            options: options.clone(),
                        },
                    };
                    d.hub.submit(d.id, pool_op, bounds)?;
                    Ok(None)
                }
            },
            |buffer, connect_fd| PendingOp::Connect {
                addr: addr.clone(),
                buffer_handle: buffer,
                connect_fd,
                port,
            },
        )
    }

    /// Submit a Sleep operation. No port — just a timer.
    pub(super) fn submit_sleep(&self, duration: Duration) -> Result<SubmissionId, String> {
        self.submit_op(
            0,
            |d| match &mut *d.platform {
                #[cfg(target_os = "linux")]
                PlatformBackend::Uring(ring) => {
                    crate::io::uring::submit_uring_sleep(ring, d.id, duration)
                }
                PlatformBackend::ThreadPool => {
                    // The duration is the bound: a timer has nothing else to
                    // wait for. `ev/timeout` cancels its timer on every call
                    // the body wins, and a timer that ran on to its full
                    // duration would hold a worker for that long.
                    let bounds = d.hub.bounds(d.id, Some(duration));
                    d.hub.submit(d.id, PoolOp::Sleep, bounds)
                }
            },
            |buffer, ()| PendingOp::Sleep {
                buffer_handle: buffer,
            },
        )
    }

    /// Submit a ChanSelectPark operation — wait for a `chan/wait-ready` wake fd
    /// to become readable, or for the timeout to elapse.
    ///
    /// The wait is `submit_poll_fd`'s; what differs is that the pending entry
    /// retains the guard, so its Drop closes the fd and deregisters from every
    /// `WakeList` exactly once.
    pub(super) fn submit_chan_select_park(
        &self,
        guard: crate::primitives::chan::ChanSelectGuard,
        timeout: Option<Duration>,
    ) -> Result<SubmissionId, String> {
        let fd = guard.poll_fd();
        self.submit_op(
            0,
            |d| d.poll_fd(fd, libc::POLLIN as u32, timeout),
            |buffer, ()| PendingOp::ChanSelectPark {
                buffer_handle: buffer,
                guard,
            },
        )
    }

    /// Submit a PollFd operation — wait for a raw fd to become ready.
    pub(super) fn submit_poll_fd(
        &self,
        fd: RawFd,
        events: u32,
        timeout: Option<Duration>,
    ) -> Result<SubmissionId, String> {
        self.submit_op(
            0,
            |d| d.poll_fd(fd, events, timeout),
            |buffer, ()| PendingOp::PollFd {
                buffer_handle: buffer,
            },
        )
    }

    /// Submit a DNS resolution. getaddrinfo(3) has no io_uring form, so this
    /// always goes to the thread pool.
    ///
    /// It is also the one operation nothing can bound. `getaddrinfo(3)` runs to
    /// the resolver's own end — through every retry `resolv.conf` asks for —
    /// and offers no descriptor to wait on and no way to be interrupted. A
    /// cancel therefore discards the answer without giving the worker thread
    /// back any sooner.
    pub(super) fn submit_resolve(&self, hostname: &str) -> Result<SubmissionId, String> {
        self.submit_op(
            0,
            |d| {
                d.hub.submit(
                    d.id,
                    PoolOp::Resolve {
                        hostname: hostname.to_string(),
                    },
                    Bounds::uninterruptible(),
                )
            },
            |buffer, ()| PendingOp::Resolve {
                buffer_handle: buffer,
            },
        )
    }

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
                        // Dedicated io_uring + signalfd path: a single
                        // IORING_OP_READ on the signalfd, completing via the
                        // kernel's poll pipeline with no elle-side worker
                        // thread. Threadpool is reached only when --no-uring
                        // is in effect (see PlatformBackend::ThreadPool arm).
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
    /// One implementation also means one answer — the fifo behaviour below is
    /// the same whichever platform is underneath.
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
        let result = req.spawn_to_struct(origin_heap);

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
    /// The dispatch decides the `siginfo_t` the completion reads: io_uring's
    /// `WAITID` needs one for the kernel to fill, while the pool worker calls
    /// `waitid(2)` itself and reports the code, so its entry holds null.
    pub(super) fn submit_process_wait(&self, handle_val: &Value) -> Result<SubmissionId, String> {
        let handle = handle_val
            .as_external::<ProcessHandle>()
            .ok_or_else(|| "io/submit: ProcessWait requires a process handle".to_string())?;

        // Fast path: already exited (cached). Push immediate completion, no pending entry.
        {
            let state = handle.inner.borrow();
            if let ProcessState::Exited(code) = &*state {
                let mut inner = self.inner.borrow_mut();
                let id = inner.mint_id();
                inner
                    .completions
                    .push_back(Completion::ok(id, Value::int(*code as i64)));
                return Ok(id);
            }
        }

        let pid = handle.pid();
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
                    d.hub.submit(d.id, PoolOp::ProcessWait { pid }, bounds)?;
                    Ok(std::ptr::null_mut())
                }
            },
            |buffer, siginfo| PendingOp::ProcessWait {
                buffer_handle: buffer,
                handle_val: *handle_val,
                siginfo,
            },
        )
    }
}
