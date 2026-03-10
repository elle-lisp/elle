//! Network operation handlers for SyncBackend.

use super::SyncBackend;
use crate::io::request::ConnectAddr;
use crate::port::{Port, PortKind};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};

use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};

impl SyncBackend {
    pub(super) fn execute_accept(&self, port: &Port) -> (SignalBits, Value) {
        let raw_fd = match port.with_fd(|fd| fd.as_raw_fd()) {
            Some(fd) => fd,
            None => {
                return (
                    SIG_ERROR,
                    error_val("io-error", "accept: port fd unavailable"),
                )
            }
        };

        let mut addr_storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        let mut addr_len: libc::socklen_t =
            std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;

        let new_fd = unsafe {
            libc::accept(
                raw_fd,
                &mut addr_storage as *mut libc::sockaddr_storage as *mut libc::sockaddr,
                &mut addr_len,
            )
        };
        if new_fd < 0 {
            return (
                SIG_ERROR,
                error_val(
                    "io-error",
                    format!("accept: {}", io::Error::last_os_error()),
                ),
            );
        }

        // Set CLOEXEC
        unsafe {
            libc::fcntl(new_fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }

        let owned_fd = unsafe { OwnedFd::from_raw_fd(new_fd) };
        let peer_addr = format_sockaddr(&addr_storage, addr_len);

        let new_port = match port.kind() {
            PortKind::TcpListener => Port::new_tcp_stream(owned_fd, peer_addr),
            PortKind::UnixListener => Port::new_unix_stream(owned_fd, peer_addr),
            _ => unreachable!(), // dispatch guarantees listener kind
        };

        (SIG_OK, Value::external("port", new_port))
    }

    pub(super) fn execute_connect(&self, addr: &ConnectAddr) -> (SignalBits, Value) {
        match addr {
            ConnectAddr::Tcp {
                addr: host,
                port: port_num,
            } => {
                let addr_str = format!("{}:{}", host, port_num);
                match std::net::TcpStream::connect(&addr_str) {
                    Ok(stream) => {
                        let peer = stream
                            .peer_addr()
                            .map(|a| a.to_string())
                            .unwrap_or_else(|_| addr_str.clone());
                        let owned_fd: OwnedFd = stream.into();
                        let new_port = Port::new_tcp_stream(owned_fd, peer);
                        (SIG_OK, Value::external("port", new_port))
                    }
                    Err(e) => (
                        SIG_ERROR,
                        error_val("io-error", format!("tcp/connect: {}", e)),
                    ),
                }
            }
            ConnectAddr::Unix { path } => {
                let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
                if fd < 0 {
                    return (
                        SIG_ERROR,
                        error_val(
                            "io-error",
                            format!("unix/connect: socket: {}", io::Error::last_os_error()),
                        ),
                    );
                }

                let mut sun: libc::sockaddr_un = unsafe { std::mem::zeroed() };
                sun.sun_family = libc::AF_UNIX as libc::sa_family_t;

                let (path_bytes, addr_len) = if let Some(name) = path.strip_prefix('@') {
                    // Abstract socket: sun_path[0] = 0, then rest of name
                    let max = sun.sun_path.len() - 1;
                    if name.len() > max {
                        unsafe { libc::close(fd) };
                        return (
                            SIG_ERROR,
                            error_val("io-error", "unix/connect: path too long"),
                        );
                    }
                    sun.sun_path[0] = 0;
                    for (i, b) in name.bytes().enumerate() {
                        sun.sun_path[i + 1] = b as libc::c_char;
                    }
                    let len = std::mem::size_of::<libc::sa_family_t>() + 1 + name.len();
                    (name.len() + 1, len as libc::socklen_t)
                } else {
                    let max = sun.sun_path.len() - 1;
                    if path.len() > max {
                        unsafe { libc::close(fd) };
                        return (
                            SIG_ERROR,
                            error_val("io-error", "unix/connect: path too long"),
                        );
                    }
                    for (i, b) in path.bytes().enumerate() {
                        sun.sun_path[i] = b as libc::c_char;
                    }
                    let len = std::mem::size_of::<libc::sa_family_t>() + path.len() + 1;
                    (path.len(), len as libc::socklen_t)
                };
                let _ = path_bytes; // used only for length calculation

                let ret = unsafe {
                    libc::connect(
                        fd,
                        &sun as *const libc::sockaddr_un as *const libc::sockaddr,
                        addr_len,
                    )
                };
                if ret < 0 {
                    let err = io::Error::last_os_error();
                    unsafe { libc::close(fd) };
                    return (
                        SIG_ERROR,
                        error_val("io-error", format!("unix/connect: {}", err)),
                    );
                }

                unsafe {
                    libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
                }

                let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };
                let new_port = Port::new_unix_stream(owned_fd, path.clone());
                (SIG_OK, Value::external("port", new_port))
            }
        }
    }

