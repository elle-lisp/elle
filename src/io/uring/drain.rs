use super::*;

/// Push a resubmitted operation, re-arming the caller's timeout as a linked
/// timeout SQE so the bound applies to this operation as it did to the first.
///
/// Returns the `Timespec` the kernel reads when it processes the SQE. It must
/// stay alive until `ring.submit()` hands the queue over, so the caller holds
/// it — a resubmit loop pushes many SQEs before one submit.
#[must_use = "the timespec must outlive the ring.submit() that consumes the SQE"]
fn push_resubmit(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    entry: io_uring::squeue::Entry,
    timeout: Option<Duration>,
) -> Option<Box<io_uring::types::Timespec>> {
    let entry = if timeout.is_some() {
        entry.flags(io_uring::squeue::Flags::IO_LINK)
    } else {
        entry
    };
    unsafe {
        let _ = ring.submission().push(&entry);
    }
    let dur = timeout?;
    let ts = Box::new(
        io_uring::types::Timespec::new()
            .sec(dur.as_secs())
            .nsec(dur.subsec_nanos()),
    );
    let timeout_sqe = io_uring::opcode::LinkTimeout::new(&*ts)
        .build()
        .user_data(id.as_u64() | TIMEOUT_USER_DATA_TAG);
    unsafe {
        let _ = ring.submission().push(&timeout_sqe);
    }
    Some(ts)
}

