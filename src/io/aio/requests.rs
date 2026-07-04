use super::*;

impl AsyncBackend {
    /// Submit a Connect operation. Connect creates a new port, so
    /// request.port is Value::NIL — we handle it separately.
    #[allow(unused_variables)]
    pub(super) fn submit_connect(
        &self,
        addr: &ConnectAddr,
        timeout: Option<Duration>,
        port: Value,
    ) -> Result<SubmissionId, String> {
        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();
        let buf_handle = inner.buffer_pool.alloc(0);

        let AsyncBackendInner {
            ref mut platform,
            ref mut hub,
            ref mut pending,
            ref mut buffer_pool,
            ..
        } = *inner;

        let uring_fd = match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                // Connect always carries a parsed IP, so io_uring hands it to the
                // kernel directly — there is no hostname branch to fall back from.
                // A submission failure (queue full, socket()) is a hard error, not
                // a silent demotion to the thread pool.
                Some(crate::io::uring::submit_uring_connect(
                    ring,
                    id,
                    addr,
                    timeout,
                    buffer_pool,
                    buf_handle,
                )?)
            }
            PlatformBackend::ThreadPool => {
                let _ = buffer_pool;
                let pool_op = match addr {
                    ConnectAddr::Tcp {
                        addr: ip,
                        port,
                        ref options,
                        ..
                    } => PoolOp::ConnectTcp {
                        // `TcpStream::connect` on a numeric address resolves to
                        // itself without touching DNS; the bracketing keeps IPv6
                        // parseable.
                        addr: crate::io::sockaddr::format_host_port(&ip.to_string(), *port),
                        options: options.clone(),
                    },
                    ConnectAddr::Unix {
                        path, ref options, ..
                    } => PoolOp::ConnectUnix {
                        path: path.clone(),
                        options: options.clone(),
                    },
                };
                hub.submit(id, pool_op)?;
                None
            }
        };

        pending.insert(
            id,
            PendingOp::Connect {
                addr: match addr {
                    ConnectAddr::Tcp {
                        addr: ip,
                        port,
                        ref options,
                        encoding,
                    } => ConnectAddr::Tcp {
                        addr: *ip,
                        port: *port,
                        options: options.clone(),
                        encoding: *encoding,
                    },
                    ConnectAddr::Unix {
                        path,
                        ref options,
                        encoding,
                    } => ConnectAddr::Unix {
                        path: path.clone(),
                        options: options.clone(),
                        encoding: *encoding,
                    },
                },
                buffer_handle: buf_handle,
                connect_fd: uring_fd,
                port,
            },
        );
        Ok(id)
    }

    /// Submit a Sleep operation. No port — just a timer.
    pub(super) fn submit_sleep(&self, duration: Duration) -> Result<SubmissionId, String> {
        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();
        let buf_handle = inner.buffer_pool.alloc(0);

        let AsyncBackendInner {
            ref mut platform,
            ref mut hub,
            ref mut pending,
            ..
        } = *inner;

        match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                crate::io::uring::submit_uring_sleep(ring, id, duration)?;
            }
            PlatformBackend::ThreadPool => {
                let nanos = duration.as_nanos() as u64;
                hub.submit(id, PoolOp::Sleep { nanos })?;
            }
        }

        pending.insert(
            id,
            PendingOp::Sleep {
                buffer_handle: buf_handle,
            },
        );
        Ok(id)
    }

    /// Submit a ChanSelectPark operation — wait for a `chan/wait-ready`
    /// wake fd to become readable, or for the timeout to elapse.
    /// Internally the same shape as `submit_poll_fd` (POLL_ADD on uring,
    /// `poll(2)` on the thread pool) but the `PendingOp::ChanSelectPark`
    /// retains the guard so its Drop closes the fd and deregisters from
    /// every `WakeList` exactly once.
    pub(super) fn submit_chan_select_park(
        &self,
        guard: crate::primitives::chan::ChanSelectGuard,
        timeout: Option<Duration>,
    ) -> Result<SubmissionId, String> {
        let fd = guard.poll_fd();
        let events = libc::POLLIN as u32;

        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();
        let buf_handle = inner.buffer_pool.alloc(0);

        let AsyncBackendInner {
            ref mut platform,
            ref mut hub,
            ref mut pending,
            ..
        } = *inner;

        match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                crate::io::uring::submit_uring_poll_add(ring, id, fd, events, timeout)?;
            }
            PlatformBackend::ThreadPool => {
                let timeout_ms = timeout.map(|d| d.as_millis() as i32).unwrap_or(-1);
                hub.submit(
                    id,
                    PoolOp::PollFd {
                        fd,
                        events,
                        timeout_ms,
                    },
                )?;
            }
        }

        pending.insert(
            id,
            PendingOp::ChanSelectPark {
                buffer_handle: buf_handle,
                guard,
            },
        );
        Ok(id)
    }

    /// Submit a PollFd operation — wait for a raw fd to become ready.
    pub(super) fn submit_poll_fd(
        &self,
        fd: std::os::unix::io::RawFd,
        events: u32,
        timeout: Option<Duration>,
    ) -> Result<SubmissionId, String> {
        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();
        let buf_handle = inner.buffer_pool.alloc(0);

        let AsyncBackendInner {
            ref mut platform,
            ref mut hub,
            ref mut pending,
            ..
        } = *inner;

        match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                crate::io::uring::submit_uring_poll_add(ring, id, fd, events, timeout)?;
            }
            PlatformBackend::ThreadPool => {
                let timeout_ms = timeout.map(|d| d.as_millis() as i32).unwrap_or(-1);
                hub.submit(
                    id,
                    PoolOp::PollFd {
                        fd,
                        events,
                        timeout_ms,
                    },
                )?;
            }
        }

        pending.insert(
            id,
            PendingOp::PollFd {
                buffer_handle: buf_handle,
            },
        );
        Ok(id)
    }

    /// Submit a DNS resolution. Always dispatched to the thread pool.
    pub(super) fn submit_resolve(&self, hostname: &str) -> Result<SubmissionId, String> {
        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();
        let buf_handle = inner.buffer_pool.alloc(0);
        inner.hub.submit(
            id,
            PoolOp::Resolve {
                hostname: hostname.to_string(),
            },
        )?;
        inner.pending.insert(
            id,
            PendingOp::Resolve {
                buffer_handle: buf_handle,
            },
        );
        Ok(id)
    }

    /// Submit a watch-next operation. Reads from the inotify fd.
    #[allow(unused_variables)]
    pub(super) fn submit_watch_next(&self, watcher_val: &Value) -> Result<SubmissionId, String> {
        use crate::io::watch::FsWatcher;

        let watcher = watcher_val
            .as_external::<FsWatcher>()
            .ok_or("watch-next: expected a watcher handle")?;
        let fd = watcher.raw_fd()?;

        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();
        let buf_handle = inner.buffer_pool.alloc(4096);

        let AsyncBackendInner {
            ref mut platform,
            ref mut hub,
            ref mut pending,
            ref mut buffer_pool,
            ..
        } = *inner;

        match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                crate::io::uring::submit_uring_watch_next(ring, id, fd, buffer_pool, buf_handle)?;
            }
            PlatformBackend::ThreadPool => {
                hub.submit(id, PoolOp::WatchRead { fd })?;
            }
        }

        pending.insert(
            id,
            PendingOp::WatchNext {
                watcher: *watcher_val,
                buffer_handle: buf_handle,
            },
        );
        Ok(id)
    }

    /// Submit a sig-next operation. Reads from the signalfd / kqueue fd.
    /// Buffer is sized for several batched signalfd_siginfo structs (Linux)
    /// or several kevent result pairs (macOS).
    #[allow(unused_variables)]
    pub(super) fn submit_sig_next(&self, receiver_val: &Value) -> Result<SubmissionId, String> {
        use crate::io::sigfd::{posix_trace, SignalReceiver};

        let receiver = receiver_val
            .as_external::<SignalReceiver>()
            .ok_or("sig-next: expected a signal receiver handle")?;
        let fd = receiver.raw_fd()?;
        posix_trace(format_args!("submit_sig_next fd={}", fd));

        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();
        // signalfd_siginfo is 128 bytes on Linux; round up generously.
        let buf_handle = inner.buffer_pool.alloc(1024);

        let AsyncBackendInner {
            ref mut platform,
            ref mut hub,
            ref mut pending,
            ref mut buffer_pool,
            ..
        } = *inner;

        match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                // Dedicated io_uring + signalfd path: a single
                // IORING_OP_READ on the signalfd, completing via the
                // kernel's poll pipeline with no elle-side worker
                // thread. Threadpool is reached only when --no-uring
                // is in effect (see PlatformBackend::ThreadPool arm).
                crate::io::uring::submit_uring_sig_next(ring, id, fd, buffer_pool, buf_handle)?;
            }
            PlatformBackend::ThreadPool => {
                #[cfg(any(target_os = "linux", target_os = "android"))]
                hub.submit(id, PoolOp::SigfdRead { fd })?;
                #[cfg(target_os = "macos")]
                hub.submit(
                    id,
                    PoolOp::KqSigRead {
                        fd,
                        // Worker pthread_sigmask-unblocks these so kqueue's
                        // EVFILT_SIGNAL has a thread the kernel can pick
                        // as the delivery target — see kq_sig_read_blocking
                        // in src/io/threadpool.rs.
                        signals: receiver.signals(),
                    },
                )?;
                #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
                {
                    let _ = (id, fd);
                    return Err("sig-next: not supported on this platform".into());
                }
            }
        }

        pending.insert(
            id,
            PendingOp::SigNext {
                receiver: *receiver_val,
                buffer_handle: buf_handle,
            },
        );
        Ok(id)
    }

    /// Submit a file open operation. Open creates a new port, so
    /// request.port is Value::NIL — we handle it before the port guard.
    #[allow(unused_variables, clippy::too_many_arguments)]
    pub(super) fn submit_open(
        &self,
        path: &str,
        flags: i32,
        mode: u32,
        direction: Direction,
        encoding: Encoding,
        timeout: Option<Duration>,
        port: Value,
    ) -> Result<SubmissionId, String> {
        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();
        let buf_handle = inner.buffer_pool.alloc(0);

        let c_path = std::ffi::CString::new(path)
            .map_err(|_| format!("port/open: path contains null byte: {}", path))?;

        let AsyncBackendInner {
            ref mut platform,
            ref mut hub,
            ref mut pending,
            ref mut buffer_pool,
            ..
        } = *inner;

        match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                crate::io::uring::submit_uring_open(
                    ring,
                    id,
                    &c_path,
                    flags,
                    mode,
                    timeout,
                    buffer_pool,
                    buf_handle,
                )?;
            }
            PlatformBackend::ThreadPool => {
                let _ = buffer_pool;
                hub.submit(
                    id,
                    PoolOp::Open {
                        path: c_path,
                        flags,
                        mode,
                    },
                )?;
            }
        }

        pending.insert(
            id,
            PendingOp::Open {
                path: path.to_string(),
                buffer_handle: buf_handle,
                port,
            },
        );
        Ok(id)
    }

    pub(super) fn submit_spawn(
        &self,
        req: &SpawnRequest,
        origin_heap: *mut crate::value::fiberheap::FiberHeap,
    ) -> Result<SubmissionId, String> {
        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();
        let buf_handle = inner.buffer_pool.alloc(0);

        // Build the spawn result on the requesting fiber's heap (`origin_heap`),
        // so there are no cross-heap references: the requesting fiber receives
        // this value via fiber/resume and the heap that built it is the heap that
        // manages its lifetime.
        let result = req.spawn_to_struct(origin_heap);

        inner.completions.push_back(Completion::new(id, result));

        // Spawn is an immediate completion — no CQE will arrive.
        // Release the placeholder buffer (was alloc(0), nothing stored).
        inner.buffer_pool.release(buf_handle);
        Ok(id)
    }

    #[allow(unused_variables)]
    pub(super) fn submit_task(&self, task_fn: &TaskFn) -> Result<SubmissionId, String> {
        let closure = task_fn
            .take()
            .ok_or_else(|| "io/submit: task closure already consumed".to_string())?;
        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();
        let buf_handle = inner.buffer_pool.alloc(0);

        // An arbitrary closure has no io_uring equivalent, so a Task always runs
        // on the thread pool, feeding the hub — on every platform.
        inner.hub.submit(id, PoolOp::Task(closure))?;

        inner.pending.insert(
            id,
            PendingOp::Task {
                buffer_handle: buf_handle,
            },
        );
        Ok(id)
    }

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
        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();
        let buf_handle = inner.buffer_pool.alloc(0);

        // Allocate siginfo_t for the kernel to fill on child exit.
        // Must live until the CQE arrives — stored in PendingOp.
        // SAFETY: zeroed() is valid for siginfo_t (all-zero is a valid initialized state).
        let siginfo_ptr = {
            let si: Box<libc::siginfo_t> = unsafe { Box::new(std::mem::zeroed()) };
            Box::into_raw(si)
        };

        let AsyncBackendInner {
            ref mut platform,
            ref mut hub,
            ref mut pending,
            ..
        } = *inner;

        match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                if let Err(e) =
                    crate::io::uring::submit_uring_process_wait(ring, id, pid, siginfo_ptr)
                {
                    // SAFETY: we own siginfo_ptr, just allocated above; reclaim it on error.
                    unsafe { drop(Box::from_raw(siginfo_ptr)) };
                    return Err(e);
                }
            }
            PlatformBackend::ThreadPool => {
                // No siginfo needed for thread pool path — reclaim the allocation.
                unsafe { drop(Box::from_raw(siginfo_ptr)) };
                hub.submit(id, PoolOp::ProcessWait { pid })?;
            }
        }

        // For the thread pool path, siginfo_ptr was already freed above.
        // Store null so the completion handler knows to use the raw result integer.
        let stored_siginfo = match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(_) => siginfo_ptr,
            PlatformBackend::ThreadPool => std::ptr::null_mut(),
        };

        pending.insert(
            id,
            PendingOp::ProcessWait {
                buffer_handle: buf_handle,
                handle_val: *handle_val,
                siginfo: stored_siginfo,
            },
        );
        Ok(id)
    }
}
