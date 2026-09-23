//! audited: 2026-09-23
//! Cooking a raw worker completion into one a fiber can be handed.
//!
//! src/io/AGENTS.md
//! docs/impl/io-inflight.md

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
        RawCompletion::Stdin(sc) => {
            stdin_to_completion(sc, pending, fd_states, buffer_pool, origin_heap, gen)
        }
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
    fd_states: &mut HashMap<PortKey, FdState>,
    buffer_pool: &mut BufferPool,
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
    gen: crate::segment::Generation,
) -> Option<Completion> {
    let id = SubmissionId::from_raw(sc.id);
    let pending_op = match pending.take(id) {
        Taken::Live(op) => op,
        // The stdin worker reports no descriptor, so there is none to close.
        Taken::Cancelled(op) => {
            op.retire(0, buffer_pool);
            return None;
        }
        // Nobody is left to read a result from, but the scheduler still holds
        // this id against the fiber that asked; answer so it can let go.
        Taken::Orphaned(mut op) => {
            crate::io::landing::give_back_lent(&mut op, fd_states);
            op.retire(0, buffer_pool);
            return Some(PendingTable::orphaned_asker_error(id, origin_heap));
        }
        Taken::Unknown => return None,
    };
    // The stdin worker runs reads on a port and nothing else, so its completions
    // answer to one kind.
    if let Some(mismatch) = misrouted(&pending_op, OpKind::Port, id) {
        std::mem::forget(pending_op);
        let birth = crate::io::Birthplace::on(origin_heap);
        return Some(Completion::failed(id, birth, "io-error", mismatch));
    }
    // Release BufferPool handle if present
    if let Some(bh) = pending_op.buffer_handle() {
        buffer_pool.release(bh);
    }
    Some(match sc.result {
        Ok(data) if data.is_empty() => {
            Completion::ok(id, crate::io::Birthplace::on(origin_heap), Value::NIL)
        }
        // This worker reads into a buffer of its own and hands every byte over
        // in `data`, so it put nothing in the caller's buffer: the result code
        // is zero. A line has no upper bound, and the completion answers from
        // the caller's buffer when the bytes fit and from the requesting
        // instance's heap when they do not, instead of dropping the excess
        // (docs/impl/io-bytes.md). The buffer handle is released above, so
        // none is passed here.
        Ok(data) => completion::process_raw_completion(
            id,
            0,
            data,
            &pending_op,
            fd_states,
            buffer_pool,
            None,
            origin_heap,
            gen,
        ),
        Err(e) => {
            let birth = crate::io::Birthplace::on(origin_heap);
            Completion::failed(id, birth, "io-error", e)
        }
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
        // As above: give back a borrowed remainder, retire the entry unread,
        // and answer so the scheduler can retire the pairing it still holds
        // under this id.
        Taken::Orphaned(mut op) => {
            crate::io::landing::give_back_lent(&mut op, fd_states);
            op.retire(pc.result_code, buffer_pool);
            return Some(PendingTable::orphaned_asker_error(id, origin_heap));
        }
        Taken::Unknown => return None,
    };
    if let Some(mismatch) = misrouted(&pending_op, pc.kind, id) {
        // The entry filed under this id is not the operation that finished, so
        // nothing it holds can be trusted to be what it claims. The entry is let
        // go unread rather than retired: retiring reclaims exactly the payload
        // in question — a `Box<siginfo_t>`, a descriptor, a pooled buffer — and
        // that is the free this check exists to prevent. The operand hold goes
        // the same way, so the regions it named stay retained too. Leaking all
        // of it is the cheap half of the trade.
        std::mem::forget(pending_op);
        let birth = crate::io::Birthplace::on(origin_heap);
        return Some(Completion::failed(id, birth, "io-error", mismatch));
    }
    if let PendingOp::Connect {
        ref mut connect_fd, ..
    } = &mut *pending_op
    {
        if pc.result_code > 0 {
            *connect_fd = Some(pc.result_code);
        }
    }

    // A read's result code counts the bytes its worker put in the caller's
    // buffer, and `pc.data` holds whatever it read anywhere else — past a full
    // buffer, or into a buffer of its own (docs/impl/io-bytes.md).
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
