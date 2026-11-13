//! audited: 2026-11-12
//! The byte-stream submissions — the three buffered reads, `read-all`, write
//! and flush — and where each one's bytes land.
//!
//! docs/impl/io/bytes.md
//! docs/impl/io/cancel.md

use super::*;
use crate::io::landing::{lend_remainder, Landing};
use crate::io::outgoing::Payload;
use crate::io::types::FdState;

impl AsyncBackend {
    /// Submit a byte-stream operation on an open port, and file its entry.
    ///
    /// A buffered read borrows the port's remainder into the front of the
    /// caller's buffer and lands behind it, on the ring and on a pool worker
    /// alike. A write hands over the address of each of its parts when they
    /// all lie in a region. A pool operation that carries no stop pipe addresses
    /// nothing of the caller's, because nothing could end it at teardown; it
    /// reads into and writes from buffers of its worker's own
    /// (docs/impl/io/bytes.md).
    ///
    /// `port_value` is the open port the caller found, `bound` the request's,
    /// and `submitter` who the operation is for. A write that waited in line
    /// starts here long after its own `submit`, so none of the three may be
    /// read off the submission being dispatched now (docs/impl/io/write.md).
    /// `copy` is the write's [`copy_at_submission`], taken when it was
    /// submitted; this copies nothing more of what the program can change.
    ///
    /// [`copy_at_submission`]: crate::io::outgoing::Outgoing::copy_at_submission
    pub(in crate::io::aio) fn submit_stream(
        inner: &mut AsyncBackendInner,
        op: &PortOp,
        id: SubmissionId,
        port_value: Value,
        bound: Bound,
        submitter: crate::io::pending::Submitter,
        copy: Option<Vec<u8>>,
    ) -> Result<SubmissionId, String> {
        let port = port_value
            .as_external::<Port>()
            .ok_or_else(|| "io/submit: request contains non-port value".to_string())?;
        let port_key = PortKey::from_port(port);
        let fd = port_key.raw_fd();
        let gen = inner.unicode_generation;
        let text = matches!(port.encoding(), Encoding::Text);
        // The operation's own share of the descriptor, held until its entry is
        // retired. `fd` is resolved again when the worker runs, so the number
        // must stay this port's for as long as the operation names it
        // (docs/impl/io/descriptor.md).
        let descriptor = port.fd_share();
        let AsyncBackendInner {
            ref mut platform,
            ref mut hub,
            ref mut buffer_pool,
            ref mut pending,
            ref mut fd_states,
            ..
        } = *inner;
        let state = crate::io::types::fd_state_mut(fd_states, &port_key);

        let (lent, buf_handle) = match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => match op {
                PortOp::ReadLine { buffer }
                | PortOp::Read { buffer, .. }
                | PortOp::ReadExact { buffer, .. } => {
                    // SAFETY: the buffer is this read's own, and its fiber is
                    // parked in the call that submits it.
                    let lent = unsafe { lend_remainder(state, buffer) };
                    let submitted = crate::io::uring::submit_uring_read(
                        ring,
                        id,
                        fd,
                        op,
                        lent.len(),
                        bound.next_wait(),
                    );
                    if let Err(e) = submitted {
                        restore_remainder(state, lent);
                        return Err(e);
                    }
                    (lent, None)
                }
                PortOp::ReadAll => {
                    let bh = buffer_pool.alloc(0);
                    buffer_pool.get_mut(bh).reserve(
                        crate::io::landing::rest_of_file(fd).unwrap_or(0)
                            + crate::io::landing::READ_ALL_CHUNK,
                    );
                    crate::io::uring::submit_uring_stream(
                        ring,
                        id,
                        fd,
                        op,
                        bound.next_wait(),
                        buffer_pool,
                        Some(bh),
                    )?;
                    (Vec::new(), Some(bh))
                }
                PortOp::Write { data } => {
                    // The copy of bytes that must be copied goes into a pooled
                    // buffer; a gathered write of several parts lays out its
                    // iovecs there.
                    let bh =
                        crate::io::uring::write_needs_buffer(data).then(|| buffer_pool.alloc(0));
                    crate::io::uring::keep_copy(copy, bh, buffer_pool);
                    crate::io::uring::submit_uring_stream(
                        ring,
                        id,
                        fd,
                        op,
                        bound.next_wait(),
                        buffer_pool,
                        bh,
                    )?;
                    (Vec::new(), bh)
                }
                PortOp::Flush => {
                    let bh = buffer_pool.alloc(0);
                    crate::io::uring::submit_uring_stream(
                        ring,
                        id,
                        fd,
                        op,
                        bound.next_wait(),
                        buffer_pool,
                        Some(bh),
                    )?;
                    (Vec::new(), Some(bh))
                }
                PortOp::Accept { .. }
                | PortOp::SendTo { .. }
                | PortOp::RecvFrom { .. }
                | PortOp::Shutdown { .. } => unreachable!("submit_stream: socket op"),
            },
            PlatformBackend::ThreadPool => {
                let _ = buffer_pool;
                // A read takes a stop pipe: `io/cancel` must end it rather
                // than abandon it, or the abandoned read goes on consuming
                // bytes meant for whoever reads the port next.
                //
                // A write takes one for the peer it waits on. A write runs to
                // the end of its payload, and a payload past the send buffer
                // only gets there as the peer takes what is already in it. So
                // a peer that stops reading parks the write with nothing else
                // able to end it.
                //
                // `Flush` waits on nobody: `fsync(2)` transfers what this
                // process already handed the kernel.
                let bounds = match op {
                    PortOp::Flush => Bounds::prompt(),
                    _ => hub.bounds(id, bound),
                };
                let reach = bounds.can_stop();
                let (pool_op, lent) = match op {
                    PortOp::ReadLine { buffer } => {
                        let (landing, lent) = unsafe { land(state, buffer, reach) };
                        (PoolOp::ReadLine { fd, landing }, lent)
                    }
                    PortOp::Read { buffer, .. } => {
                        let (landing, lent) = unsafe { land(state, buffer, reach) };
                        (PoolOp::Read { fd, landing }, lent)
                    }
                    PortOp::ReadExact { count, buffer } => {
                        let (landing, lent) = unsafe { land(state, buffer, reach) };
                        // A remainder the read did not borrow still counts
                        // toward its count, so the worker is handed a copy.
                        let held = if lent.is_empty() {
                            state.buffer.clone()
                        } else {
                            Vec::new()
                        };
                        let pool_op = PoolOp::ReadExact {
                            fd,
                            landing,
                            count: *count,
                            graphemes: text,
                            gen,
                            held,
                        };
                        (pool_op, lent)
                    }
                    PortOp::ReadAll => (PoolOp::ReadAll { fd }, Vec::new()),
                    PortOp::Write { data } => {
                        // A worker that no stop pipe can reach writes from a
                        // copy of its own, even of bytes in region pages
                        // (docs/impl/io/bytes.md).
                        let payload = match (data.in_region(), copy) {
                            (_, Some(copy)) => Payload::Owned(copy),
                            (Some(parts), None) if reach => Payload::addressed(&parts),
                            (_, None) => Payload::Owned(data.copied()),
                        };
                        (PoolOp::Write { fd, payload }, Vec::new())
                    }
                    PortOp::Flush => (PoolOp::Flush { fd }, Vec::new()),
                    PortOp::Accept { .. }
                    | PortOp::SendTo { .. }
                    | PortOp::RecvFrom { .. }
                    | PortOp::Shutdown { .. } => unreachable!("submit_stream: socket op"),
                };
                if let Err(e) = hub.submit(id, pool_op, bounds) {
                    restore_remainder(state, lent);
                    return Err(e);
                }
                // The worker owns every buffer it reads into or writes from
                // that is not the caller's, so the pool keeps none.
                (lent, None)
            }
        };

        pending.insert(
            id,
            PendingOp::port(
                op.clone(),
                port_key,
                port_value,
                descriptor,
                buf_handle,
                bound,
            )
            .lending(lent),
            submitter,
        );
        Ok(id)
    }
}

/// Where a pool read lands. A worker that can be stopped reads into the
/// caller's buffer, behind the port's remainder it borrows; one that cannot
/// reads into a buffer of its own the same size, and the port keeps its
/// remainder for the completion to join.
///
/// # Safety
///
/// `buffer` is the read's own pre-allocated `LBytes`, and its fiber is parked.
unsafe fn land(state: &mut FdState, buffer: &Value, reach: bool) -> (Landing, Vec<u8>) {
    if reach {
        let lent = lend_remainder(state, buffer);
        (Landing::in_buffer(buffer, lent.len()), lent)
    } else {
        let cap = buffer.as_bytes().map_or(0, <[u8]>::len);
        (Landing::owned(cap), Vec::new())
    }
}

/// Put a borrowed remainder back in the port whose read never went out. The
/// port's remainder was empty the moment it was lent, so nothing else can be
/// in it yet.
fn restore_remainder(state: &mut FdState, lent: Vec<u8>) {
    if !lent.is_empty() {
        state.buffer = lent;
    }
}