    pub(super) fn execute_send_to(
        &self,
        port: &Port,
        addr: &str,
        port_num: u16,
        data: &Value,
    ) -> (SignalBits, Value) {
        let bytes: Vec<u8> = if let Some(s) = data.with_string(|s| s.as_bytes().to_vec()) {
            s
        } else if let Some(b) = data.as_bytes() {
            b.to_vec()
        } else if let Some(b) = data.as_bytes_mut() {
            b.borrow().clone()
        } else if let Some(b) = data.as_string_mut() {
            b.borrow().clone()
        } else {
            format!("{}", data).into_bytes()
        };

        let addr_str = format!("{}:{}", addr, port_num);
        let dest: std::net::SocketAddr = match addr_str.parse() {
            Ok(a) => a,
            Err(_) => {
                // Try DNS resolution
                use std::net::ToSocketAddrs;
                match addr_str.to_socket_addrs() {
                    Ok(mut addrs) => match addrs.next() {
                        Some(a) => a,
                        None => {
                            return (
                                SIG_ERROR,
                                error_val(
                                    "io-error",
                                    format!("udp/send-to: could not resolve {}", addr_str),
                                ),
                            )
                        }
                    },
                    Err(e) => {
                        return (
                            SIG_ERROR,
                            error_val("io-error", format!("udp/send-to: {}", e)),
                        )
                    }
                }
            }
        };

        let raw_fd = match port.with_fd(|fd| fd.as_raw_fd()) {
            Some(fd) => fd,
            None => {
                return (
                    SIG_ERROR,
                    error_val("io-error", "udp/send-to: port fd unavailable"),
                )
            }
        };

        let (sa_ptr, sa_len) = match dest {
            std::net::SocketAddr::V4(ref v4) => {
                let sin = libc::sockaddr_in {
                    sin_family: libc::AF_INET as libc::sa_family_t,
                    sin_port: v4.port().to_be(),
                    sin_addr: libc::in_addr {
                        s_addr: u32::from_ne_bytes(v4.ip().octets()),
                    },
                    sin_zero: [0; 8],
                };
                // SAFETY: sin is stack-local and lives through the sendto call
                let boxed = Box::new(sin);
                (
                    Box::into_raw(boxed) as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                )
            }
            std::net::SocketAddr::V6(ref v6) => {
                let sin6 = libc::sockaddr_in6 {
                    sin6_family: libc::AF_INET6 as libc::sa_family_t,
                    sin6_port: v6.port().to_be(),
                    sin6_flowinfo: v6.flowinfo(),
                    sin6_addr: libc::in6_addr {
                        s6_addr: v6.ip().octets(),
                    },
                    sin6_scope_id: v6.scope_id(),
                };
                let boxed = Box::new(sin6);
                (
                    Box::into_raw(boxed) as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                )
            }
        };

        let ret = unsafe {
            let r = libc::sendto(
                raw_fd,
                bytes.as_ptr() as *const libc::c_void,
                bytes.len(),
                0,
                sa_ptr,
                sa_len,
            );
            // Reclaim the box to avoid leak
            match dest {
                std::net::SocketAddr::V4(_) => {
                    drop(Box::from_raw(sa_ptr as *mut libc::sockaddr_in));
                }
                std::net::SocketAddr::V6(_) => {
                    drop(Box::from_raw(sa_ptr as *mut libc::sockaddr_in6));
                }
            }
            r
        };

        if ret < 0 {
            (
                SIG_ERROR,
                error_val(
                    "io-error",
                    format!("udp/send-to: {}", io::Error::last_os_error()),
                ),
            )
        } else {
            (SIG_OK, Value::int(ret as i64))
        }
    }