/// Drain all available CQEs from the completion ring.
///
/// This is the **single** CQE processing path — used by both poll (non-blocking)
/// and wait (after blocking). Handles:
/// - Timeout CQE filtering (high-bit user_data tag)
/// - Connect fd cleanup on error
/// - PortOp-aware buffer extraction (only reads buffer for stream reads)
#[allow(clippy::too_many_arguments)]
pub(crate) fn drain_cqes(
    ring: &mut io_uring::IoUring,
    pending: &mut HashMap<SubmissionId, PendingOp>,
    buffer_pool: &mut BufferPool,
    fd_states: &mut HashMap<PortKey, FdState>,
    completions: &mut VecDeque<Completion>,
    // The requesting instance's heap; completion values are born on it
    // (`crate::io::completion_heap_ptr`).
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
    // The owning VM's Unicode generation; text ReadExact decides "enough
    // graphemes yet?" and splits the stream with it.
    gen: crate::segment::Generation,
    // Set to true if the standing eventfd bridge `POLL_ADD` fired (a hub worker
    // raised the eventfd). The caller clears the eventfd and re-arms the poll —
    // the sentinel CQE has no `pending` entry and no buffer to process.
    eventfd_fired: &mut bool,
) {
    // Collect ReadLine and short-Read ops that need re-submission (can't
    // submit SQEs while iterating the CQ ring).
    let mut read_resubmits: Vec<(SubmissionId, RawFd, usize, PendingOp)> = Vec::new();
    // Short writes needing re-submission, collected for the same reason.
    let mut write_resubmits: Vec<(SubmissionId, RawFd, PendingOp)> = Vec::new();

    for cqe in ring.completion() {
        let user_data = cqe.user_data();
        let result_code = cqe.result();

        // Bridge eventfd POLL_ADD CQE: a hub worker raised the eventfd to wake
        // this wait. Record the edge and skip — it carries no pending op. Matched
        // before the timeout tag (its high bit is clear) and the pending lookup.
        if user_data == EVENTFD_USER_DATA {
            *eventfd_fired = true;
            continue;
        }

        // Timeout CQEs have the high bit set — skip them.
        if user_data & TIMEOUT_USER_DATA_TAG != 0 {
            continue;
        }

        let id = SubmissionId::from_raw(user_data);
        if let Some(mut pending_op) = pending.remove(&id) {
            // Connect: on failure, close the pre-created socket.
            if let PendingOp::Connect {
                ref mut connect_fd, ..
            } = pending_op
            {
                if let Some(fd) = *connect_fd {
                    if result_code < 0 {
                        unsafe { libc::close(fd) };
                        *connect_fd = None;
                    }
                }
            }

            // Only read from the buffer for stream I/O ops where result_code
            // is a byte count. Accept (result = new fd), Connect (result = 0),
            // Sleep, Shutdown, Flush — no buffer data.
            //
            // RecvFrom uses RecvMsg with a scratch control layout:
            //   [msghdr | iovec | sockaddr_storage]
            // The payload was received zero-copy into the result's `:data`
            // buffer (the iovec points there), so we extract only the sender
            // address and encode it as:
            //   addr_len(4 LE) + sockaddr_storage
            // The completion truncates `:data` to `result_code` and fills
            // `:addr`/`:port` from this. (The thread-pool path appends the
            // payload after the sockaddr; the completion copies it in then.)
            let buf_handle = pending_op.buffer_handle();
            let data = match &pending_op {
                PendingOp::Port { op, .. } => match op {
                    PortOp::RecvFrom { .. } if result_code > 0 => {
                        let msghdr_size = std::mem::size_of::<libc::msghdr>();
                        let iovec_size = std::mem::size_of::<libc::iovec>();
                        let sockaddr_size = std::mem::size_of::<libc::sockaddr_storage>();
                        let buf = buffer_pool.get_mut(buf_handle.unwrap());

                        // Read actual address length from msghdr (kernel updates msg_namelen)
                        let addr_len = unsafe {
                            let msg_ptr = buf.as_ptr() as *const libc::msghdr;
                            (*msg_ptr).msg_namelen
                        };

                        // Extract sockaddr_storage bytes
                        let sa_start = msghdr_size + iovec_size;
                        let sa_bytes = buf[sa_start..sa_start + sockaddr_size].to_vec();

                        // Encode: addr_len(4 LE) + sockaddr_storage (no payload — it
                        // is already in the fiber-heap `:data` buffer).
                        let mut encoded = Vec::with_capacity(4 + sockaddr_size);
                        encoded.extend_from_slice(&addr_len.to_le_bytes());
                        encoded.extend_from_slice(&sa_bytes);
                        encoded
                    }
                    PortOp::ReadLine { .. } | PortOp::Read { .. } | PortOp::ReadExact { .. }
                        if result_code > 0 =>
                    {
                        // Data was written directly into the fiber buffer by the kernel.
                        Vec::new()
                    }
                    PortOp::ReadAll if result_code > 0 => {
                        let buf = buffer_pool.get_mut(buf_handle.unwrap());
                        buf[..result_code as usize].to_vec()
                    }
                    _ => Vec::new(),
                },
                PendingOp::WatchNext { .. } if result_code > 0 => {
                    let buf = buffer_pool.get_mut(buf_handle.unwrap());
                    buf[..result_code as usize].to_vec()
                }
                PendingOp::SigNext { .. } if result_code > 0 => {
                    let buf = buffer_pool.get_mut(buf_handle.unwrap());
                    buf[..result_code as usize].to_vec()
                }
                _ => Vec::new(),
            };

            // ReadLine re-submission: if the read returned data but no newline
            // was found in the fiber's buffer, resubmit for more data.
            if let PendingOp::Port {
                op: PortOp::ReadLine { ref buffer },
                ref port_key,
                ref mut filled,
                ..
            } = pending_op
            {
                if result_code > 0 {
                    let total_in_fiber = *filled + result_code as usize;
                    // Check for newline in the fiber's buffer content
                    let buf_bytes = buffer.as_bytes().unwrap();
                    let has_newline =
                        buf_bytes[..total_in_fiber.min(buf_bytes.len())].contains(&b'\n');
                    if !has_newline {
                        // No newline found — advance filled cursor for re-submission.
                        *filled = total_in_fiber;
                        let fd = port_key.raw_fd();
                        read_resubmits.push((id, fd, 4096, pending_op));
                        continue;
                    }
                }
            }

            // Read short-read re-submission: regular files may return short
            // reads before EOF (rare but POSIX-legal). Buffer partial data
            // and resubmit for the remainder. Stream sockets (TCP, Unix)
            // are excluded for plain `Read` — port/read returns "up to N
            // bytes" per POSIX semantics, so a short read is a normal
            // completion. `ReadExact` is the strict variant: resubmit for
            // stream sockets too so callers get exactly N bytes (or nil if
            // the stream ended early).
            let (
                count,
                buffer_ref,
                port_key_for_resubmit,
                port_for_resubmit,
                filled_for_resubmit,
                is_exact,
            ) = match &mut pending_op {
                PendingOp::Port {
                    op: PortOp::Read { count, buffer },
                    port_key,
                    port,
                    filled,
                    ..
                } => (
                    Some(*count),
                    Some(*buffer),
                    Some(port_key.clone()),
                    Some(*port),
                    Some(filled),
                    false,
                ),
                PendingOp::Port {
                    op: PortOp::ReadExact { count, buffer },
                    port_key,
                    port,
                    filled,
                    ..
                } => (
                    Some(*count),
                    Some(*buffer),
                    Some(port_key.clone()),
                    Some(*port),
                    Some(filled),
                    true,
                ),
                _ => (None, None, None, None, None, false),
            };
            if let (Some(count), Some(buffer), Some(port_key), Some(port), Some(filled)) = (
                count,
                buffer_ref,
                port_key_for_resubmit,
                port_for_resubmit,
                filled_for_resubmit,
            ) {
                let is_stream = port
                    .as_external::<Port>()
                    .map(|p| matches!(p.kind(), PortKind::TcpStream | PortKind::UnixStream))
                    .unwrap_or(false);
                if (is_exact || !is_stream) && result_code > 0 {
                    let got = result_code as usize;
                    let state = fd_states
                        .entry(port_key.clone())
                        .or_insert_with(FdState::new);
                    let total_in_fiber = *filled + got;
                    let total = state.buffer.len() + total_in_fiber;
                    // Text ReadExact counts grapheme clusters, not bytes: a
                    // multibyte grapheme spans several bytes, so reading `count`
                    // bytes can leave a grapheme split mid-sequence (the
                    // "invalid UTF-8 in N bytes" symptom).  Decide "enough yet?"
                    // in the port's own unit, then let completion split at the
                    // Nth grapheme boundary and stash the remainder.
                    let text_exact = is_exact
                        && port
                            .as_external::<Port>()
                            .map(|p| p.encoding() == crate::port::Encoding::Text)
                            .unwrap_or(false);
                    let need_more = if text_exact {
                        let fiber_bytes = unsafe {
                            let (dst, _) = crate::io::request::writeable_buffer_ptr(&buffer);
                            std::slice::from_raw_parts(dst, total_in_fiber)
                        };
                        let mut combined = state.buffer.clone();
                        combined.extend_from_slice(fiber_bytes);
                        crate::io::grapheme_count_in_valid_prefix(&combined, gen) < count
                    } else {
                        total < count
                    };
                    if need_more {
                        // Short read — copy from fiber buffer into state.buffer and resubmit.
                        unsafe {
                            let (dst, _) = crate::io::request::writeable_buffer_ptr(&buffer);
                            state
                                .buffer
                                .extend_from_slice(std::slice::from_raw_parts(dst, total_in_fiber));
                        }
                        // Reset filled for re-submission (data moved to state.buffer)
                        *filled = 0;
                        let fd = port_key.raw_fd();
                        // The resubmit loop reads `buf_len - filled` bytes into
                        // the fiber buffer (filled was just reset to 0), so the
                        // size passed here is unused — kept 0 for clarity.
                        read_resubmits.push((id, fd, 0, pending_op));
                        continue;
                    }
                }
            }

            // ReadAll re-submission: buffer data and resubmit until EOF
            // (result_code == 0). ReadAll reads until the write end closes.
            if let PendingOp::Port {
                op: PortOp::ReadAll,
                ref port_key,
                ..
            } = pending_op
            {
                if result_code > 0 {
                    let state = fd_states
                        .entry(port_key.clone())
                        .or_insert_with(FdState::new);
                    state.buffer.extend_from_slice(&data);
                    if let Some(bh) = buf_handle {
                        buffer_pool.release(bh);
                    }
                    let fd = port_key.raw_fd();
                    read_resubmits.push((id, fd, 4096, pending_op));
                    continue;
                }
            }

            // Write re-submission. One write(2) transfers only what fits in the
            // fd's send buffer at that moment, which on a socket is routinely a
            // fraction of a large payload. `port/write` writes every byte before
            // it returns (docs/io.md), so resubmit the unwritten tail from the
            // same pooled buffer — `filled` is the offset the payload has been
            // accepted up to — and let the operation complete only once nothing
            // is left. The completion reports `filled + result_code`.
            let mut write_stalled = false;
            if let PendingOp::Port {
                op: PortOp::Write { .. },
                ref port_key,
                ref mut filled,
                ..
            } = pending_op
            {
                // The pooled buffer holds the whole payload (copied there at
                // submission), so its length is what the write owes.
                let payload_len = buf_handle
                    .map(|bh| buffer_pool.get_mut(bh).len())
                    .unwrap_or(0);
                if result_code >= 0 && *filled + (result_code as usize) < payload_len {
                    if result_code == 0 {
                        // The kernel accepted nothing of a non-empty tail.
                        // Resubmitting would spin, and reporting the partial
                        // count would read as a completed write to a caller
                        // that trusts the contract — so fail the operation.
                        write_stalled = true;
                    } else {
                        *filled += result_code as usize;
                        let fd = port_key.raw_fd();
                        write_resubmits.push((id, fd, pending_op));
                        continue;
                    }
                }
            }

            // Note: filled is NOT updated here. Re-submissions update filled in
            // the re-submit block below. For final completions, process_raw_completion
            // computes total = filled + result_code, which is correct because filled
            // was set at submission time (read_buffered) or by a previous re-submission.

            let completion = process_raw_completion(
                id,
                if write_stalled {
                    -libc::EIO
                } else {
                    result_code
                },
                data,
                &pending_op,
                fd_states,
                buffer_pool,
                buf_handle,
                origin_heap,
                gen,
            );
            completions.push_back(completion);
        }
    }

    // A re-armed `LinkTimeout` hands the kernel a pointer to its `Timespec`,
    // which must stay put until the `ring.submit()` below consumes the SQE.
    // The boxes live here so every resubmission's timespec outlives that call.
    let mut link_timeouts: Vec<Box<io_uring::types::Timespec>> = Vec::new();

    // Re-submit ReadLine and short-Read ops that need more data.
    // For reads with pre-allocated buffers, re-submit into the remaining
    // space in the fiber's buffer (advance past filled bytes).
    for (id, fd, _size, mut pending_op) in read_resubmits {
        // Bound this operation the way the original submission was bounded. A
        // read that needs several operations — read-exact until its count,
        // read-all until EOF, read-line until a newline — would otherwise be
        // unbounded from its second operation on, and a peer that goes quiet
        // would hang a read that asked for a timeout.
        let op_timeout = pending_op.timeout();
        if let PendingOp::Port {
            op:
                PortOp::ReadLine { ref buffer }
                | PortOp::Read { ref buffer, .. }
                | PortOp::ReadExact { ref buffer, .. },
            ref mut filled,
            ref port_key,
            ..
        } = pending_op
        {
            // The completion will prepend whatever is already stashed in the
            // fd_state buffer (e.g. a ReadExact whose earlier short reads were
            // moved there) ahead of the bytes this read produces. Subtract
            // both the in-fiber `filled` prefix and that stash from the
            // capacity so the assembled total never exceeds the fixed buffer
            // — otherwise read-exact overflows when the peer has more bytes
            // available than `count` (e.g. a fixed header followed by a body).
            let buffered = fd_states.get(port_key).map(|s| s.buffer.len()).unwrap_or(0);
            let buf_bytes = buffer.as_bytes().unwrap();
            let remaining = buf_bytes
                .len()
                .saturating_sub(*filled)
                .saturating_sub(buffered)
                .min(MAX_READ_CHUNK);
            if remaining == 0 {
                // Buffer full — return what we have as a partial result.
                // Don't re-submit; let process_raw_completion handle it.
                // Push back into completions via process_raw_completion.
                continue;
            }
            let sqe = unsafe {
                let (dst, _) = crate::io::request::writeable_buffer_ptr(buffer);
                io_uring::opcode::Read::new(
                    io_uring::types::Fd(fd),
                    dst.add(*filled),
                    remaining as u32,
                )
                .offset(u64::MAX)
                .build()
                .user_data(id.as_u64())
            };
            pending.insert(id, pending_op);
            link_timeouts.extend(push_resubmit(ring, id, sqe, op_timeout));
        } else {
            // `ReadAll` lands here: it has no pre-allocated fiber buffer to
            // read into (its length is unknown until EOF), so each round takes
            // a fresh pooled buffer and the completion accumulates it in the
            // fd_state.
            let new_buf = buffer_pool.alloc(4096);
            let buf = buffer_pool.get_mut(new_buf);
            buf.resize(4096, 0);
            let sqe = io_uring::opcode::Read::new(
                io_uring::types::Fd(fd),
                buf.as_mut_ptr(),
                buf.len() as u32,
            )
            .offset(u64::MAX)
            .build()
            .user_data(id.as_u64());
            if let PendingOp::Port {
                ref mut buffer_handle,
                ref mut filled,
                ..
            } = pending_op
            {
                *buffer_handle = Some(new_buf);
                *filled = 0;
            }
            pending.insert(id, pending_op);
            link_timeouts.extend(push_resubmit(ring, id, sqe, op_timeout));
        }
    }

    // Re-submit the unwritten tail of each short write. The payload still sits
    // in the pooled buffer the original submission copied it into, so the tail
    // needs no second copy — only a pointer past the bytes already accepted.
    for (id, fd, pending_op) in write_resubmits {
        let (bh, filled, timeout) = match &pending_op {
            PendingOp::Port {
                buffer_handle: Some(bh),
                filled,
                timeout,
                ..
            } => (*bh, *filled, *timeout),
            // A write always carries a pooled buffer (aio::submit allocates one
            // for every non-read op), so this arm is unreachable; dropping the
            // op here would strand the fiber, so keep it pending for teardown.
            _ => continue,
        };
        let buf = buffer_pool.get_mut(bh);
        let remaining = (buf.len() - filled).min(MAX_WRITE_CHUNK);
        let sqe = unsafe {
            io_uring::opcode::Write::new(
                io_uring::types::Fd(fd),
                buf.as_ptr().add(filled),
                remaining as u32,
            )
            .offset(u64::MAX)
            .build()
            .user_data(id.as_u64())
        };
        // Bound this chunk the way the original submission was bounded. A
        // payload larger than one syscall would otherwise be unbounded from
        // its second chunk on, and a peer that stops reading would hang a
        // write that asked for a timeout.
        pending.insert(id, pending_op);
        link_timeouts.extend(push_resubmit(ring, id, sqe, timeout));
    }

    if !ring.submission().is_empty() {
        let _ = ring.submit();
    }
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn wait_uring(
    ring: &mut io_uring::IoUring,
    timeout: Option<u64>,
    pending: &mut HashMap<SubmissionId, PendingOp>,
    buffer_pool: &mut BufferPool,
    fd_states: &mut HashMap<PortKey, FdState>,
    completions: &mut VecDeque<Completion>,
    // The requesting instance's heap; completion values are born on it
    // (`crate::io::completion_heap_ptr`).
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
    // The owning VM's Unicode generation, forwarded to drain_cqes.
    gen: crate::segment::Generation,
    // The bridge eventfd (`Some` on the uring platform). When its standing
    // `POLL_ADD` is what woke this wait, the counter is cleared and the poll
    // re-armed before returning so the next wait stays wakeable. The hub
    // channel that carries the actual completion is drained by the caller.
    eventfd: Option<RawFd>,
) -> Result<(), String> {
    let mut eventfd_fired = false;
    // Block until at least one CQE is available (or timeout).
    match timeout {
        Some(0) => {} // poll only — no wait
        Some(ms) => {
            let ts = io_uring::types::Timespec::new()
                .sec(ms / 1000)
                .nsec(((ms % 1000) * 1_000_000) as u32);
            let args = io_uring::types::SubmitArgs::new().timespec(&ts);
            loop {
                match ring.submitter().submit_with_args(1, &args) {
                    Ok(_) => break,
                    Err(e) if e.raw_os_error() == Some(libc::EINTR) => {
                        // Interrupted by a signal (e.g. SIGCHLD from a subprocess
                        // in a concurrent test). Retry — the timeout is still active.
                        continue;
                    }
                    Err(e) if e.raw_os_error() == Some(libc::ETIME) => {
                        // Timeout expired with no completions — that's valid.
                        break;
                    }
                    Err(e) => {
                        return Err(format!("io/wait: io_uring wait failed: {}", e));
                    }
                }
            }
        }
        None => loop {
            match ring.submit_and_wait(1) {
                Ok(_) => break,
                Err(e) if e.raw_os_error() == Some(libc::EINTR) => continue,
                Err(e) => {
                    return Err(format!("io/wait: io_uring wait failed: {}", e));
                }
            }
        },
    }

    drain_cqes(
        ring,
        pending,
        buffer_pool,
        fd_states,
        completions,
        origin_heap,
        gen,
        &mut eventfd_fired,
    );

    // If drain_cqes resubmitted ops (ReadAll/ReadLine) and produced no
    // completions, loop to wait for the resubmitted read's CQE. Only do
    // this for blocking waits (no timeout) — callers with timeouts
    // should return and retry via the outer event loop.
    //
    // Stop if the eventfd bridge fired: the wake came from a hub worker whose
    // completion sits in the channel (drained by the caller), not from a ring
    // op. A pending *pool* op posts no ring CQE, so looping on `submit_and_wait`
    // here — with the one-shot `POLL_ADD` already consumed — would block forever.
    if timeout.is_none() {
        while completions.is_empty() && !pending.is_empty() && !eventfd_fired {
            loop {
                match ring.submit_and_wait(1) {
                    Ok(_) => break,
                    Err(e) if e.raw_os_error() == Some(libc::EINTR) => continue,
                    Err(e) => {
                        return Err(format!("io/wait: io_uring wait failed: {}", e));
                    }
                }
            }
            drain_cqes(
                ring,
                pending,
                buffer_pool,
                fd_states,
                completions,
                origin_heap,
                gen,
                &mut eventfd_fired,
            );
        }
    }

    // The bridge eventfd's `POLL_ADD` is one-shot. If it fired, clear the
    // counter (so the re-armed poll blocks instead of completing on a stale
    // count) and re-arm so the next wait is wakeable. A re-arm failure must
    // propagate — a deaf bridge would hang a later wait, and surfacing it here
    // fails a test instead.
    if eventfd_fired {
        if let Some(efd) = eventfd {
            crate::io::eventfd::drain(efd);
            arm_eventfd_poll(ring, efd)?;
        }
    }
    Ok(())
}
