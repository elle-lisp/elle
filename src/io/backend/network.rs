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
        let peer_addr = crate::io::sockaddr::format(&addr_storage, addr_len);

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
                let addr_str = crate::io::sockaddr::format_host_port(host, *port_num);
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

                let (sun, addr_len) = match crate::io::sockaddr::build_unix(path) {
                    Ok(result) => result,
                    Err(msg) => {
                        unsafe { libc::close(fd) };
                        return (
                            SIG_ERROR,
                            error_val("io-error", format!("unix/connect: {}", msg)),
                        );
                    }
                };

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

        let addr_str = crate::io::sockaddr::format_host_port(addr, port_num);
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

        let (sa_bytes, sa_len) = crate::io::sockaddr::build_inet(&dest);

        let ret = unsafe {
            libc::sendto(
                raw_fd,
                bytes.as_ptr() as *const libc::c_void,
                bytes.len(),
                0,
                sa_bytes.as_ptr() as *const libc::sockaddr,
                sa_len,
            )
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
        let (src_addr, src_port) = crate::io::sockaddr::parse(&addr_storage, addr_len);

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
