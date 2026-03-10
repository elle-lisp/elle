//! Completion processing for async I/O operations.

use crate::io::aio::{Completion, PendingOp};
use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::{ConnectAddr, IoOp};
use crate::io::types::{FdState, FdStatus, PortKey};
use crate::port::{Encoding, Port, PortKind};
use crate::value::heap::TableKey;
use crate::value::{error_val, Value};
use std::collections::HashMap;
use std::os::unix::io::{FromRawFd, OwnedFd};

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

    if result_code < 0 {
        // Error
        let errno = -result_code;
        // Check for timeout (ECANCELED = 125 on Linux)
        let is_timeout = errno == 125; // ECANCELED
        let msg = if is_timeout {
            "I/O operation timed out".to_string()
        } else {
            format!("I/O error: errno {}", errno)
        };
        let error_type = if is_timeout { "timeout" } else { "io-error" };
        let state = fd_states
            .entry(pending.port_key.clone())
            .or_insert_with(FdState::new);
        state.status = FdStatus::Error(msg.clone());
        Completion {
            id,
            result: Err(error_val(error_type, msg)),
        }
    } else if result_code == 0
        && matches!(
            pending.op,
            IoOp::ReadLine | IoOp::Read { .. } | IoOp::ReadAll
        )
    {
        // EOF for read operations
        let state = fd_states
            .entry(pending.port_key.clone())
            .or_insert_with(FdState::new);
        state.status = FdStatus::Eof;
        Completion {
            id,
            result: Ok(Value::NIL),
        }
    } else {
        // Success
        let value = match &pending.op {
            IoOp::ReadLine => {
                let s = String::from_utf8_lossy(&data);
                let trimmed = s.trim_end_matches('\n').trim_end_matches('\r');
                Value::string(trimmed)
            }
            IoOp::Read { .. } | IoOp::ReadAll => {
                // Check port encoding
                if let Some(port) = pending.port.as_external::<Port>() {
                    match port.encoding() {
                        Encoding::Text => {
                            let s = String::from_utf8_lossy(&data);
                            Value::string(s.as_ref())
                        }
                        Encoding::Binary => Value::bytes(data),
                    }
                } else {
                    Value::string(String::from_utf8_lossy(&data).as_ref())
                }
            }
            IoOp::Write { .. } | IoOp::SendTo { .. } => Value::int(result_code as i64),
            IoOp::Flush | IoOp::Shutdown { .. } => Value::NIL,
            IoOp::Accept => {
                // Accept: result_code is new fd, data is encoded address
                // Data format: addr_len (4 bytes LE) + sockaddr_storage
                if data.len() < 4 {
                    return Completion {
                        id,
                        result: Err(error_val("io-error", "invalid accept data")),
                    };
                }
                let addr_len =
                    u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as libc::socklen_t;
                if data.len() < 4 + std::mem::size_of::<libc::sockaddr_storage>() {
                    return Completion {
                        id,
                        result: Err(error_val("io-error", "invalid accept data")),
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
                let peer_addr = format_sockaddr(&addr_storage, addr_len);
                let fd = unsafe { OwnedFd::from_raw_fd(result_code) };
                let new_port = match pending.listener_kind {
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
            IoOp::Connect { addr } => {
                // Connect: result_code is new fd, data is peer address string
                let peer_addr = String::from_utf8_lossy(&data).to_string();
                let fd = unsafe { OwnedFd::from_raw_fd(result_code) };
                let new_port = match addr {
                    ConnectAddr::Tcp { .. } => Port::new_tcp_stream(fd, peer_addr),
                    ConnectAddr::Unix { path } => Port::new_unix_stream(fd, path.clone()),
                };
                Value::external("port", new_port)
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
                let (addr_str, port_num) = parse_sockaddr(&addr_storage, addr_len);
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

/// Format a sockaddr_storage into a string address.
pub(super) fn format_sockaddr(addr: &libc::sockaddr_storage, _len: libc::socklen_t) -> String {
    unsafe {
        match addr.ss_family as i32 {
            libc::AF_INET => {
                let sin = addr as *const _ as *const libc::sockaddr_in;
                let ip = (*sin).sin_addr.s_addr;
                let port = u16::from_be((*sin).sin_port);
                let octets = ip.to_le_bytes();
                format!(
                    "{}.{}.{}.{}:{}",
                    octets[0], octets[1], octets[2], octets[3], port
                )
            }
            libc::AF_INET6 => {
                let sin6 = addr as *const _ as *const libc::sockaddr_in6;
                let ip = (*sin6).sin6_addr.s6_addr;
                let port = u16::from_be((*sin6).sin6_port);
                let ip_str = format!(
                    "{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}",
                    ip[0], ip[1], ip[2], ip[3], ip[4], ip[5], ip[6], ip[7],
                    ip[8], ip[9], ip[10], ip[11], ip[12], ip[13], ip[14], ip[15]
                );
                format!("[{}]:{}", ip_str, port)
            }
            libc::AF_UNIX => {
                let sun = addr as *const _ as *const libc::sockaddr_un;
                let path_ptr = (*sun).sun_path.as_ptr();
                if *path_ptr == 0 {
                    // Abstract socket
                    let mut name = String::new();
                    let mut i = 1;
                    while i < (*sun).sun_path.len() && (*sun).sun_path[i] != 0 {
                        name.push((*sun).sun_path[i] as u8 as char);
                        i += 1;
                    }
                    format!("@{}", name)
                } else {
                    // Regular path
                    let cstr = std::ffi::CStr::from_ptr(path_ptr);
                    cstr.to_string_lossy().to_string()
                }
            }
            _ => "unknown".to_string(),
        }
    }
}

/// Parse a sockaddr_storage into (address_string, port_number).
pub(super) fn parse_sockaddr(
    addr: &libc::sockaddr_storage,
    _len: libc::socklen_t,
) -> (String, u16) {
    unsafe {
        match addr.ss_family as i32 {
            libc::AF_INET => {
                let sin = addr as *const _ as *const libc::sockaddr_in;
                let ip = (*sin).sin_addr.s_addr;
                let port = u16::from_be((*sin).sin_port);
                let octets = ip.to_le_bytes();
                let addr_str = format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3]);
                (addr_str, port)
            }
            libc::AF_INET6 => {
                let sin6 = addr as *const _ as *const libc::sockaddr_in6;
                let ip = (*sin6).sin6_addr.s6_addr;
                let port = u16::from_be((*sin6).sin6_port);
                let ip_str = format!(
                    "{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}",
                    ip[0], ip[1], ip[2], ip[3], ip[4], ip[5], ip[6], ip[7],
                    ip[8], ip[9], ip[10], ip[11], ip[12], ip[13], ip[14], ip[15]
                );
                (format!("[{}]", ip_str), port)
            }
            libc::AF_UNIX => {
                let sun = addr as *const _ as *const libc::sockaddr_un;
                let path_ptr = (*sun).sun_path.as_ptr();
                if *path_ptr == 0 {
                    // Abstract socket
                    let mut name = String::new();
                    let mut i = 1;
                    while i < (*sun).sun_path.len() && (*sun).sun_path[i] != 0 {
                        name.push((*sun).sun_path[i] as u8 as char);
                        i += 1;
                    }
                    (format!("@{}", name), 0)
                } else {
                    // Regular path
                    let cstr = std::ffi::CStr::from_ptr(path_ptr);
                    (cstr.to_string_lossy().to_string(), 0)
                }
            }
            _ => ("unknown".to_string(), 0),
        }
    }
}
