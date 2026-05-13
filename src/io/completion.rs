//! Completion processing for async I/O operations.

use crate::io::pending::PendingOp;
use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::{ConnectAddr, IoOp};
use crate::io::types::{FdState, FdStatus, PortKey};
use crate::io::Completion;
use crate::port::{Port, PortKind};
use crate::value::heap::TableKey;
use crate::value::{error_val, Value};
use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::os::unix::io::{FromRawFd, OwnedFd, RawFd};

/// Set TCP_NODELAY on a TCP stream fd to disable Nagle's algorithm.
fn set_tcp_nodelay(fd: &OwnedFd) {
    unsafe {
        let opt: libc::c_int = 1;
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::IPPROTO_TCP,
            libc::TCP_NODELAY,
            &opt as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
}

/// Convert an errno to a human-readable message via strerror.
fn errno_message(errno: i32) -> String {
    std::io::Error::from_raw_os_error(errno).to_string()
}

pub(super) fn process_raw_completion(
    id: u64,
    result_code: i32,
    data: Vec<u8>,
    pending: &PendingOp,
    fd_states: &mut HashMap<PortKey, FdState>,
    buffer_pool: &mut BufferPool,
    buf_handle: Option<BufferHandle>,
) -> Completion {
    // Release the buffer back to the pool (if present — reads don't use BufferPool)
    if let Some(bh) = buf_handle {
        buffer_pool.release(bh);
    }

    match pending {
        PendingOp::ProcessWait {
            handle_val,
            siginfo,
            ..
        } => {
            // buffer_pool.release is already called at the top of process_raw_completion.

            if result_code < 0 {
                // On the uring path, reclaim siginfo before returning.
                if !siginfo.is_null() {
                    unsafe { drop(Box::from_raw(*siginfo)) };
                }
                let errno = -result_code;
                return Completion {
                    id,
                    result: Err(error_val(
                        "exec-error",
                        format!("subprocess/wait: waitid failed: errno {}", errno),
                    )),
                };
            }

            let exit_code: i32 = if siginfo.is_null() {
                // Thread pool path: exit code is encoded as 4-byte LE int in data.
                if data.len() >= 4 {
                    i32::from_le_bytes(data[..4].try_into().unwrap())
                } else {
                    result_code
                }
            } else {
                // io_uring path: exit status is in siginfo_t filled by the kernel.
                // Reclaim the siginfo_t allocation.
                // SAFETY: `siginfo` was allocated via Box::into_raw in submit_process_wait.
                // This completion arm is the single exit point — the CQE fires exactly once
                // per SQE.
                let si = unsafe { Box::from_raw(*siginfo) };
                // si_code values for SIGCHLD:
                //   CLD_EXITED (1): si_status is exit code
                //   CLD_KILLED (2): si_status is signal number (return as negative)
                //   CLD_DUMPED (3): killed + core dump (return signal as negative)
                //
                // SAFETY: si is fully initialized (kernel wrote it on child exit;
                // result_code >= 0 confirms the waitid completed successfully).
                unsafe {
                    let si_code = si.si_code;
                    let si_status = si.si_status();
                    match si_code {
                        1 => si_status,      // CLD_EXITED: normal exit
                        2 | 3 => -si_status, // CLD_KILLED / CLD_DUMPED: negative signal number
                        _ => -1,             // unknown
                    }
                }
            };

            // Cache the exit code in the ProcessHandle.
            if let Some(handle) = handle_val.as_external::<crate::io::request::ProcessHandle>() {
                let mut state = handle.inner.borrow_mut();
                *state = crate::io::request::ProcessState::Exited(exit_code);
            }

            Completion {
                id,
                result: Ok(Value::int(exit_code as i64)),
            }
        }
        PendingOp::Sleep { .. } => {
            // Sleep completes with -ETIME (62) on io_uring, or 0 on thread pool.
            // Both are success for a timer.
            Completion {
                id,
                result: Ok(Value::NIL),
            }
        }
        PendingOp::Open {
            path,
            direction,
            encoding,
            ..
        } => {
            if result_code < 0 {
                let errno = -result_code;
                let is_timeout = errno == 125; // ECANCELED from linked timeout
                let msg = if is_timeout {
                    "I/O operation timed out".to_string()
                } else {
                    let os_err = std::io::Error::from_raw_os_error(errno);
                    format!("port/open: {}: {}", path, os_err)
                };
                let error_type = if is_timeout { "timeout" } else { "io-error" };
                return Completion {
                    id,
                    result: Err(error_val(error_type, msg)),
                };
            }
            // SAFETY: result_code is a valid fd returned by the kernel (>= 0).
            // No fallible operations between here and OwnedFd::from_raw_fd.
            let fd = unsafe { OwnedFd::from_raw_fd(result_code) };
            let port = Port::new_file(fd, *direction, *encoding, path.clone());
            Completion {
                id,
                result: Ok(Value::external("port", port)),
            }
        }
        PendingOp::Connect {
            addr, connect_fd, ..
        } => {
            if result_code < 0 {
                let errno = -result_code;
                let is_timeout = errno == 125;
                let msg = if is_timeout {
                    "I/O operation timed out".to_string()
                } else {
                    format!("I/O error: {}", errno_message(errno))
                };
                let error_type = if is_timeout { "timeout" } else { "io-error" };
                return Completion {
                    id,
                    result: Err(error_val(error_type, msg)),
                };
            }
            // Connect: fd and address come from PendingOp (set at submission time).
            // io_uring: connect_fd = pre-created socket, result_code = 0.
            // thread pool: connect_fd = fd from TcpStream::connect, result_code unused.
            // io_uring: fd is pre-created in connect_fd. Thread pool: fd is result_code.
            let fd = connect_fd.unwrap_or(result_code as RawFd);
            let fd = unsafe { OwnedFd::from_raw_fd(fd) };
            let peer_addr = match addr {
                ConnectAddr::Tcp {
                    addr: host, port, ..
                } => crate::io::sockaddr::format_host_port(host, *port),
                ConnectAddr::Unix { path, .. } => path.clone(),
            };
            let new_port = match addr {
                ConnectAddr::Tcp { .. } => {
                    set_tcp_nodelay(&fd);
                    Port::new_tcp_stream(fd, peer_addr)
                }
                ConnectAddr::Unix { .. } => Port::new_unix_stream(fd, peer_addr),
            };
            Completion {
                id,
                result: Ok(Value::external("port", new_port)),
            }
        }
        PendingOp::Task { .. } => {
            if result_code < 0 {
                let msg = String::from_utf8_lossy(&data).to_string();
                Completion {
                    id,
                    result: Err(error_val("task-error", msg)),
                }
            } else {
                Completion {
                    id,
                    result: Ok(Value::bytes(data)),
                }
            }
        }
        PendingOp::WatchNext { watcher, .. } => {
            if result_code <= 0 {
                let msg = if result_code == 0 {
                    "watcher closed".to_string()
                } else {
                    format!(
                        "watch read error: {}",
                        std::io::Error::from_raw_os_error(-result_code)
                    )
                };
                return Completion {
                    id,
                    result: Err(error_val("io-error", msg)),
                };
            }
            // Parse inotify events from raw bytes
            let events = if let Some(w) = watcher.as_external::<crate::io::watch::FsWatcher>() {
                w.parse_events(&data[..result_code as usize])
            } else {
                Vec::new()
            };
            // Convert to Elle array of structs
            let event_values: Vec<Value> = events
                .iter()
                .map(|ev| {
                    let mut fields = std::collections::BTreeMap::new();
                    fields.insert(
                        crate::value::heap::TableKey::Keyword("kind".into()),
                        Value::keyword(ev.kind.as_keyword()),
                    );
                    fields.insert(
                        crate::value::heap::TableKey::Keyword("path".into()),
                        Value::string(ev.path.to_string_lossy().as_ref()),
                    );
                    Value::struct_from(fields)
                })
                .collect();
            Completion {
                id,
                result: Ok(Value::array(event_values)),
            }
        }
        PendingOp::PollFd { .. } => {
            // result_code is the revents mask (positive) or negative errno.
            if result_code < 0 {
                let errno = -result_code;
                let is_timeout = errno == 125; // ECANCELED from linked timeout
                let msg = if is_timeout {
                    "ev/poll-fd: timed out".to_string()
                } else {
                    format!("ev/poll-fd: poll error: errno {}", errno)
                };
                let error_type = if is_timeout { "timeout" } else { "io-error" };
                return Completion {
                    id,
                    result: Err(error_val(error_type, msg)),
                };
            }
            Completion {
                id,
                result: Ok(Value::int(result_code as i64)),
            }
        }
        PendingOp::Resolve { .. } => {
            if result_code < 0 {
                let msg = if data.is_empty() {
                    "getaddrinfo: resolution failed".to_string()
                } else {
                    String::from_utf8_lossy(&data).to_string()
                };
                return Completion {
                    id,
                    result: Err(error_val("dns-error", msg)),
                };
            }
            // data contains newline-separated IP address strings.
            let ips_str = String::from_utf8_lossy(&data);
            let ips: Vec<Value> = ips_str
                .lines()
                .filter(|s| !s.is_empty())
                .map(Value::string)
                .collect();
            Completion {
                id,
                result: Ok(Value::array(ips)),
            }
        }
        PendingOp::Port {
            op,
            port_key,
            port,
            listener_kind,
            ..
        } => {
            if result_code < 0 {
                // Error
                let errno = -result_code;
                let is_timeout = errno == 125; // ECANCELED
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
                return Completion {
                    id,
                    result: Err(error_val(error_type, msg)),
                };
            }

            if result_code == 0
                && matches!(
                    op,
                    IoOp::ReadLine { .. } | IoOp::Read { .. } | IoOp::ReadAll
                )
            {
                // EOF for read operations
                let state = fd_states
                    .entry(port_key.clone())
                    .or_insert_with(FdState::new);
                state.status = FdStatus::Eof;

                // For ReadLine: check buffer for a partial last line
                // (file content without trailing newline).
                if let IoOp::ReadLine { ref buffer } = op {
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
                        return Completion {
                            id,
                            result: unsafe {
                                crate::io::request::bytes_to_string_in_place(*buffer)
                            },
                        };
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
                        return Completion {
                            id,
                            result: unsafe {
                                crate::io::request::bytes_to_string_in_place(*buffer)
                            },
                        };
                    }
                    // No data at all — EOF on first read
                    return Completion {
                        id,
                        result: Ok(Value::NIL),
                    };
                }

                // For Read: return accumulated data on EOF.
                if let IoOp::Read { ref buffer, .. } = op {
                    // Check fd_state buffer first
                    if !state.buffer.is_empty() {
                        let partial: Vec<u8> = state.buffer.drain(..).collect();
                        unsafe {
                            let (dst, dst_cap) = crate::io::request::writeable_buffer_ptr(buffer);
                            let copy_len = partial.len().min(dst_cap);
                            std::ptr::copy_nonoverlapping(partial.as_ptr(), dst, copy_len);
                            crate::io::request::truncate_buffer(buffer, copy_len);
                        }
                        return Completion {
                            id,
                            result: Ok(*buffer),
                        };
                    }
                    // Check fiber buffer
                    let filled = pending.filled();
                    if filled > 0 {
                        unsafe {
                            crate::io::request::truncate_buffer(buffer, filled);
                        }
                        return Completion {
                            id,
                            result: Ok(*buffer),
                        };
                    }
                    // No data and EOF — return nil (empty read)
                    return Completion {
                        id,
                        result: Ok(Value::NIL),
                    };
                }

                // For ReadAll: return accumulated buffer on EOF
                // (empty bytes for empty files, not nil).
                if matches!(op, IoOp::ReadAll) {
                    let all: Vec<u8> = state.buffer.drain(..).collect();
                    return Completion {
                        id,
                        result: Ok(Value::bytes(all)),
                    };
                }

                return Completion {
                    id,
                    result: Ok(Value::NIL),
                };
            }

            // Success
            let value = match op {
                IoOp::ReadLine { ref buffer } => {
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
                    return Completion {
                        id,
                        result: unsafe { crate::io::request::bytes_to_string_in_place(*buffer) },
                    };
                }
                IoOp::Read { ref buffer, .. } => {
                    // Prepend any bytes left in the fd_state buffer from a
                    // previous over-read (e.g. ReadLine read past the line
                    // boundary). The submit path reduced the kernel read count
                    // by this amount so the total equals the requested count.
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
                    *buffer
                }
                IoOp::ReadAll => {
                    // ReadAll still uses the existing fd_states.buffer accumulation.
                    // Accumulated in fd_states.buffer by re-submission loop.
                    let state = fd_states
                        .entry(port_key.clone())
                        .or_insert_with(FdState::new);
                    state.buffer.extend_from_slice(&data);
                    // ReadAll returns bytes regardless of port encoding.
                    let all: Vec<u8> = state.buffer.drain(..).collect();
                    Value::bytes(all)
                }
                IoOp::Write { .. } | IoOp::SendTo { .. } => Value::int(result_code as i64),
                IoOp::Flush | IoOp::Shutdown { .. } | IoOp::Sleep { .. } => Value::NIL,
                IoOp::Accept { ref options } => {
                    // Accept: result_code is the new fd (from both io_uring and thread pool).
                    // Peer address is obtained via getpeername() — works uniformly.
                    let fd = result_code;
                    let peer_addr = crate::io::sockaddr::peer_address(fd);
                    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
                    // Apply user-specified socket options to the accepted fd.
                    crate::io::request::apply_socket_options(fd.as_raw_fd(), options);
                    let new_port = match listener_kind {
                        Some(PortKind::TcpListener) => {
                            set_tcp_nodelay(&fd);
                            Port::new_tcp_stream(fd, peer_addr)
                        }
                        Some(PortKind::UnixListener) => Port::new_unix_stream(fd, peer_addr),
                        _ => {
                            return Completion {
                                id,
                                result: Err(error_val("io-error", "invalid listener kind")),
                            };
                        }
                    };
                    Value::external("port", new_port)
                }
                IoOp::Connect { .. } => {
                    // Connect ops use PendingOp::Connect, not PendingOp::Port
                    unreachable!("Connect should use PendingOp::Connect variant")
                }
                IoOp::Spawn(_) | IoOp::ProcessWait => {
                    // Subprocess ops are dispatched before the port guard and never
                    // produce a PendingOp::Port entry — they cannot reach this branch.
                    unreachable!("Spawn/ProcessWait should be dispatched before port guard")
                }
                IoOp::Open { .. } => {
                    // Open ops use PendingOp::Open, not PendingOp::Port — cannot reach here.
                    unreachable!("Open should use PendingOp::Open variant")
                }
                IoOp::Seek { .. } | IoOp::Tell => {
                    // Seek/Tell are immediate completions (lseek syscall, no io_uring).
                    // They never produce a PendingOp::Port entry — cannot reach here.
                    unreachable!(
                        "Seek/Tell are handled as immediate completions before PendingOp insertion"
                    )
                }
                IoOp::Task(_) => {
                    // Task ops use PendingOp::Task, not PendingOp::Port — cannot reach here.
                    unreachable!("Task should use PendingOp::Task variant")
                }
                IoOp::Resolve { .. } => {
                    unreachable!("Resolve is portless; cannot reach PendingOp::Port")
                }
                IoOp::WatchNext => {
                    unreachable!("WatchNext uses PendingOp::WatchNext, not PendingOp::Port")
                }
                IoOp::PollFd { .. } => {
                    unreachable!("PollFd is portless; cannot reach PendingOp::Port")
                }
                // Close completion: port already closed in submit. Return nil.
                IoOp::Close => Value::NIL,
                IoOp::RecvFrom { .. } => {
                    // RecvFrom: data format is addr_len (4 bytes LE) + sockaddr_storage + payload
                    if data.len() < 4 {
                        return Completion {
                            id,
                            result: Err(error_val("io-error", "invalid recvfrom data")),
                        };
                    }
                    let addr_len =
                        u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as libc::socklen_t;
                    let addr_offset = 4 + std::mem::size_of::<libc::sockaddr_storage>();
                    if data.len() < addr_offset {
                        return Completion {
                            id,
                            result: Err(error_val("io-error", "invalid recvfrom data")),
                        };
                    }
                    let addr_bytes = &data[4..4 + std::mem::size_of::<libc::sockaddr_storage>()];
                    let addr_storage = unsafe {
                        let mut storage: libc::sockaddr_storage = std::mem::zeroed();
                        std::ptr::copy_nonoverlapping(
                            addr_bytes.as_ptr(),
                            &mut storage as *mut _ as *mut u8,
                            std::mem::size_of::<libc::sockaddr_storage>(),
                        );
                        storage
                    };
                    let (addr_str, port_num) = crate::io::sockaddr::parse(&addr_storage, addr_len);
                    let payload = data[addr_offset..].to_vec();
                    let mut fields = std::collections::BTreeMap::new();
                    fields.insert(TableKey::Keyword("data".into()), Value::bytes(payload));
                    fields.insert(TableKey::Keyword("addr".into()), Value::string(addr_str));
                    fields.insert(
                        TableKey::Keyword("port".into()),
                        Value::int(port_num as i64),
                    );
                    Value::struct_from(fields)
                }
            };
            Completion {
                id,
                result: Ok(value),
            }
        }
    }
}