    pub(super) fn execute_recv_from(&self, port: &Port, count: usize) -> (SignalBits, Value) {
        let raw_fd = match port.with_fd(|fd| fd.as_raw_fd()) {
            Some(fd) => fd,
            None => {
                return (
                    SIG_ERROR,
                    error_val("io-error", "udp/recv-from: port fd unavailable"),
                )
            }
        };

        let mut buf = vec![0u8; count];
        let mut addr_storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        let mut addr_len: libc::socklen_t =
            std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;

        let ret = unsafe {
            libc::recvfrom(
                raw_fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                0,
                &mut addr_storage as *mut libc::sockaddr_storage as *mut libc::sockaddr,
                &mut addr_len,
            )
        };

        if ret < 0 {
            return (
                SIG_ERROR,
                error_val(
                    "io-error",
                    format!("udp/recv-from: {}", io::Error::last_os_error()),
                ),
            );
        }

        buf.truncate(ret as usize);
        let (src_addr, src_port) = parse_sockaddr_ip(&addr_storage, addr_len);

        // Build struct: {:data bytes :addr string :port int}
        use crate::value::heap::TableKey;
        use std::collections::BTreeMap;
        let mut fields = BTreeMap::new();
        fields.insert(TableKey::Keyword("data".into()), Value::bytes(buf));
        fields.insert(TableKey::Keyword("addr".into()), Value::string(src_addr));
        fields.insert(
            TableKey::Keyword("port".into()),
            Value::int(src_port as i64),
        );

        (SIG_OK, Value::struct_from(fields))
    }

    pub(super) fn execute_shutdown(&self, port: &Port, how: i32) -> (SignalBits, Value) {
        let raw_fd = match port.with_fd(|fd| fd.as_raw_fd()) {
            Some(fd) => fd,
            None => {
                return (
                    SIG_ERROR,
                    error_val("io-error", "shutdown: port fd unavailable"),
                )
            }
        };

        let ret = unsafe { libc::shutdown(raw_fd, how) };
        if ret < 0 {
            (
                SIG_ERROR,
                error_val(
                    "io-error",
                    format!("shutdown: {}", io::Error::last_os_error()),
                ),
            )
        } else {
            (SIG_OK, Value::NIL)
        }
    }
}

/// Format a sockaddr_storage as a human-readable string.
pub(super) fn format_sockaddr(addr: &libc::sockaddr_storage, len: libc::socklen_t) -> String {
    match addr.ss_family as libc::c_int {
        libc::AF_INET => {
            let sin =
                unsafe { &*(addr as *const libc::sockaddr_storage as *const libc::sockaddr_in) };
            let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
            let port = u16::from_be(sin.sin_port);
            format!("{}:{}", ip, port)
        }
        libc::AF_INET6 => {
            let sin6 =
                unsafe { &*(addr as *const libc::sockaddr_storage as *const libc::sockaddr_in6) };
            let ip = std::net::Ipv6Addr::from(sin6.sin6_addr.s6_addr);
            let port = u16::from_be(sin6.sin6_port);
            format!("[{}]:{}", ip, port)
        }
        libc::AF_UNIX => {
            let sun =
                unsafe { &*(addr as *const libc::sockaddr_storage as *const libc::sockaddr_un) };
            let path_offset = std::mem::size_of::<libc::sa_family_t>();
            let path_len = (len as usize).saturating_sub(path_offset);
            if path_len == 0 {
                return "unix:unnamed".to_string();
            }
            if sun.sun_path[0] == 0 {
                // Abstract socket
                let name_bytes: Vec<u8> =
                    sun.sun_path[1..path_len].iter().map(|&c| c as u8).collect();
                format!("@{}", String::from_utf8_lossy(&name_bytes))
            } else {
                let name_bytes: Vec<u8> = sun.sun_path[..path_len]
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8)
                    .collect();
                String::from_utf8_lossy(&name_bytes).to_string()
            }
        }
        _ => "unknown".to_string(),
    }
}

/// Parse an IP sockaddr into (addr_string, port_number).
pub(super) fn parse_sockaddr_ip(
    addr: &libc::sockaddr_storage,
    _len: libc::socklen_t,
) -> (String, u16) {
    match addr.ss_family as libc::c_int {
        libc::AF_INET => {
            let sin =
                unsafe { &*(addr as *const libc::sockaddr_storage as *const libc::sockaddr_in) };
            let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
            let port = u16::from_be(sin.sin_port);
            (ip.to_string(), port)
        }
        libc::AF_INET6 => {
            let sin6 =
                unsafe { &*(addr as *const libc::sockaddr_storage as *const libc::sockaddr_in6) };
            let ip = std::net::Ipv6Addr::from(sin6.sin6_addr.s6_addr);
            let port = u16::from_be(sin6.sin6_port);
            (format!("[{}]", ip), port)
        }
        _ => ("unknown".to_string(), 0),
    }
}
