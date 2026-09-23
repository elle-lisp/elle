//! audited: 2026-09-23
//! `AsyncBackend::submit` — the one entry point, and how it routes a request to
//! a portless path, an immediate answer, or a backend.
//!
//! src/io/AGENTS.md

use super::*;

impl AsyncBackend {
    /// Submit an I/O request. Returns a submission ID.
    ///
    /// `submitter` is who the submission is on behalf of: the heap results and
    /// errors are allocated on, and the fiber that will read them.
    pub(crate) fn submit(
        &self,
        request: &IoRequest,
        submitter: crate::io::pending::Submitter,
    ) -> Result<SubmissionId, String> {
        let origin_heap = submitter.heap();
        // Record the requesting instance's heap so the scheduler-thread completion
        // harvest builds every result/error value on it. Constant per backend (one
        // instance per scheduler). The submitter is recorded beside it because
        // every `pending.insert` below files an entry against it, and the dispatch
        // helpers between here and there take no argument for it.
        {
            let mut inner = self.inner.borrow_mut();
            if !origin_heap.is_null() {
                inner.origin_heap = origin_heap;
            }
            inner.submitter = submitter;
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
                let birth = crate::io::Birthplace::on(inner.origin_heap);
                inner
                    .completions
                    .push_back(Completion::ok(id, birth, Value::NIL));
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

                // A CONNECTED stream socket is woken by shutdown(2): the worker's
                // poll reports the fd readable, its read returns 0, and the fiber
                // sees a clean EOF. Every other descriptor needs the operation's
                // stop pipe instead: shutdown of a LISTENING socket wakes a
                // parked accept only on Linux — macOS and the BSDs return
                // ENOTCONN and wake nothing — and an unconnected UDP socket, a
                // pipe, or a file is not a connected socket anywhere. A worker
                // left unwoken polls the closed port's descriptor forever, and
                // the fiber waiting on it is never resumed (pinned by
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

                // The remainder a read left behind belongs to the port that
                // produced it, and that port is what this close ends.
                crate::io::types::discard_fd_state(&mut inner.fd_states, *fd);

                drop(inner);
            }

            // Close the port: it stops answering at once, so Elle's semantics
            // are those of a close. What it gives up is its SHARE of the
            // descriptor — an operation still in flight holds one of its own,
            // and the number goes back to the OS with the last of them
            // (docs/impl/io-descriptor.md § "Descriptor retirement").
            port.close();

            // Queue immediate completion.
            let mut inner = self.inner.borrow_mut();
            let id = inner.mint_id();
            let birth = crate::io::Birthplace::on(inner.origin_heap);
            inner
                .completions
                .push_back(Completion::ok(id, birth, Value::NIL));
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

        let fd = port_key.raw_fd();

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
            let birth = crate::io::Birthplace::on(inner.origin_heap);
            inner
                .completions
                .push_back(Completion::ok(id, birth, Value::NIL));
            return Ok(id);
        }

        // Answer from the remainder a previous read on this port left behind,
        // whenever it covers the request in full — `frame::line_end` and
        // `frame::exact_end` are the same cuts the completion makes. A remainder
        // that falls short goes with the read, for the completion to answer
        // from (docs/impl/io-bytes.md § "The port hands its remainder to the
        // read").
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
                // A line's terminator is not part of it; the other two answer
                // with every byte they took.
                let end = if matches!(op, PortOp::ReadLine { .. }) {
                    crate::io::frame::line_end(&state.buffer[..take]).0
                } else {
                    take
                };
                let mut birth = crate::io::Birthplace::on(origin_heap);
                let result = crate::io::frame::answer_from(
                    buffer,
                    &state.buffer[..end],
                    encoding,
                    &mut birth,
                );
                state.buffer.drain(..take);
                inner
                    .completions
                    .push_back(Completion::new(id, birth, result));
                return Ok(id);
            }
        }

        match op {
            PortOp::Accept { .. }
            | PortOp::SendTo { .. }
            | PortOp::RecvFrom { .. }
            | PortOp::Shutdown { .. } => {
                let buf_handle = Some(inner.buffer_pool.alloc(4096));
                Self::submit_socket(&mut inner, request, op, id, fd, port_key, port, buf_handle)
            }
            PortOp::ReadLine { .. }
            | PortOp::Read { .. }
            | PortOp::ReadExact { .. }
            | PortOp::ReadAll
            | PortOp::Write { .. }
            | PortOp::Flush => Self::submit_stream(&mut inner, request, op, id, fd, port_key, port),
        }
    }
}

mod socket;
mod stream;
