use super::*;

/// Drain all available CQEs from the completion ring.
///
/// This is the **single** CQE processing path — used by both poll (non-blocking)
/// and wait (after blocking). Handles:
/// - Timeout CQE filtering (high-bit user_data tag)
/// - Connect fd cleanup on error
/// - IoOp-aware buffer extraction (only reads buffer for stream reads)
pub(crate) fn drain_cqes(
    ring: &mut io_uring::IoUring,
    pending: &mut HashMap<SubmissionId, PendingOp>,
    buffer_pool: &mut BufferPool,
    fd_states: &mut HashMap<PortKey, FdState>,
    completions: &mut VecDeque<Completion>,
    // The requesting instance's heap; completion values are born on it
    // (`crate::io::completion_heap_ptr`).
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
    // Set to true if the standing eventfd bridge `POLL_ADD` fired (a hub worker
    // raised the eventfd). The caller clears the eventfd and re-arms the poll —
    // the sentinel CQE has no `pending` entry and no buffer to process.
    eventfd_fired: &mut bool,
) {
    // Collect ReadLine and short-Read ops that need re-submission (can't
    // submit SQEs while iterating the CQ ring).
    let mut read_resubmits: Vec<(SubmissionId, RawFd, usize, PendingOp)> = Vec::new();

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
                    IoOp::RecvFrom { .. } if result_code > 0 => {
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
                    IoOp::ReadLine { .. } | IoOp::Read { .. } | IoOp::ReadExact { .. }
                        if result_code > 0 =>
                    {
                        // Data was written directly into the fiber buffer by the kernel.
                        Vec::new()
                    }
                    IoOp::ReadAll if result_code > 0 => {
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
                op: IoOp::ReadLine { ref buffer },
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
                        let fd = match port_key {
                            PortKey::Fd(raw) => *raw,
                            PortKey::Stdout => 1,
                            PortKey::Stderr => 2,
                            PortKey::Stdin => unreachable!(),
                        };
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
                    op: IoOp::Read { count, buffer },
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
                    op: IoOp::ReadExact { count, buffer },
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
                        crate::io::grapheme_count_in_valid_prefix(&combined) < count
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
                        let fd = match &port_key {
                            PortKey::Fd(raw) => *raw,
                            PortKey::Stdout => 1,
                            PortKey::Stderr => 2,
                            PortKey::Stdin => unreachable!(),
                        };
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
                op: IoOp::ReadAll,
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
                    let fd = match port_key {
                        PortKey::Fd(raw) => *raw,
                        PortKey::Stdout => 1,
                        PortKey::Stderr => 2,
                        PortKey::Stdin => unreachable!(),
                    };
                    read_resubmits.push((id, fd, 4096, pending_op));
                    continue;
                }
            }

            // Note: filled is NOT updated here. Re-submissions update filled in
            // the re-submit block below. For final completions, process_raw_completion
            // computes total = filled + result_code, which is correct because filled
            // was set at submission time (read_buffered) or by a previous re-submission.

            let completion = process_raw_completion(
                id,
                result_code,
                data,
                &pending_op,
                fd_states,
                buffer_pool,
                buf_handle,
                origin_heap,
            );
            completions.push_back(completion);
        }
    }

    // Re-submit ReadLine and short-Read ops that need more data.
    // For reads with pre-allocated buffers, re-submit into the remaining
    // space in the fiber's buffer (advance past filled bytes).
    for (id, fd, _size, mut pending_op) in read_resubmits {
        if let PendingOp::Port {
            op:
                IoOp::ReadLine { ref buffer }
                | IoOp::Read { ref buffer, .. }
                | IoOp::ReadExact { ref buffer, .. },
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
            unsafe {
                let (dst, _) = crate::io::request::writeable_buffer_ptr(buffer);
                let sqe = io_uring::opcode::Read::new(
                    io_uring::types::Fd(fd),
                    dst.add(*filled),
                    remaining as u32,
                )
                .offset(u64::MAX)
                .build()
                .user_data(id.as_u64());
                pending.insert(id, pending_op);
                let _ = ring.submission().push(&sqe);
            }
        } else {
            // Non-read re-submission (shouldn't happen, but handle defensively)
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
            unsafe {
                let _ = ring.submission().push(&sqe);
            }
        }
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
