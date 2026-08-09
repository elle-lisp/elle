use super::*;

/// Cook one `RawCompletion` reaped from the hub into a `Completion`, dispatching
/// on the variant. Returns `None` when no fiber is waiting for the result —
/// either the op was cancelled, or its `pending` entry is already gone. The
/// caller discards it; the hub's `in_flight` was already decremented at the
/// drain site, so a discarded item still balances the count.
///
/// A cancelled op is discarded *before* cooking. Cooking a read writes the
/// worker's bytes into the buffer the requesting fiber pre-allocated, and a
/// cancelled read's fiber is gone — the write would land in a freed heap.
pub(super) fn cook_raw(
    rc: RawCompletion,
    pending: &mut HashMap<SubmissionId, PendingOp>,
    cancelled: &mut std::collections::HashSet<SubmissionId>,
    fd_states: &mut HashMap<PortKey, FdState>,
    buffer_pool: &mut BufferPool,
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
    gen: crate::segment::Generation,
) -> Option<Completion> {
    let id = SubmissionId::from_raw(match &rc {
        RawCompletion::Pool(pc) => pc.id,
        RawCompletion::Stdin(sc) => sc.id,
    });
    if cancelled.remove(&id) {
        let result_fd = match &rc {
            RawCompletion::Pool(pc) => pc.result_code,
            RawCompletion::Stdin(_) => 0,
        };
        discard_pending(id, result_fd, pending, buffer_pool);
        return None;
    }
    match rc {
        RawCompletion::Pool(pc) => {
            pool_to_completion(pc, pending, fd_states, buffer_pool, origin_heap, gen)
        }
        RawCompletion::Stdin(sc) => stdin_to_completion(sc, pending, buffer_pool, origin_heap),
    }
}

/// Retire a cancelled operation's entry, releasing everything it owns without
/// building a value for it. `result_fd` is the raw completion's result code,
/// which for a connect is the descriptor the worker opened — nobody will take
/// it now, so it is closed here rather than leaked.
fn discard_pending(
    id: SubmissionId,
    result_fd: i32,
    pending: &mut HashMap<SubmissionId, PendingOp>,
    buffer_pool: &mut BufferPool,
) {
    let Some(op) = pending.remove(&id) else {
        return;
    };
    if let Some(bh) = op.buffer_handle() {
        buffer_pool.release(bh);
    }
    match op {
        PendingOp::Connect { connect_fd, .. } => {
            // io_uring pre-creates the socket; the pool reports it here.
            if let Some(fd) = connect_fd.or(if result_fd > 0 { Some(result_fd) } else { None }) {
                // SAFETY: nothing else holds this descriptor — the port that
                // would have owned it is never built.
                unsafe { libc::close(fd) };
            }
        }
        PendingOp::Open { .. } if result_fd > 0 => {
            // SAFETY: as above — the port for this descriptor is never built.
            unsafe { libc::close(result_fd) };
        }
        PendingOp::ProcessWait { siginfo, .. } if !siginfo.is_null() => {
            // SAFETY: allocated by `Box::into_raw` at submit; reclaimed once.
            drop(unsafe { Box::from_raw(siginfo) });
        }
        _ => {}
    }
}

