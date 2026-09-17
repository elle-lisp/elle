//! audited: 2026-09-17
//! The frame every submission shares, and the operations that name no OS object
//! of their own.
//!
//! `submit_op` is that frame: mint the id, reserve the pinned buffer, hand the
//! operation to the platform, file the pending entry the completion resolves
//! through. Each `submit_*` supplies only what is its own — how many bytes the
//! kernel may write, which platform call runs the operation, what its
//! completion must remember. The submissions that name an object — a watcher, a
//! receiver, a file, a child — are in src/io/aio/externals.rs.
//!
//! src/io/AGENTS.md

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
pub(super) struct Dispatch<'a> {
    /// The id the operation is issued under, and the key its pending entry
    /// will be filed by.
    pub(super) id: SubmissionId,
    /// The pooled buffer reserved for this operation. Only the io_uring arms
    /// use it — a pool worker copies its result back through the hub rather
    /// than writing into pinned memory — so off Linux nothing reads it.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(super) buffer: BufferHandle,
    pub(super) platform: &'a mut PlatformBackend,
    pub(super) hub: &'a mut CompletionHub,
    /// The pool `buffer` indexes into, for the io_uring arms that must fill it
    /// (a path, a sockaddr) before handing the pointer to the kernel.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(super) buffer_pool: &'a mut BufferPool,
}

impl Dispatch<'_> {
    /// Arm a readiness wait on `fd` for `events`: a `POLL_ADD` on the ring, or
    /// a worker blocking in `poll(2)` on the pool. `ev/poll-fd` and the
    /// `chan/wait-ready` park differ only in what their pending entry keeps.
    pub(super) fn poll_fd(
        &mut self,
        fd: RawFd,
        events: u32,
        timeout: Option<Duration>,
    ) -> Result<(), String> {
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
    pub(super) fn submit_op<D>(
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
}
