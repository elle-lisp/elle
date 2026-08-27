use super::*;

impl AsyncBackend {
    /// Submit an I/O request. Returns a submission ID.
    ///
    /// `origin_heap` is the requesting fiber's heap; results and errors are
    /// allocated on it.
    pub(crate) fn submit(
        &self,
        request: &IoRequest,
        origin_heap: *mut crate::value::fiberheap::FiberHeap,
    ) -> Result<SubmissionId, String> {
        // Record the requesting instance's heap so the scheduler-thread completion
        // harvest builds every result/error value on it. Constant per backend (one
        // instance per scheduler).
        if !origin_heap.is_null() {
            self.inner.borrow_mut().origin_heap = origin_heap;
        }

        // Portless operations — handle before port extraction.
        if let IoOp::Connect { ref addr } = request.op {
            return self.submit_connect(addr, request.timeout, request.port);
        }
        if let IoOp::Sleep { duration } = request.op {
            return self.submit_sleep(duration);
        }

        // Subprocess ops: portless (Spawn) or ProcessHandle-in-port (ProcessWait).
        if let IoOp::Spawn(ref req) = request.op {
            return self.submit_spawn(req, origin_heap);
        }
        if let IoOp::ProcessWait = request.op {
            return self.submit_process_wait(&request.port);
        }

        // Resolve is portless — always goes to the thread pool.
        if let IoOp::Resolve { ref hostname } = request.op {
            return self.submit_resolve(hostname);
        }

        // WatchNext is portless — the FsWatcher External is in request.port.
        if let IoOp::WatchNext = request.op {
            return self.submit_watch_next(&request.port);
        }

        // SigNext is portless — the SignalReceiver External is in request.port.
        if let IoOp::SigNext = request.op {
            return self.submit_sig_next(&request.port);
        }

        // Open is portless — creates a new port rather than operating on one.
        // The op's `direction`/`encoding` describe the port the completion
        // fills; the port itself arrives pre-allocated in `request.port`, so
        // the submission needs neither.
        if let IoOp::Open {
            ref path,
            flags,
            mode,
            ..
        } = request.op
        {
            return self.submit_open(path, flags, mode, request.timeout, request.port);
        }

        // Task: run closure on thread pool.
        if let IoOp::Task(ref task_fn) = request.op {
            return self.submit_task(task_fn);
        }

        // PollFd: poll a raw fd for readiness.
        if let IoOp::PollFd { fd, events } = request.op {
            return self.submit_poll_fd(fd, events, request.timeout);
        }

        // ChanSelectPark: poll a chan/wait-ready eventfd until any
        // registered sender signals it or the timeout elapses.  The
        // guard owns the fd(s) and the wake-list registrations and is
        // transferred into PendingOp::ChanSelectPark so cleanup runs
        // exactly once on completion / cancellation.
        if let IoOp::ChanSelectPark(ref guard_cell) = request.op {
            let guard = guard_cell
                .take()
                .ok_or_else(|| "io/submit: ChanSelectPark guard already consumed".to_string())?;
            return self.submit_chan_select_park(guard, request.timeout);
        }

        let port = request
            .port
            .as_external::<Port>()
            .ok_or_else(|| "io/submit: request contains non-port value".to_string())?;

        // Close: cancel pending ops on this fd, then close the port.
        // Must come before the is_closed() check since the port is open
        // when close is requested.
        if matches!(&request.op, IoOp::Close) {
            let port_key = PortKey::from_port(port);
            // Stdin close has its own path: the dedicated stdin worker
            // thread reads via blocking poll(2)+read(2) on fd 0, not
            // through io_uring. Signal the thread to shut down — the
            // worker detects the self-pipe wakeup inside its next
            // `poll(2)`, sends a `stdin closed` error completion for
            // whatever read was in flight, drains any further
            // requests as cancelled, and exits.
            //
            // We do NOT take/drop `stdin_thread` here: dropping joins
            // the worker, which would block the scheduler **on this
            // very thread** before the worker's cancellation
            // completion can be drained by the main poll loop and
            // delivered to the fiber waiting on the read. The fiber
            // would then sit in `ev/join` indefinitely. Leave the
            // struct in place; the worker reaps itself via channel
            // disconnect at AsyncBackend drop time.
            //
            // See `docs/io.md` "Closing `*stdin*`".
            if matches!(port_key, PortKey::Stdin) {
                {
                    let inner = self.inner.borrow();
                    if let Some(ref st) = inner.stdin_thread {
                        st.shutdown();
                    }
                }
                port.close();
                let mut inner = self.inner.borrow_mut();
                let id = inner.mint_id();
                inner.completions.push_back(Completion::ok(id, Value::NIL));
                return Ok(id);
            }
            if let PortKey::Fd(fd, _) = &port_key {
                let mut inner = self.inner.borrow_mut();
                // Cancel all pending ops on this fd
                let ids_to_cancel: Vec<SubmissionId> = inner
                    .pending
                    .iter()
                    .filter_map(|(&op_id, op)| match op {
                        PendingOp::Port { port_key: pk, .. } if *pk == port_key => Some(op_id),
                        _ => None,
                    })
                    .collect();

                let still_running = !ids_to_cancel.is_empty();
                // A CONNECTED stream socket is woken by shutdown(2): the worker's
                // poll reports the fd readable, its read returns 0, and the fiber
                // sees a clean EOF. Every other descriptor needs the operation's
                // stop pipe instead: shutdown of a LISTENING socket wakes a
                // parked accept only on Linux — macOS and the BSDs return
                // ENOTCONN and wake nothing — and an unconnected UDP socket, a
                // pipe, or a file is not a connected socket anywhere. A worker
                // left unwoken polls the retired descriptor forever, and the
                // fiber waiting on it is never resumed (pinned by
                // `closing_a_listener_ends_its_parked_pool_accept`).
                let stream_socket =
                    matches!(port.kind(), PortKind::TcpStream | PortKind::UnixStream);
                for op_id in ids_to_cancel {
                    match inner.platform {
                        #[cfg(target_os = "linux")]
                        PlatformBackend::Uring(ref mut ring) => {
                            let _ = crate::io::uring::submit_uring_cancel(ring, op_id);
                        }
                        PlatformBackend::ThreadPool => {
                            // Do NOT remove the pending entry — let the worker's
                            // error completion flow back so the fiber resumes and
                            // can exit cleanly.
                            if stream_socket {
                                unsafe { libc::shutdown(*fd, libc::SHUT_RDWR) };
                            } else {
                                inner.hub.stop(op_id);
                            }
                        }
                    }
                }

                // A pool worker resolves its descriptor when it runs, so the
                // number must not go back to the OS while an operation still
                // names it — a new socket handed that number would be read by
                // the stale operation, and its bytes would reach no fiber.
                // Hold the descriptor here instead; `close_drained_fds` closes
                // it, and drops its `fd_states` entry, once the last operation
                // naming it has completed.
                let retired =
                    if still_running && matches!(inner.platform, PlatformBackend::ThreadPool) {
                        match port.retire_fd() {
                            Some(owned) => {
                                inner.retired.insert(*fd, owned);
                                true
                            }
                            None => false,
                        }
                    } else {
                        false
                    };
                if !retired {
                    crate::io::types::discard_fd_state(&mut inner.fd_states, *fd);
                }

                drop(inner);
            }

            // Close the port. Already a no-op when the descriptor was retired:
            // `retire_fd` marked the port closed when it took the descriptor.
            port.close();

            // Queue immediate completion.
            let mut inner = self.inner.borrow_mut();
            let id = inner.mint_id();
            inner.completions.push_back(Completion::ok(id, Value::NIL));
            return Ok(id);
        }

        if port.is_closed() {
            return Err("io/submit: port is closed".into());
        }

        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();

        let port_key = PortKey::from_port(port);

        // Seek and Tell: synchronous file-only ops — handle as immediate completions.
        // Must come before stdin routing and buffer allocation.
        if matches!(&request.op, IoOp::Seek { .. } | IoOp::Tell) {
            return inner.handle_seek_tell(id, port, &port_key, &request.op);
        }

        // Everything above either returned or was portless, so what is left is
        // an operation the backend runs asynchronously against this port.
        let op = match &request.op {
            IoOp::Port(op) => op,
            other => {
                return Err(format!(
                    "io/submit: {:?} does not operate on an open port",
                    other
                ))
            }
        };

        // For stdin, route to stdin thread
        if matches!(port_key, PortKey::Stdin) {
            return inner.submit_stdin(id, op);
        }

        // Determine fd
        let fd = port_key.raw_fd();

        let buf_handle = match op {
            PortOp::ReadLine { .. } | PortOp::Read { .. } | PortOp::ReadExact { .. } => None,
            _ => Some(inner.buffer_pool.alloc(4096)),
        };

        // Flush on socket/pipe/stdio ports is a no-op: fsync(2) returns EINVAL on
        // non-file fds (sockets, pipes, and stdio when redirected to pipes in subprocesses).
        // Return an immediate successful completion rather than submitting to the pool.
        if matches!(op, PortOp::Flush)
            && matches!(
                port.kind(),
                PortKind::TcpStream
                    | PortKind::UnixStream
                    | PortKind::UdpSocket
                    | PortKind::Pipe
                    | PortKind::Stdout
                    | PortKind::Stderr
            )
        {
            if let Some(bh) = buf_handle {
                inner.buffer_pool.release(bh);
            }
            inner.completions.push_back(Completion::ok(id, Value::NIL));
            return Ok(id);
        }

        // A previous read on this port took more from the kernel than it
        // answered with, and the remainder belongs to this one. When it already
        // answers the request in full, this read finishes here and no backend
        // runs — which is not merely a saved syscall: a read submitted for bytes
        // the port is already holding would park until the peer sent more, and a
        // peer that has said everything it has to say never would.
        //
        // When the remainder falls short it stays exactly where it is. The
        // completion joins it to the bytes this read produces (`assemble_read`),
        // so the fiber's buffer holds only what a kernel read put there. Moving
        // the remainder in instead would make the buffer hold both, and no size
        // fixed in advance can promise that: a remainder is as long as whatever
        // the last kernel read returned.
        let port_encoding = port.encoding();
        let gen = inner.unicode_generation;
        {
            let origin_heap = inner.origin_heap;
            let state = crate::io::types::fd_state_mut(&mut inner.fd_states, &port_key);
            let held = match op {
                PortOp::ReadLine { buffer } => state
                    .buffer
                    .iter()
                    .position(|&b| b == b'\n')
                    .map(|pos| (buffer, pos + 1, Encoding::Text)),
                PortOp::Read { count, buffer } => {
                    (state.buffer.len() >= *count).then_some((buffer, *count, port_encoding))
                }
                PortOp::ReadExact { count, buffer } => {
                    crate::io::frame::exact_end(&state.buffer, *count, port_encoding, gen)
                        .map(|end| (buffer, end, port_encoding))
                }
                _ => None,
            };
            if let Some((buffer, take, encoding)) = held {
                let chunk: Vec<u8> = state.buffer.drain(..take).collect();
                // A line's terminator is not part of it; the other two answer
                // with every byte they took.
                let answer = if matches!(op, PortOp::ReadLine { .. }) {
                    chunk[..crate::io::frame::line_end(&chunk).0].to_vec()
                } else {
                    chunk
                };
                let result = crate::io::frame::read_result(buffer, answer, encoding, origin_heap);
                if let Some(bh) = buf_handle {
                    inner.buffer_pool.release(bh);
                }
                inner.completions.push_back(Completion::new(id, result));
                return Ok(id);
            }
        }

        // Dispatch by operation type
        match op {
            PortOp::Accept { .. }
            | PortOp::SendTo { .. }
            | PortOp::RecvFrom { .. }
            | PortOp::Shutdown { .. } => {
                Self::submit_socket(&mut inner, request, op, id, fd, port_key, port, buf_handle)
            }
            PortOp::ReadLine { .. }
            | PortOp::Read { .. }
            | PortOp::ReadExact { .. }
            | PortOp::ReadAll
            | PortOp::Write { .. }
            | PortOp::Flush => {
                let origin_heap = inner.origin_heap;
                let AsyncBackendInner {
                    ref mut platform,
                    ref mut hub,
                    ref mut buffer_pool,
                    ref mut pending,
                    ..
                } = *inner;

                match platform {
                    #[cfg(target_os = "linux")]
                    PlatformBackend::Uring(ring) => {
                        crate::io::uring::submit_uring_stream(
                            ring,
                            id,
                            fd,
                            op,
                            request.timeout,
                            buffer_pool,
                            buf_handle,
                        )?;
                    }
                    PlatformBackend::ThreadPool => {
                        let _ = buffer_pool;
                        // A read takes a stop pipe: `io/cancel` must end it
                        // rather than abandon it, or the abandoned read goes on
                        // consuming bytes meant for whoever reads the port next.
                        //
                        // A write takes the deadline without one. Nothing it
                        // holds is contended — stopping one would only cut the
                        // payload short, and a peer decoding a stream cannot
                        // use half a message. It runs to the end of its payload
                        // as the full-write invariant promises, and gives its
                        // worker back then.
                        //
                        // `Flush` waits on nobody: `fsync(2)` transfers what
                        // this process already handed the kernel.
                        let bounds = match op {
                            PortOp::Read { .. }
                            | PortOp::ReadExact { .. }
                            | PortOp::ReadLine { .. }
                            | PortOp::ReadAll => hub.bounds(id, request.timeout),
                            PortOp::Write { .. } => Bounds::new(request.timeout, None),
                            _ => Bounds::prompt(),
                        };
                        // Each read asks the kernel for its whole count. The
                        // remainder the port is holding is short of that count
                        // — a remainder that met it answered above — but by how
                        // much is a question in the port's own unit, and only
                        // the completion, holding the join, can answer it. So
                        // the worker may bring back more than the request needs,
                        // and the completion gives the surplus to the port.
                        let pool_op = match op {
                            PortOp::ReadLine { .. } => PoolOp::ReadLine { fd },
                            PortOp::ReadAll => PoolOp::ReadAll { fd },
                            PortOp::Read { count, .. } => PoolOp::Read { fd, size: *count },
                            PortOp::ReadExact { count, .. } => PoolOp::ReadExact {
                                fd,
                                size: *count,
                                graphemes: matches!(port.encoding(), Encoding::Text),
                                gen,
                            },
                            PortOp::Write { data } => PoolOp::Write {
                                fd,
                                data: Self::extract_write_bytes(data),
                            },
                            PortOp::Flush => PoolOp::Flush { fd },
                            // The socket arm above claims these.
                            PortOp::Accept { .. }
                            | PortOp::SendTo { .. }
                            | PortOp::RecvFrom { .. }
                            | PortOp::Shutdown { .. } => unreachable!(),
                        };
                        hub.submit(id, pool_op, bounds)?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp::Port {
                        op: op.clone(),
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind: None,
                        filled: 0,
                        timeout: request.timeout,
                    },
                    origin_heap,
                );
                Ok(id)
            }
        }
    }
}

mod socket;
