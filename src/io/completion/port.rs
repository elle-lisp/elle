//! Completion handling for the PendingOp::Port (stream/socket I/O) case.

use super::*;
use crate::io::frame::{exact_end, line_end, read_result};

/// Every byte a finishing read owns, in stream order: the remainder a previous
/// read on this port left behind, then the bytes this read produced.
///
/// The two backends deliver those bytes differently, and `data` says which. A
/// pool worker hands its bytes back there. io_uring writes them into the fiber's
/// own buffer and leaves `data` empty, so `filled + result_code` of that buffer
/// is what this operation added.
fn assemble_read(
    pending: &PendingOp,
    buffer: &Value,
    state: &mut FdState,
    data: &[u8],
    result_code: i32,
) -> Vec<u8> {
    let mut all: Vec<u8> = std::mem::take(&mut state.buffer);
    if data.is_empty() {
        let bytes = buffer.as_bytes().unwrap_or(&[]);
        let end = (pending.filled() + result_code.max(0) as usize).min(bytes.len());
        all.extend_from_slice(&bytes[..end]);
    } else {
        all.extend_from_slice(data);
    }
    all
}

pub(super) fn complete_port_op(
    id: SubmissionId,
    result_code: i32,
    data: Vec<u8>,
    pending: &PendingOp,
    fd_states: &mut HashMap<PortKey, FdState>,
    // The requesting instance's heap; result values are born on it
    // (`crate::io::completion_heap_ptr`).
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
    // The owning VM's Unicode generation; text ReadExact splits the byte
    // stream at its cluster boundaries and stashes the remainder.
    gen: crate::segment::Generation,
) -> Completion {
    match pending {
        PendingOp::Port {
            op,
            port_key,
            port,
            listener_kind,
            ..
        } => {
            let encoding = port
                .as_external::<Port>()
                .map(|p| p.encoding())
                .unwrap_or(Encoding::Binary);
            if result_code < 0 {
                // Error
                let errno = -result_code;
                let is_timeout = is_timeout_errno(errno);
                let msg = if is_timeout {
                    "I/O operation timed out".to_string()
                } else {
                    format!("I/O error: {}", errno_message(errno))
                };
                let error_type = if is_timeout { "timeout" } else { "io-error" };
                return Completion::err(id, crate::io::io_error(error_type, msg, origin_heap));
            }

            // The three buffer-backed reads answer the same way whether the
            // stream ended (`result_code == 0`) or delivered bytes: assemble
            // everything this operation owns, take the part that answers the
            // request, and give the rest back to the port. Only `read-all`
            // needs the end of the stream told apart, because that is what it
            // waits for.
            if matches!(
                op,
                PortOp::ReadLine { .. } | PortOp::Read { .. } | PortOp::ReadExact { .. }
            ) {
                let state = crate::io::types::fd_state_mut(fd_states, port_key);
                let buffer = match op {
                    PortOp::ReadLine { buffer }
                    | PortOp::Read { buffer, .. }
                    | PortOp::ReadExact { buffer, .. } => buffer,
                    _ => unreachable!(),
                };
                let mut all = assemble_read(pending, buffer, state, &data, result_code);

                // How much of `all` answers the request, in the port's own unit,
                // and where the port's remainder starts. `None` is a request the
                // stream cannot answer.
                let split = match op {
                    PortOp::ReadLine { .. } => {
                        if all.is_empty() {
                            None
                        } else {
                            Some(line_end(&all))
                        }
                    }
                    // `port/read` answers with up to `count` bytes — whatever
                    // arrived — so only an empty stream leaves it nothing to say.
                    PortOp::Read { count, .. } => {
                        if all.is_empty() {
                            None
                        } else {
                            let end = all.len().min(*count);
                            Some((end, end))
                        }
                    }
                    // `read-exact` is all-or-nothing: a stream that ended before
                    // the count yields nil, and the partial goes with it, so a
                    // caller can tell "got n" from "ended early".
                    PortOp::ReadExact { count, .. } => {
                        exact_end(&all, *count, encoding, gen).map(|end| (end, end))
                    }
                    _ => unreachable!(),
                };
                let Some((end, rest)) = split else {
                    return Completion::ok(id, Value::NIL);
                };
                if rest < all.len() {
                    state.buffer.extend_from_slice(&all[rest..]);
                }
                all.truncate(end);
                // A line is text whatever the port is measured in.
                let as_text = if matches!(op, PortOp::ReadLine { .. }) {
                    Encoding::Text
                } else {
                    encoding
                };
                return Completion::new(id, read_result(buffer, all, as_text, origin_heap));
            }

            if result_code == 0 && matches!(op, PortOp::ReadAll) {
                // ReadAll returns its accumulated buffer at EOF — empty bytes for
                // an empty file, not nil.
                let state = crate::io::types::fd_state_mut(fd_states, port_key);
                let all: Vec<u8> = std::mem::take(&mut state.buffer);
                let heap = unsafe { &mut *crate::io::completion_heap_ptr(origin_heap) };
                let ctx = crate::primitives::ctx::Alloc::new(heap);
                let val = ctx.bytes(all);
                return Completion::new(
                    id,
                    if encoding == Encoding::Text {
                        unsafe { crate::io::request::bytes_to_string_in_place(val, origin_heap) }
                    } else {
                        Ok(val)
                    },
                );
            }

            // Everything below completes the same way at any non-negative
            // result: a write reports the bytes it moved, an accept its
            // descriptor, and a zero from any of them is a count rather than an
            // end of stream. Only the reads answered above have an EOF to tell
            // apart, and each of them told it.
            let value = match op {
                // Claimed by the assembled-read path above.
                PortOp::ReadLine { .. } | PortOp::Read { .. } | PortOp::ReadExact { .. } => {
                    unreachable!("buffer-backed reads complete through assemble_read")
                }
                PortOp::ReadAll => {
                    // ReadAll still uses the existing fd_states.buffer accumulation.
                    // Accumulated in fd_states.buffer by re-submission loop.
                    let state = crate::io::types::fd_state_mut(fd_states, port_key);
                    state.buffer.extend_from_slice(&data);
                    let all: Vec<u8> = std::mem::take(&mut state.buffer);
                    let heap = unsafe { &mut *crate::io::completion_heap_ptr(origin_heap) };
                    let ctx = crate::primitives::ctx::Alloc::new(heap);
                    let val = ctx.bytes(all);
                    if encoding == Encoding::Text {
                        return Completion::new(id, unsafe {
                            crate::io::request::bytes_to_string_in_place(val, origin_heap)
                        });
                    }
                    val
                }
                // A write completes only when the whole payload is gone, so the
                // count is everything transferred across every resubmission —
                // the last CQE's bytes plus the offset it started from. The
                // io_uring path accumulates that offset in `filled`; the pool
                // worker loops internally and leaves it at zero.
                PortOp::Write { .. } => {
                    Value::int((pending.filled() + result_code as usize) as i64)
                }
                // A datagram send is atomic: the kernel takes all of it or none,
                // so there is no partial to accumulate.
                PortOp::SendTo { .. } => Value::int(result_code as i64),
                PortOp::Flush | PortOp::Shutdown { .. } => Value::NIL,
                PortOp::Accept {
                    ref options,
                    ref accept_port,
                    ..
                } => {
                    // Accept: result_code is the new fd (from both io_uring and thread pool).
                    // The accept_port was pre-allocated by the caller with the
                    // requested encoding already set (see prim_tcp_accept /
                    // prim_unix_accept). Set the fd on it and return.
                    let fd = result_code;
                    let _peer_addr = crate::io::sockaddr::peer_address(fd);
                    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
                    // Apply user-specified socket options to the accepted fd.
                    crate::io::request::apply_socket_options(fd.as_raw_fd(), options);
                    if let Some(PortKind::TcpListener) = listener_kind {
                        set_tcp_nodelay(&fd);
                    }
                    let port_ref = accept_port
                        .as_external::<Port>()
                        .expect("accept_port must be a Port");
                    port_ref.set_fd(fd);
                    *accept_port
                }
                PortOp::RecvFrom { result, .. } => {
                    // `data` is addr_len(4 LE) + sockaddr_storage, optionally
                    // followed by the payload (thread-pool path; the io_uring
                    // path received it zero-copy straight into `:data`).
                    // Fill the pre-allocated result struct in place and return
                    // it — no value is instantiated on this (the scheduler's)
                    // heap, so there is no cross-heap reference for the resumed
                    // fiber to dangle on.
                    use crate::io::request::{
                        bytes_to_string_in_place, set_struct_field_in_place, truncate_buffer,
                        writeable_buffer_ptr,
                    };
                    let sockaddr_size = std::mem::size_of::<libc::sockaddr_storage>();
                    let addr_offset = 4 + sockaddr_size;
                    if data.len() < addr_offset {
                        return Completion::err(
                            id,
                            crate::io::io_error("io-error", "invalid recvfrom data", origin_heap),
                        );
                    }
                    let addr_len =
                        u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as libc::socklen_t;
                    let addr_storage = unsafe {
                        let mut storage: libc::sockaddr_storage = std::mem::zeroed();
                        std::ptr::copy_nonoverlapping(
                            data[4..4 + sockaddr_size].as_ptr(),
                            &mut storage as *mut _ as *mut u8,
                            sockaddr_size,
                        );
                        storage
                    };
                    let (addr_str, port_num) = crate::io::sockaddr::parse(&addr_storage, addr_len);

                    let struct_ref = result.as_struct().expect("recv result must be a struct");
                    let data_buf = crate::value::sorted_struct_get(
                        struct_ref,
                        &TableKey::Keyword("data".into()),
                    )
                    .copied()
                    .expect("recv result must have :data");
                    let addr_buf = crate::value::sorted_struct_get(
                        struct_ref,
                        &TableKey::Keyword("addr".into()),
                    )
                    .copied()
                    .expect("recv result must have :addr");

                    unsafe {
                        // :data — payload already in the buffer on the io_uring
                        // path (truncate to result_code); copied in on the
                        // thread-pool path (payload appended after the sockaddr).
                        if data.len() > addr_offset {
                            let payload = &data[addr_offset..];
                            let (dst, cap) = writeable_buffer_ptr(&data_buf);
                            let n = payload.len().min(cap);
                            std::ptr::copy_nonoverlapping(payload.as_ptr(), dst, n);
                            truncate_buffer(&data_buf, n);
                        } else {
                            let (_, cap) = writeable_buffer_ptr(&data_buf);
                            truncate_buffer(&data_buf, (result_code as usize).min(cap));
                        }

                        // :addr — fill the pre-allocated buffer and transmute it
                        // to a string in place, then stamp it into the slot.
                        let abytes = addr_str.as_bytes();
                        let (dst, cap) = writeable_buffer_ptr(&addr_buf);
                        let n = abytes.len().min(cap);
                        std::ptr::copy_nonoverlapping(abytes.as_ptr(), dst, n);
                        truncate_buffer(&addr_buf, n);
                        let addr_val =
                            bytes_to_string_in_place(addr_buf, origin_heap).unwrap_or(addr_buf);
                        set_struct_field_in_place(
                            result,
                            &TableKey::Keyword("addr".into()),
                            addr_val,
                        );

                        // :port — stamp the sender port int into the slot.
                        set_struct_field_in_place(
                            result,
                            &TableKey::Keyword("port".into()),
                            Value::int(port_num as i64),
                        );
                    }
                    *result
                }
            };
            Completion::ok(id, value)
        }
        _ => unreachable!("complete_port_op: non-Port pending"),
    }
}
