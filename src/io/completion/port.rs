//! Completion handling for the PendingOp::Port (stream/socket I/O) case.

use super::*;

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
                // ECANCELED is io_uring cancelling an op whose linked timeout
                // fired; ETIMEDOUT is a thread-pool worker whose own bounded
                // wait expired. Both are the caller's `:timeout` elapsing, so
                // they carry the same error kind.
                let is_timeout = errno == libc::ECANCELED || errno == libc::ETIMEDOUT;
                let msg = if is_timeout {
                    "I/O operation timed out".to_string()
                } else {
                    format!("I/O error: {}", errno_message(errno))
                };
                let error_type = if is_timeout { "timeout" } else { "io-error" };
                let state = fd_states
                    .entry(port_key.clone())
                    .or_insert_with(FdState::new);
                state.status = FdStatus::Error;
                return Completion::err(id, crate::io::io_error(error_type, msg, origin_heap));
            }

            if result_code == 0
                && matches!(
                    op,
                    PortOp::ReadLine { .. }
                        | PortOp::Read { .. }
                        | PortOp::ReadExact { .. }
                        | PortOp::ReadAll
                )
            {
                // EOF for read operations
                let state = fd_states
                    .entry(port_key.clone())
                    .or_insert_with(FdState::new);
                state.status = FdStatus::Eof;

                // For ReadLine: check buffer for a partial last line
                // (file content without trailing newline).
                if let PortOp::ReadLine { ref buffer } = op {
                    // Check fd_state buffer first (leftover from previous calls)
                    if !state.buffer.is_empty() {
                        let remainder: Vec<u8> = state.buffer.drain(..).collect();
                        unsafe {
                            let (dst, dst_cap) = crate::io::request::writeable_buffer_ptr(buffer);
                            let copy_len = remainder.len().min(dst_cap);
                            std::ptr::copy_nonoverlapping(remainder.as_ptr(), dst, copy_len);
                            // Trim trailing \r\n
                            let trimmed = if copy_len > 0 && remainder[copy_len - 1] == b'\n' {
                                let mut end = copy_len - 1;
                                if end > 0 && remainder[end - 1] == b'\r' {
                                    end -= 1;
                                }
                                end
                            } else {
                                copy_len
                            };
                            crate::io::request::truncate_buffer(buffer, trimmed);
                        }
                        return Completion::new(id, unsafe {
                            crate::io::request::bytes_to_string_in_place(*buffer, origin_heap)
                        });
                    }
                    // Check if fiber buffer has data from a previous short read
                    let filled = pending.filled();
                    if filled > 0 {
                        // Trim trailing \r\n from fiber buffer data
                        let buf_bytes = buffer.as_bytes().unwrap();
                        let mut end = filled.min(buf_bytes.len());
                        if end > 0 && buf_bytes[end - 1] == b'\n' {
                            end -= 1;
                            if end > 0 && buf_bytes[end - 1] == b'\r' {
                                end -= 1;
                            }
                        }
                        unsafe {
                            crate::io::request::truncate_buffer(buffer, end);
                        }
                        return Completion::new(id, unsafe {
                            crate::io::request::bytes_to_string_in_place(*buffer, origin_heap)
                        });
                    }
                    // No data at all — EOF on first read
                    return Completion::ok(id, Value::NIL);
                }

                // For Read: return accumulated data on EOF.
                if let PortOp::Read { ref buffer, .. } = op {
                    // Check fd_state buffer first
                    if !state.buffer.is_empty() {
                        let partial: Vec<u8> = state.buffer.drain(..).collect();
                        unsafe {
                            let (dst, dst_cap) = crate::io::request::writeable_buffer_ptr(buffer);
                            let copy_len = partial.len().min(dst_cap);
                            std::ptr::copy_nonoverlapping(partial.as_ptr(), dst, copy_len);
                            crate::io::request::truncate_buffer(buffer, copy_len);
                        }
                        return Completion::new(
                            id,
                            if encoding == Encoding::Text {
                                unsafe {
                                    crate::io::request::bytes_to_string_in_place(
                                        *buffer,
                                        origin_heap,
                                    )
                                }
                            } else {
                                Ok(*buffer)
                            },
                        );
                    }
                    // Check fiber buffer
                    let filled = pending.filled();
                    if filled > 0 {
                        unsafe {
                            crate::io::request::truncate_buffer(buffer, filled);
                        }
                        return Completion::new(
                            id,
                            if encoding == Encoding::Text {
                                unsafe {
                                    crate::io::request::bytes_to_string_in_place(
                                        *buffer,
                                        origin_heap,
                                    )
                                }
                            } else {
                                Ok(*buffer)
                            },
                        );
                    }
                    // No data and EOF — return nil (empty read)
                    return Completion::ok(id, Value::NIL);
                }

                // For ReadExact: EOF before the full count is a failed
                // read — discard whatever partial sat in the buffer and
                // return nil so the caller can distinguish "got n bytes"
                // from "stream ended early".  Callers who want the
                // partial should use Read.
                if matches!(op, PortOp::ReadExact { .. }) {
                    state.buffer.clear();
                    return Completion::ok(id, Value::NIL);
                }

                // For ReadAll: return accumulated buffer on EOF
                // (empty bytes for empty files, not nil).
                if matches!(op, PortOp::ReadAll) {
                    let all: Vec<u8> = state.buffer.drain(..).collect();
                    let heap = unsafe { &mut *crate::io::completion_heap_ptr(origin_heap) };
                    let ctx = crate::primitives::ctx::Alloc::new(heap);
                    let val = ctx.bytes(all);
                    return Completion::new(
                        id,
                        if encoding == Encoding::Text {
                            unsafe {
                                crate::io::request::bytes_to_string_in_place(val, origin_heap)
                            }
                        } else {
                            Ok(val)
                        },
                    );
                }

                return Completion::ok(id, Value::NIL);
            }

            // Success
            let value = match op {
                PortOp::ReadLine { ref buffer } => {
                    // The data is in the fiber's pre-allocated buffer (for both
                    // io_uring and thread pool paths). Total valid bytes:
                    let total = pending.filled() + result_code as usize;

                    // Also check if fd_state has leftover bytes from a previous
                    // over-read. If so, prepend them.
                    let state = fd_states
                        .entry(port_key.clone())
                        .or_insert_with(FdState::new);

                    if !state.buffer.is_empty() {
                        // Copy leftover bytes into the fiber buffer, shifting
                        // the kernel data to make room.
                        let leftover = state.buffer.len();
                        unsafe {
                            let (dst, dst_cap) = crate::io::request::writeable_buffer_ptr(buffer);
                            // Shift existing data right to make room for leftover
                            if total > leftover {
                                std::ptr::copy(
                                    dst.add(0),
                                    dst.add(leftover),
                                    total.min(dst_cap) - leftover,
                                );
                            }
                            std::ptr::copy_nonoverlapping(
                                state.buffer.as_ptr(),
                                dst,
                                leftover.min(dst_cap),
                            );
                        }
                        state.buffer.clear();
                    }

                    // Read the fiber buffer content to find the line boundary
                    let buf_bytes = buffer.as_bytes().unwrap();
                    let scan_len = total.min(buf_bytes.len());

                    // Find newline in the fiber buffer
                    let newline_pos = buf_bytes[..scan_len].iter().position(|&b| b == b'\n');

                    let final_len = if let Some(pos) = newline_pos {
                        // Store any bytes after the newline in state.buffer
                        // for the next ReadLine call.
                        if pos + 1 < scan_len {
                            state
                                .buffer
                                .extend_from_slice(&buf_bytes[pos + 1..scan_len]);
                        }
                        // Trim trailing \r\n
                        let mut end = pos;
                        if end > 0 && buf_bytes[end - 1] == b'\r' {
                            end -= 1;
                        }
                        end
                    } else {
                        // No newline found — return all data as partial line
                        let mut end = scan_len;
                        if end > 0 && buf_bytes[end - 1] == b'\n' {
                            end -= 1;
                            if end > 0 && buf_bytes[end - 1] == b'\r' {
                                end -= 1;
                            }
                        }
                        end
                    };

                    unsafe {
                        crate::io::request::truncate_buffer(buffer, final_len);
                    }
                    // Transmute LBytes → LString in place (zero-copy, validates UTF-8)
                    return Completion::new(id, unsafe {
                        crate::io::request::bytes_to_string_in_place(*buffer, origin_heap)
                    });
                }
                PortOp::Read { ref buffer, .. } | PortOp::ReadExact { ref buffer, .. } => {
                    // Prepend any bytes left in the fd_state buffer from a
                    // previous over-read (e.g. ReadLine read past the line
                    // boundary, or a previous short-read for this op).  The
                    // submit/resubmit path reduced the kernel read count by
                    // what was buffered.
                    let state = fd_states
                        .entry(port_key.clone())
                        .or_insert_with(FdState::new);

                    let total = if !state.buffer.is_empty() {
                        let buffered = state.buffer.len();
                        // Copy buffered bytes to the start of the fiber buffer,
                        // then shift kernel data after them.
                        unsafe {
                            let (dst, _) = crate::io::request::writeable_buffer_ptr(buffer);
                            let kernel_data_start = pending.filled();
                            let kernel_data_len = result_code as usize;
                            // Shift kernel data right by `buffered` positions
                            std::ptr::copy(
                                dst,
                                dst.add(buffered),
                                (kernel_data_start + kernel_data_len)
                                    .min(buffer.as_bytes().unwrap().len()),
                            );
                            std::ptr::copy_nonoverlapping(state.buffer.as_ptr(), dst, buffered);
                        }
                        state.buffer.clear();
                        buffered + pending.filled() + result_code as usize
                    } else {
                        // Data is already in the fiber's buffer (io_uring: kernel
                        // wrote it; thread pool: pool_to_completion copied it).
                        pending.filled() + result_code as usize
                    };

                    unsafe {
                        crate::io::request::truncate_buffer(buffer, total);
                    }
                    // ReadExact is all-or-nothing: a stream that ends before the
                    // full count yields nil, not the partial. The io_uring path
                    // reaches this as a 0-length EOF completion (handled in the
                    // result_code == 0 arm above); the pool worker instead loops
                    // internally and hands back the short partial with a positive
                    // count, so detect "short" here — in the port's own unit — and
                    // map it to nil too, discarding the partial exactly as the EOF
                    // arm does.
                    if let PortOp::ReadExact { count, .. } = op {
                        let enough = match encoding {
                            Encoding::Text => {
                                let bytes = buffer.as_bytes().unwrap_or(&[]);
                                crate::io::grapheme_count_in_valid_prefix(bytes, gen) >= *count
                            }
                            Encoding::Binary => total >= *count,
                        };
                        if !enough {
                            state.buffer.clear();
                            return Completion::ok(id, Value::NIL);
                        }
                    }
                    if encoding == Encoding::Text {
                        // ReadExact on a text port is grapheme-counted: return
                        // exactly `count` grapheme clusters and stash the
                        // trailing bytes (extra graphemes the resubmit gate
                        // over-read, or a partial trailing grapheme) in the
                        // fd_state buffer for the next read.  Plain Read and
                        // ReadAll keep the full buffer.  `bytes_to_string_in_place`
                        // is retained so the result lands in the buffer's region
                        // (s11 region routing) rather than a fresh allocation.
                        if let PortOp::ReadExact { count, .. } = op {
                            let (end, leftover) = {
                                let bytes = buffer.as_bytes().unwrap_or(&[]);
                                let end = crate::io::nth_grapheme_byte_end(bytes, *count, gen)
                                    .unwrap_or(bytes.len());
                                (end, bytes[end..].to_vec())
                            };
                            if !leftover.is_empty() {
                                state.buffer.extend_from_slice(&leftover);
                            }
                            unsafe {
                                crate::io::request::truncate_buffer(buffer, end);
                            }
                        }
                        return Completion::new(id, unsafe {
                            crate::io::request::bytes_to_string_in_place(*buffer, origin_heap)
                        });
                    }
                    *buffer
                }
                PortOp::ReadAll => {
                    // ReadAll still uses the existing fd_states.buffer accumulation.
                    // Accumulated in fd_states.buffer by re-submission loop.
                    let state = fd_states
                        .entry(port_key.clone())
                        .or_insert_with(FdState::new);
                    state.buffer.extend_from_slice(&data);
                    let all: Vec<u8> = state.buffer.drain(..).collect();
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
