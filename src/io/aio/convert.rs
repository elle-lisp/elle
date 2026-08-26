use super::*;

/// Cook one `RawCompletion` reaped from the hub into a `Completion`, dispatching
/// on the variant. Returns `None` when no fiber is waiting for the result —
/// either the op was cancelled, or its `pending` entry is already gone. The
/// caller discards it; the hub's `in_flight` was already decremented at the
/// drain site, so a discarded item still balances the count.
///
/// [`PendingTable::take`] is what decides. A cancelled op is retired instead of
/// cooked, because cooking reads the values the operation held: the bytes go
/// into the buffer the requesting fiber pre-allocated, and the result is
/// assembled through the port. Both belong to a fiber that is already gone.
pub(super) fn cook_raw(
    rc: RawCompletion,
    pending: &mut PendingTable,
    fd_states: &mut HashMap<PortKey, FdState>,
    buffer_pool: &mut BufferPool,
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
    gen: crate::segment::Generation,
) -> Option<Completion> {
    match rc {
        RawCompletion::Pool(pc) => {
            pool_to_completion(pc, pending, fd_states, buffer_pool, origin_heap, gen)
        }
        RawCompletion::Stdin(sc) => stdin_to_completion(sc, pending, buffer_pool, origin_heap),
    }
}

/// Why a completion may not be cooked through the entry it resolved to, or
/// `None` when the two agree.
///
/// One id names one operation, so a disagreement is the submission table saying
/// something the worker contradicts. Cooking on would hand the wrong-shaped
/// payload to an arm that trusts what it matched — the shape of defect this
/// reports rather than performs. The report goes to the fiber as an error, which
/// is louder than any assertion: it raises in the caller's own code, in every
/// build, naming both sides.
fn misrouted(pending_op: &PendingOp, kind: OpKind, id: SubmissionId) -> Option<String> {
    if pending_op.accepts(kind) {
        return None;
    }
    Some(format!(
        "io completion {}: a {:?} operation completed, but the submission filed \
         under that id is a {} — the result is withheld rather than read as one",
        id,
        kind,
        pending_op.name()
    ))
}

/// Convert a `StdinCompletion` into a `Completion`, releasing the buffer.
pub(super) fn stdin_to_completion(
    sc: crate::io::threadpool::StdinCompletion,
    pending: &mut PendingTable,
    buffer_pool: &mut BufferPool,
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
) -> Option<Completion> {
    let id = SubmissionId::from_raw(sc.id);
    let pending_op = match pending.take(id) {
        Taken::Live(op) => op,
        // The stdin worker reports no descriptor, so there is none to close.
        Taken::Cancelled(op) => {
            op.retire(0, buffer_pool);
            return None;
        }
        Taken::Unknown => return None,
    };
    // The stdin worker runs reads on a port and nothing else, so its completions
    // answer to one kind.
    if let Some(mismatch) = misrouted(&pending_op, OpKind::Port, id) {
        std::mem::forget(pending_op);
        return Some(Completion::err(
            id,
            crate::io::io_error("io-error", mismatch, origin_heap),
        ));
    }
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
    pending: &mut PendingTable,
    fd_states: &mut HashMap<PortKey, FdState>,
    buffer_pool: &mut BufferPool,
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
    gen: crate::segment::Generation,
) -> Option<Completion> {
    let id = SubmissionId::from_raw(pc.id);
    let mut pending_op = match pending.take(id) {
        Taken::Live(op) => op,
        Taken::Cancelled(op) => {
            op.retire(pc.result_code, buffer_pool);
            return None;
        }
        Taken::Unknown => return None,
    };
    if let Some(mismatch) = misrouted(&pending_op, pc.kind, id) {
        // The entry filed under this id is not the operation that finished, so
        // nothing it holds can be trusted to be what it claims. The entry is let
        // go unread rather than retired: retiring reclaims exactly the payload
        // in question — a `Box<siginfo_t>`, a descriptor, a pooled buffer — and
        // that is the free this check exists to prevent. Leaking those is the
        // cheap half of the trade.
        std::mem::forget(pending_op);
        return Some(Completion::err(
            id,
            crate::io::io_error("io-error", mismatch, origin_heap),
        ));
    }
    if let PendingOp::Connect {
        ref mut connect_fd, ..
    } = pending_op
    {
        if pc.result_code > 0 {
            *connect_fd = Some(pc.result_code);
        }
    }

    // A pool worker's bytes stay in `pc.data`, and `assemble_read` reads them
    // there. Staging them into the fiber's buffer first would clamp them to its
    // size, and that size is not a bound on what the worker read: `read_until`
    // runs to the newline and `read_exact` to its cluster count, each returning
    // however many bytes that took. Bytes dropped by such a clamp are bytes the
    // port has already taken from the kernel, so nothing is left to read them
    // again.
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
