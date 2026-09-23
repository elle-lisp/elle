//! audited: 2026-09-23
//! Completion handling for the PendingOp::Port (stream/socket I/O) case.
//!
//! src/io/AGENTS.md
//! docs/impl/io-bytes.md

use super::*;
use crate::io::frame::{answer_from, answer_in_buffer, span, Ask};

/// Cook a port operation's completion.
///
/// For the three buffered reads, `filled + result_code` bytes of the caller's
/// buffer are what the operation landed there — a borrowed remainder included —
/// and `data` is whatever it produced anywhere else, which follows those bytes
/// in the stream. The ring and a pool worker that reads into the caller's
/// buffer leave `data` empty; a worker that read into a buffer of its own, and
/// the stdin worker, hand everything over in `data`.
pub(super) fn complete_port_op(
    id: SubmissionId,
    result_code: i32,
    data: Vec<u8>,
    pending: &PendingOp,
    fd_states: &mut HashMap<PortKey, FdState>,
    // Where this completion builds a result it cannot be handed — a read whose
    // bytes outgrew the caller's buffer, a `read-all`, and every error
    // (docs/impl/io-inflight.md).
    mut birth: crate::io::Birthplace,
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
            lent,
            ..
        } => {
            let encoding = port
                .as_external::<Port>()
                .map(|p| p.encoding())
                .unwrap_or(Encoding::Binary);
            if result_code < 0 {
                // A read that failed took nothing from the stream, so the
                // remainder it borrowed goes back to the port for the next one.
                // A copy, because the entry is only lent here; the original
                // goes with it.
                crate::io::landing::give_back(fd_states, port_key, port, lent.clone());
                let errno = -result_code;
                let is_timeout = is_timeout_errno(errno);
                let msg = if is_timeout {
                    "I/O operation timed out".to_string()
                } else {
                    format!("I/O error: {}", errno_message(errno))
                };
                let error_type = if is_timeout { "timeout" } else { "io-error" };
                return Completion::failed(id, birth, error_type, msg);
            }

            // The three buffered reads answer the same way whether the stream
            // ended or delivered bytes: take the part of what they own that
            // answers the request, and give the rest back to the port. Only
            // `read-all` needs the end of the stream told apart, because that is
            // what it waits for.
            if let Some(ask) = Ask::of(op) {
                let buffer = match op {
                    PortOp::ReadLine { buffer }
                    | PortOp::Read { buffer, .. }
                    | PortOp::ReadExact { buffer, .. } => buffer,
                    _ => unreachable!(),
                };
                let as_text = ask.answer_encoding(encoding);
                let state = crate::io::types::fd_state_mut(fd_states, port_key);
                let landed = buffer.as_bytes().unwrap_or(&[]);
                let landed = &landed[..(pending.filled() + result_code as usize).min(landed.len())];

                // Everything the read owns lies in the buffer, so the answer is
                // cut where it lies and nothing moves but the bytes past it.
                if state.buffer.is_empty() && data.is_empty() {
                    let Some((end, rest)) = span(ask, landed, encoding, gen) else {
                        return Completion::ok(id, birth, Value::NIL);
                    };
                    if rest < landed.len() {
                        state.buffer.extend_from_slice(&landed[rest..]);
                    }
                    let result = answer_in_buffer(buffer, end, as_text, &mut birth);
                    return Completion::new(id, birth, result);
                }

                // Some of what the read owns lies outside the buffer — a
                // remainder too large to lend, bytes spilled from a full
                // buffer, or everything a worker read into a buffer of its own
                // — so the answer is cut from the join, in stream order.
                let mut all = std::mem::take(&mut state.buffer);
                all.extend_from_slice(landed);
                all.extend_from_slice(&data);
                let Some((end, rest)) = span(ask, &all, encoding, gen) else {
                    return Completion::ok(id, birth, Value::NIL);
                };
                if rest < all.len() {
                    state.buffer.extend_from_slice(&all[rest..]);
                }
                let result = answer_from(buffer, &all[..end], as_text, &mut birth);
                return Completion::new(id, birth, result);
            }

            if matches!(op, PortOp::ReadAll) {
                // The whole stream, the port's remainder first, copied once into
                // the answer's region — empty bytes for an empty file, not nil
                // (docs/impl/io-bytes.md § "`read-all` copies once").
                let state = crate::io::types::fd_state_mut(fd_states, port_key);
                let held = std::mem::take(&mut state.buffer);
                let val = birth.alloc().joined_bytes(&[&held, &data]);
                let result = if encoding == Encoding::Text {
                    unsafe { crate::io::request::bytes_to_string_in_place(val, &mut birth) }
                } else {
                    Ok(val)
                };
                return Completion::new(id, birth, result);
            }

            // Everything below completes the same way at any non-negative
            // result: a write reports the bytes it moved, an accept its
            // descriptor, and a zero from any of them is a count rather than an
            // end of stream. Only the reads answered above have an EOF to tell
            // apart, and each of them told it.
            let value = match op {
                // Claimed by the two read paths above.
                PortOp::ReadLine { .. }
                | PortOp::Read { .. }
                | PortOp::ReadExact { .. }
                | PortOp::ReadAll => {
                    unreachable!("the reads complete above")
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
                        return Completion::failed(id, birth, "io-error", "invalid recvfrom data");
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
                    let data_buf =
                        crate::value::sorted_struct_get(struct_ref, &TableKey::keyword("data"))
                            .copied()
                            .expect("recv result must have :data");
                    let addr_buf =
                        crate::value::sorted_struct_get(struct_ref, &TableKey::keyword("addr"))
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
                        // The error arm builds a value nobody reads, and it is
                        // released with everything else this birthplace coined.
                        // What is stamped into the caller's struct is that
                        // caller's own buffer re-tagged — never a value born
                        // here, which the handover would free under the struct.
                        let addr_val =
                            bytes_to_string_in_place(addr_buf, &mut birth).unwrap_or(addr_buf);
                        set_struct_field_in_place(result, &TableKey::keyword("addr"), addr_val);

                        // :port — stamp the sender port int into the slot.
                        set_struct_field_in_place(
                            result,
                            &TableKey::keyword("port"),
                            Value::int(port_num as i64),
                        );
                    }
                    *result
                }
            };
            Completion::ok(id, birth, value)
        }
        _ => unreachable!("complete_port_op: non-Port pending"),
    }
}
