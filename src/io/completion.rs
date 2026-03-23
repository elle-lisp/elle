//! Completion processing for async I/O operations.

use crate::io::pending::PendingOp;
use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::{ConnectAddr, IoOp};
use crate::io::types::{FdState, FdStatus, PortKey};
use crate::io::Completion;
use crate::port::{Encoding, Port, PortKind};
use crate::value::heap::TableKey;
use crate::value::{error_val, Value};
use std::collections::HashMap;
use std::os::unix::io::{FromRawFd, OwnedFd};

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
    buf_handle: BufferHandle,
) -> Completion {
    // Release the buffer back to the pool
    buffer_pool.release(buf_handle);

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
                // Thread pool path: exit code comes directly as the raw result integer
                // (from waitpid in PoolOp::ProcessWait dispatch).
                result_code
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
            let fd = connect_fd.expect("Connect completion without connect_fd");
            let fd = unsafe { OwnedFd::from_raw_fd(fd) };
            let peer_addr = match addr {
                ConnectAddr::Tcp { addr: host, port } => {
                    crate::io::sockaddr::format_host_port(host, *port)
                }
                ConnectAddr::Unix { path } => path.clone(),
            };
            let new_port = match addr {
                ConnectAddr::Tcp { .. } => Port::new_tcp_stream(fd, peer_addr),
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
                state.status = FdStatus::Error(msg.clone());
                return Completion {
                    id,
                    result: Err(error_val(error_type, msg)),
                };
            }

            if result_code == 0 && matches!(op, IoOp::ReadLine | IoOp::Read { .. } | IoOp::ReadAll)
            {
                // EOF for read operations
                let state = fd_states
                    .entry(port_key.clone())
                    .or_insert_with(FdState::new);
                state.status = FdStatus::Eof;

                // For ReadLine: check buffer for a partial last line
                // (file content without trailing newline).
                if matches!(op, IoOp::ReadLine) && !state.buffer.is_empty() {
                    let remainder: Vec<u8> = state.buffer.drain(..).collect();
                    let s = String::from_utf8_lossy(&remainder);
                    let trimmed = s.trim_end_matches('\n').trim_end_matches('\r');
                    return Completion {
                        id,
                        result: Ok(Value::string(trimmed)),
                    };
                }

                return Completion {
                    id,
                    result: Ok(Value::NIL),
                };
            }

            // Success
            let value = match op {
                IoOp::ReadLine => {
                    // Per-fd buffering: append raw data, extract one line.
                    let state = fd_states
                        .entry(port_key.clone())
                        .or_insert_with(FdState::new);
                    state.buffer.extend_from_slice(&data);

                    if let Some(pos) = state.buffer.iter().position(|&b| b == b'\n') {
                        let line_bytes: Vec<u8> = state.buffer.drain(..=pos).collect();
                        let s = String::from_utf8_lossy(&line_bytes);
                        let trimmed = s.trim_end_matches('\n').trim_end_matches('\r');
                        Value::string(trimmed)
                    } else {
                        // No newline — partial line at EOF. Drain everything.
                        let all: Vec<u8> = state.buffer.drain(..).collect();
                        let s = String::from_utf8_lossy(&all);
                        let trimmed = s.trim_end_matches('\n').trim_end_matches('\r');
                        Value::string(trimmed)
                    }
                }
                IoOp::Read { .. } | IoOp::ReadAll => {
                    // Prepend any bytes left in the fd_state buffer from a
                    // previous over-read (e.g. ReadLine read past the line
                    // boundary). The submit path reduced the kernel read count
                    // by this amount so the total equals the requested count.
                    let state = fd_states
                        .entry(port_key.clone())
                        .or_insert_with(FdState::new);
                    let combined = if !state.buffer.is_empty() {
                        let mut buf: Vec<u8> = state.buffer.drain(..).collect();
                        buf.extend_from_slice(&data);
                        buf
                    } else {
                        data
                    };
                    if let Some(p) = port.as_external::<Port>() {
                        match p.encoding() {
                            Encoding::Text => {
                                let s = String::from_utf8_lossy(&combined);
                                Value::string(s.as_ref())
                            }
                            Encoding::Binary => Value::bytes(combined),
                        }
                    } else {
                        Value::string(String::from_utf8_lossy(&combined).as_ref())
                    }
                }
                IoOp::Write { .. } | IoOp::SendTo { .. } => Value::int(result_code as i64),
                IoOp::Flush | IoOp::Shutdown { .. } | IoOp::Sleep { .. } => Value::NIL,
                IoOp::Accept => {
                    // Accept: result_code is the new fd (from both io_uring and thread pool).
                    // Peer address is obtained via getpeername() — works uniformly.
                    let fd = result_code;
                    let peer_addr = crate::io::sockaddr::peer_address(fd);
                    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
                    let new_port = match listener_kind {
                        Some(PortKind::TcpListener) => Port::new_tcp_stream(fd, peer_addr),
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
                    // Resolve is portless and dispatched to the thread pool — never
                    // produces a PendingOp::Port entry.
                    unreachable!("Resolve is portless; cannot reach PendingOp::Port")
                }
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
