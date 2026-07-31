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
        if let IoOp::Open {
            ref path,
            flags,
            mode,
            direction,
            encoding,
        } = request.op
        {
            return self.submit_open(
                path,
                flags,
                mode,
                direction,
                encoding,
                request.timeout,
                request.port,
            );
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
            if let PortKey::Fd(fd) = &port_key {
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

                for _op_id in ids_to_cancel {
                    match inner.platform {
                        #[cfg(target_os = "linux")]
                        PlatformBackend::Uring(ref mut ring) => {
                            let _ = crate::io::uring::submit_uring_cancel(ring, _op_id);
                        }
                        PlatformBackend::ThreadPool => {
                            // Thread pool: shutdown the fd to unblock any worker
                            // stuck in accept/read/recv. Do NOT remove the pending
                            // entry — let the worker's error completion flow back
                            // so the fiber resumes and can exit cleanly.
                            unsafe { libc::shutdown(*fd, libc::SHUT_RDWR) };
                        }
                    }
                }

                // Remove fd state
                inner.fd_states.remove(&PortKey::Fd(*fd));

                drop(inner);
            }

            // Now actually close the port (drops the fd).
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

        // For stdin, route to stdin thread
        if matches!(port_key, PortKey::Stdin) {
            return inner.submit_stdin(id, &request.op);
        }

        // Determine fd
        let fd = match &port_key {
            PortKey::Stdout => 1,
            PortKey::Stderr => 2,
            PortKey::Fd(raw) => *raw,
            PortKey::Stdin => unreachable!(),
        };

        let buf_handle = match &request.op {
            IoOp::ReadLine { .. } | IoOp::Read { .. } | IoOp::ReadExact { .. } => None,
            _ => Some(inner.buffer_pool.alloc(4096)),
        };

        // Flush on socket/pipe/stdio ports is a no-op: fsync(2) returns EINVAL on
        // non-file fds (sockets, pipes, and stdio when redirected to pipes in subprocesses).
        // Return an immediate successful completion rather than submitting to the pool.
        if matches!(&request.op, IoOp::Flush)
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

        // ReadLine / Read: check per-fd buffer first.
        // When a previous raw libc::read returned more data than one line (common
        // with TCP), the excess is stored in fd_states[port_key].buffer.
        // Serve subsequent reads from the buffer before submitting to the pool.
        //
        // `read_buffered` tracks how many bytes were already in the buffer
        // when a Read request can't be fully served — the completion handler
        // must prepend those bytes to the fd data.
        let port_encoding = port.encoding();
        let mut read_buffered: usize = 0;
        {
            let state = inner
                .fd_states
                .entry(port_key.clone())
                .or_insert_with(FdState::new);
            match &request.op {
                IoOp::ReadLine { buffer } => {
                    if let Some(pos) = state.buffer.iter().position(|&b| b == b'\n') {
                        let line_bytes: Vec<u8> = state.buffer.drain(..=pos).collect();
                        unsafe {
                            let (dst, dst_cap) = crate::io::request::writeable_buffer_ptr(buffer);
                            let copy_len = line_bytes.len().min(dst_cap);
                            std::ptr::copy_nonoverlapping(line_bytes.as_ptr(), dst, copy_len);
                            let final_len = if copy_len > 0 && line_bytes[copy_len - 1] == b'\n' {
                                let mut end = copy_len - 1;
                                if end > 0 && line_bytes[end - 1] == b'\r' {
                                    end -= 1;
                                }
                                end
                            } else {
                                copy_len
                            };
                            crate::io::request::truncate_buffer(buffer, final_len);
                        }
                        // Transmute LBytes → LString (zero-copy)
                        let result = unsafe {
                            crate::io::request::bytes_to_string_in_place(*buffer, origin_heap)
                        };
                        inner.completions.push_back(Completion::new(id, result));
                        return Ok(id);
                    }
                    // No newline found in state.buffer, but there IS buffered data.
                    // Copy it into the fiber buffer at offset 0 so the completion
                    // handler can prepend it to the kernel/thread-pool data.
                    if !state.buffer.is_empty() {
                        unsafe {
                            let (dst, dst_cap) = crate::io::request::writeable_buffer_ptr(buffer);
                            let copy_len = state.buffer.len().min(dst_cap);
                            std::ptr::copy_nonoverlapping(state.buffer.as_ptr(), dst, copy_len);
                        }
                        read_buffered = state.buffer.len();
                        state.buffer.clear();
                    }
                }
                IoOp::Read { count, buffer } => {
                    if state.buffer.len() >= *count {
                        let chunk: Vec<u8> = state.buffer.drain(..*count).collect();
                        unsafe {
                            let (dst, dst_cap) = crate::io::request::writeable_buffer_ptr(buffer);
                            let copy_len = chunk.len().min(dst_cap);
                            std::ptr::copy_nonoverlapping(chunk.as_ptr(), dst, copy_len);
                            crate::io::request::truncate_buffer(buffer, copy_len);
                        }
                        let result = if port_encoding == Encoding::Text {
                            unsafe {
                                crate::io::request::bytes_to_string_in_place(*buffer, origin_heap)
                            }
                        } else {
                            Ok(*buffer)
                        };
                        inner.completions.push_back(Completion::new(id, result));
                        return Ok(id);
                    }
                    // Buffered prefix is shorter than the request.  Move it
                    // into the fiber buffer at offset 0 and clear it (see the
                    // ReadExact branch below for why leaving it in state.buffer
                    // while offsetting the kernel write corrupts the result).
                    unsafe {
                        let (dst, dst_cap) = crate::io::request::writeable_buffer_ptr(buffer);
                        let copy_len = state.buffer.len().min(dst_cap);
                        std::ptr::copy_nonoverlapping(state.buffer.as_ptr(), dst, copy_len);
                    }
                    read_buffered = state.buffer.len();
                    state.buffer.clear();
                }
                IoOp::ReadExact { count, buffer } => {
                    // ReadExact's unit is whatever the port is measured in:
                    // bytes for Binary, graphemes for Text.  If the buffered
                    // prefix already contains `count` units, serve from buffer.
                    let early = match port.encoding() {
                        Encoding::Binary => {
                            if state.buffer.len() >= *count {
                                Some(*count)
                            } else {
                                None
                            }
                        }
                        Encoding::Text => crate::io::nth_grapheme_byte_end(&state.buffer, *count),
                    };
                    if let Some(take_bytes) = early {
                        let chunk: Vec<u8> = state.buffer.drain(..take_bytes).collect();
                        let heap =
                            unsafe { &mut *crate::io::completion_heap_ptr(inner.origin_heap) };
                        let ctx = crate::primitives::ctx::Alloc::new(heap);
                        let value = match port.encoding() {
                            Encoding::Text => ctx.string(String::from_utf8_lossy(&chunk).as_ref()),
                            Encoding::Binary => ctx.bytes(chunk),
                        };
                        if let Some(bh) = buf_handle {
                            inner.buffer_pool.release(bh);
                        }
                        inner.completions.push_back(Completion::ok(id, value));
                        return Ok(id);
                    }
                    // Not enough yet.  For Binary, move the buffered prefix
                    // into the fiber buffer at offset 0 and clear it — exactly
                    // the ReadLine no-newline branch above.  The kernel then
                    // writes at dst+read_buffered, so the completion sees an
                    // empty fd_state buffer and needs no shift.
                    //
                    // Leaving the prefix in state.buffer while ALSO offsetting
                    // the kernel write (read_buffered) double-handled it: the
                    // completion's shift-prepend branch moves kernel data as if
                    // it sat at dst[0] when it actually sits at dst+filled,
                    // stranding `read_buffered` zero bytes in the middle of the
                    // reassembled result.  That corrupted any read-exact that
                    // followed a read-line whose recv over-read past the line
                    // (the redis bulk-string framing bug — corruption at the
                    // byte offset equal to the over-read length).
                    //
                    // Text stays as-is: its buffer is oversized (4 B/grapheme)
                    // and the completion grapheme-splits the combined buffer.
                    if matches!(port.encoding(), Encoding::Binary) {
                        unsafe {
                            let (dst, dst_cap) = crate::io::request::writeable_buffer_ptr(buffer);
                            let copy_len = state.buffer.len().min(dst_cap);
                            std::ptr::copy_nonoverlapping(state.buffer.as_ptr(), dst, copy_len);
                        }
                        read_buffered = state.buffer.len();
                        state.buffer.clear();
                    }
                }
                _ => {}
            }
        }

        // Dispatch by operation type
        match &request.op {
            IoOp::Accept { .. }
            | IoOp::SendTo { .. }
            | IoOp::RecvFrom { .. }
            | IoOp::Shutdown { .. } => {
                Self::submit_socket(&mut inner, request, id, fd, port_key, port, buf_handle)
            }
            // Stream I/O ops (ReadLine, Read, ReadAll, Write, Flush)
            _ => {
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
                            &request.op,
                            request.timeout,
                            buffer_pool,
                            buf_handle,
                            read_buffered,
                        )?;
                    }
                    PlatformBackend::ThreadPool => {
                        let _ = buffer_pool;
                        let pool_op = match &request.op {
                            IoOp::ReadLine { .. } => PoolOp::ReadLine {
                                fd,
                                timeout: request.timeout,
                            },
                            IoOp::ReadAll => PoolOp::ReadAll {
                                fd,
                                timeout: request.timeout,
                            },
                            IoOp::Read { count, .. } => PoolOp::Read {
                                fd,
                                size: *count - read_buffered,
                                timeout: request.timeout,
                            },
                            IoOp::ReadExact { count, .. } => {
                                let is_text = matches!(port.encoding(), Encoding::Text);
                                if is_text {
                                    // Grapheme-counted: the worker grows its
                                    // own buffer and loops until `count`
                                    // graphemes are present.  `read_buffered`
                                    // bytes already sitting in fd_state are
                                    // handled by the completion path on the
                                    // combined buffer.
                                    PoolOp::ReadExact {
                                        fd,
                                        size: *count,
                                        graphemes: true,
                                        timeout: request.timeout,
                                    }
                                } else {
                                    PoolOp::ReadExact {
                                        fd,
                                        size: *count - read_buffered,
                                        graphemes: false,
                                        timeout: request.timeout,
                                    }
                                }
                            }
                            IoOp::Write { data } => {
                                let bytes = Self::extract_write_bytes(data);
                                PoolOp::Write {
                                    fd,
                                    data: bytes,
                                    timeout: request.timeout,
                                }
                            }
                            IoOp::Flush => PoolOp::Flush { fd },
                            _ => unreachable!(),
                        };
                        hub.submit(id, pool_op)?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp::Port {
                        op: match &request.op {
                            IoOp::ReadLine { buffer } => IoOp::ReadLine { buffer: *buffer },
                            IoOp::Read { count, buffer } => IoOp::Read {
                                count: *count,
                                buffer: *buffer,
                            },
                            IoOp::ReadExact { count, buffer } => IoOp::ReadExact {
                                count: *count,
                                buffer: *buffer,
                            },
                            IoOp::ReadAll => IoOp::ReadAll,
                            IoOp::Write { data } => IoOp::Write { data: *data },
                            IoOp::Flush => IoOp::Flush,
                            _ => unreachable!(),
                        },
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind: None,
                        filled: read_buffered,
                        timeout: request.timeout,
                    },
                );
                Ok(id)
            }
        }
    }
}

mod socket;