/// Convert a `StdinCompletion` into a `Completion`, releasing the buffer.
pub(super) fn stdin_to_completion(
    sc: crate::io::threadpool::StdinCompletion,
    pending: &mut HashMap<SubmissionId, PendingOp>,
    buffer_pool: &mut BufferPool,
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
) -> Option<Completion> {
    let id = SubmissionId::from_raw(sc.id);
    let pending_op = pending.remove(&id)?;
    // Release BufferPool handle if present
    if let Some(bh) = pending_op.buffer_handle() {
        buffer_pool.release(bh);
    }
    Some(match sc.result {
        Ok(data) if data.is_empty() => Completion::ok(id, Value::NIL),
        Ok(data) => {
            // For Read/ReadLine, copy data into the pre-allocated buffer
            if let PendingOp::Port {
                op: PortOp::ReadLine { ref buffer } | PortOp::Read { ref buffer, .. },
                ref port,
                ..
            } = &pending_op
            {
                let enc = port
                    .as_external::<Port>()
                    .map(|p| p.encoding())
                    .unwrap_or(Encoding::Binary);
                unsafe {
                    let (dst, dst_cap) = crate::io::request::writeable_buffer_ptr(buffer);
                    let copy_len = data.len().min(dst_cap);
                    std::ptr::copy_nonoverlapping(data.as_ptr(), dst, copy_len);
                    // For ReadLine, trim trailing \r\n
                    let final_len = if matches!(
                        &pending_op,
                        PendingOp::Port {
                            op: PortOp::ReadLine { .. },
                            ..
                        }
                    ) {
                        let mut end = copy_len;
                        if end > 0 && data[end - 1] == b'\n' {
                            end -= 1;
                            if end > 0 && data[end - 1] == b'\r' {
                                end -= 1;
                            }
                        }
                        end
                    } else {
                        copy_len
                    };
                    crate::io::request::truncate_buffer(buffer, final_len);
                }
                if let PendingOp::Port {
                    op: PortOp::ReadLine { buffer } | PortOp::Read { buffer, .. },
                    ..
                } = &pending_op
                {
                    // ReadLine always returns string; Read depends on encoding
                    let result = if matches!(
                        &pending_op,
                        PendingOp::Port {
                            op: PortOp::ReadLine { .. },
                            ..
                        }
                    ) || enc == Encoding::Text
                    {
                        unsafe {
                            crate::io::request::bytes_to_string_in_place(*buffer, origin_heap)
                        }
                    } else {
                        Ok(*buffer)
                    };
                    Completion::new(id, result)
                } else {
                    unreachable!()
                }
            } else {
                // ReadAll or other — construct bytes (legacy path)
                let heap = unsafe { &mut *crate::io::completion_heap_ptr(origin_heap) };
                let ctx = crate::primitives::ctx::Alloc::new(heap);
                Completion::ok(id, ctx.bytes(data))
            }
        }
        Err(e) => Completion::err(id, crate::io::io_error("io-error", e, origin_heap)),
    })
}

/// Convert a `PoolCompletion` into a `Completion`, handling Connect fd stash.
pub(super) fn pool_to_completion(
    pc: PoolCompletion,
    pending: &mut HashMap<SubmissionId, PendingOp>,
    fd_states: &mut HashMap<PortKey, FdState>,
    buffer_pool: &mut BufferPool,
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
    gen: crate::segment::Generation,
) -> Option<Completion> {
    let id = SubmissionId::from_raw(pc.id);
    let mut pending_op = pending.remove(&id)?;
    if let PendingOp::Connect {
        ref mut connect_fd, ..
    } = pending_op
    {
        if pc.result_code > 0 {
            *connect_fd = Some(pc.result_code);
        }
    }

    // For buffer-backed reads on the thread pool, copy the worker's bytes into
    // the pre-allocated fiber buffer before `process_raw_completion` assembles
    // the result. On io_uring the kernel writes the fiber buffer directly at
    // `dst + filled`; the pool worker instead returns the bytes in `pc.data`, so
    // we stage them at the same `dst + filled` offset here. `ReadExact` belongs
    // with `Read`/`ReadLine`: the worker runs the read-exact loop internally and
    // hands back the full result in `pc.data`, and `complete_port_op` then does
    // the same `read_buffered`-prepend / grapheme-split it does for the ring.
    // (`ReadAll` is excluded — it accumulates in `fd_state.buffer` and reads
    // `pc.data` straight through `process_raw_completion`.)
    if let PendingOp::Port {
        op:
            PortOp::Read { ref buffer, .. }
            | PortOp::ReadLine { ref buffer }
            | PortOp::ReadExact { ref buffer, .. },
        filled,
        ..
    } = &pending_op
    {
        if pc.result_code > 0 && !pc.data.is_empty() {
            unsafe {
                let (dst, dst_cap) = crate::io::request::writeable_buffer_ptr(buffer);
                let offset = *filled;
                let remaining = dst_cap.saturating_sub(offset);
                let copy_len = pc.data.len().min(remaining);
                std::ptr::copy_nonoverlapping(pc.data.as_ptr(), dst.add(offset), copy_len);
                let total = offset + copy_len;
                crate::io::request::truncate_buffer(buffer, total);
            }
        }
    }

    let bh = pending_op.buffer_handle();
    Some(completion::process_raw_completion(
        id,
        pc.result_code,
        pc.data,
        &pending_op,
        fd_states,
        buffer_pool,
        bh,
        origin_heap,
        gen,
    ))
}
