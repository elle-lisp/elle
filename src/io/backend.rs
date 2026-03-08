//! SyncBackend — synchronous I/O backend with per-fd buffering.
//!
//! Wrapped as ExternalObject with type_name "io-backend".
//! Uses `RefCell<SyncBackendInner>` for interior mutability
//! (ExternalObject wraps in Rc, so &mut self is unavailable).
//!
//! ## Buffer drain invariant
//!
//! Data already received is never lost when a fd dies (EOF or error).
//! The state machine:
//!
//! - State 1: Buffer has data, fd alive → read more if needed
//! - State 2: Buffer has data, fd dead → drain buffer first
//! - State 3: Buffer empty, fd dead → return nil (EOF) or error

use crate::io::request::{ConnectAddr, IoOp, IoRequest};
use crate::io::types::{FdState, FdStatus, PortKey};
use crate::port::{Direction, Encoding, Port, PortKind};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};

struct SyncBackendInner {
    states: HashMap<PortKey, FdState>,
}

/// Synchronous I/O backend. Wrapped as ExternalObject "io-backend".
pub struct SyncBackend {
    inner: RefCell<SyncBackendInner>,
}

impl std::fmt::Debug for SyncBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#<io-backend:sync>")
    }
}

impl Default for SyncBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncBackend {
    pub fn new() -> Self {
        SyncBackend {
            inner: RefCell::new(SyncBackendInner {
                states: HashMap::new(),
            }),
        }
    }

    /// Execute an I/O request synchronously.
    /// Returns `(SIG_OK, result)` on success, `(SIG_ERROR, error)` on failure.
    pub fn execute(&self, request: &IoRequest) -> (SignalBits, Value) {
        // Connect creates a new port — no existing port required.
        if let IoOp::Connect { ref addr } = request.op {
            return self.execute_connect(addr);
        }

        // All remaining ops require a valid port.
        let port = match request.port.as_external::<Port>() {
            Some(p) => p,
            None => {
                return (
                    SIG_ERROR,
                    error_val("type-error", "io/execute: request contains non-port value"),
                )
            }
        };

        if port.is_closed() {
            return (
                SIG_ERROR,
                error_val("io-error", "io/execute: port is closed"),
            );
        }

        // Dispatch by port kind + op.
        match port.kind() {
            PortKind::TcpListener | PortKind::UnixListener => match &request.op {
                IoOp::Accept => self.execute_accept(port),
                _ => (
                    SIG_ERROR,
                    error_val(
                        "io-error",
                        "cannot use stream operations on a listener; use tcp/accept or unix/accept",
                    ),
                ),
            },
            PortKind::UdpSocket => match &request.op {
                IoOp::SendTo {
                    ref addr,
                    port_num,
                    ref data,
                } => self.execute_send_to(port, addr, *port_num, data),
                IoOp::RecvFrom { count } => self.execute_recv_from(port, *count),
                IoOp::Shutdown { how } => self.execute_shutdown(port, *how),
                _ => (
                    SIG_ERROR,
                    error_val(
                        "io-error",
                        "cannot use stream operations on UDP socket; use udp/send-to or udp/recv-from",
                    ),
                ),
            },
            _ => match &request.op {
                IoOp::ReadLine => self.execute_read_line(port),
                IoOp::Read { count } => self.execute_read(port, *count),
                IoOp::ReadAll => self.execute_read_all(port),
                IoOp::Write { data } => self.execute_write(port, data),
                IoOp::Flush => self.execute_flush(port),
                IoOp::Shutdown { how } => self.execute_shutdown(port, *how),
                IoOp::Accept => (
                    SIG_ERROR,
                    error_val("io-error", "accept: port is not a listener"),
                ),
                IoOp::Connect { .. } => unreachable!(), // handled above
                IoOp::SendTo { .. } | IoOp::RecvFrom { .. } => (
                    SIG_ERROR,
                    error_val("io-error", "UDP operations require a UDP socket"),
                ),
            },
        }
    }

    fn port_key(port: &Port) -> PortKey {
        match port.kind() {
            PortKind::Stdin => PortKey::Stdin,
            PortKind::Stdout => PortKey::Stdout,
            PortKind::Stderr => PortKey::Stderr,
            PortKind::File
            | PortKind::TcpListener
            | PortKind::TcpStream
            | PortKind::UdpSocket
            | PortKind::UnixListener
            | PortKind::UnixStream => {
                match port.with_fd(|fd| fd.as_raw_fd()) {
                    Some(raw) => PortKey::Fd(raw),
                    None => PortKey::Fd(-1), // closed, will error elsewhere
                }
            }
        }
    }

    fn validate_readable(port: &Port) -> Result<(), (SignalBits, Value)> {
        match port.direction() {
            Direction::Read | Direction::ReadWrite => Ok(()),
            Direction::Write => Err((
                SIG_ERROR,
                error_val("io-error", "io/execute: cannot read from write-only port"),
            )),
        }
    }

    fn validate_writable(port: &Port) -> Result<(), (SignalBits, Value)> {
        match port.direction() {
            Direction::Write | Direction::ReadWrite => Ok(()),
            Direction::Read => Err((
                SIG_ERROR,
                error_val("io-error", "io/execute: cannot write to read-only port"),
            )),
        }
    }

    fn execute_read_line(&self, port: &Port) -> (SignalBits, Value) {
        if let Err(e) = Self::validate_readable(port) {
            return e;
        }
        let key = Self::port_key(port);
        let mut inner = self.inner.borrow_mut();
        let state = inner.states.entry(key).or_insert_with(FdState::new);

        // Try to find a newline in the buffer
        loop {
            if let Some(pos) = state.buffer.iter().position(|&b| b == b'\n') {
                // Found newline — return line without the newline
                let line: Vec<u8> = state.buffer.drain(..=pos).collect();
                // Strip trailing \n (and \r\n if present)
                let s = String::from_utf8_lossy(&line);
                let trimmed = s.trim_end_matches('\n').trim_end_matches('\r');
                return (SIG_OK, Value::string(trimmed));
            }

            // No newline in buffer — check fd status
            match &state.status {
                FdStatus::Eof => {
                    // Buffer drain: return remainder if any
                    if state.buffer.is_empty() {
                        return (SIG_OK, Value::NIL);
                    }
                    let remainder: Vec<u8> = state.buffer.drain(..).collect();
                    let s = String::from_utf8_lossy(&remainder);
                    return (SIG_OK, Value::string(s.as_ref()));
                }
                FdStatus::Error(msg) => {
                    if state.buffer.is_empty() {
                        return (SIG_ERROR, error_val("io-error", msg.clone()));
                    }
                    let remainder: Vec<u8> = state.buffer.drain(..).collect();
                    let s = String::from_utf8_lossy(&remainder);
                    return (SIG_OK, Value::string(s.as_ref()));
                }
                FdStatus::Open => {
                    // Read more data from fd
                    let mut tmp = [0u8; 4096];
                    match Self::read_from_port(port, &mut tmp) {
                        Ok(0) => {
                            state.status = FdStatus::Eof;
                            // Loop back to drain buffer
                        }
                        Ok(n) => {
                            state.buffer.extend_from_slice(&tmp[..n]);
                            // Loop back to scan for newline
                        }
                        Err(e) => {
                            state.status = FdStatus::Error(e.to_string());
                            // Loop back to drain buffer
                        }
                    }
                }
            }
        }
    }

    fn execute_read(&self, port: &Port, count: usize) -> (SignalBits, Value) {
        if let Err(e) = Self::validate_readable(port) {
            return e;
        }
        let key = Self::port_key(port);
        let mut inner = self.inner.borrow_mut();
        let state = inner.states.entry(key).or_insert_with(FdState::new);

        // If buffer has enough data, return from buffer
        if state.buffer.len() >= count {
            let data: Vec<u8> = state.buffer.drain(..count).collect();
            return Self::bytes_to_value(port, data);
        }

        // Try to read more
        match &state.status {
            FdStatus::Eof | FdStatus::Error(_) => {
                if state.buffer.is_empty() {
                    match &state.status {
                        FdStatus::Eof => (SIG_OK, Value::NIL),
                        FdStatus::Error(msg) => (SIG_ERROR, error_val("io-error", msg.clone())),
                        _ => unreachable!(),
                    }
                } else {
                    let data: Vec<u8> = state.buffer.drain(..).collect();
                    Self::bytes_to_value(port, data)
                }
            }
            FdStatus::Open => {
                // Read up to count bytes total
                let need = count - state.buffer.len();
                let mut tmp = vec![0u8; need];
                match Self::read_from_port(port, &mut tmp) {
                    Ok(0) => {
                        state.status = FdStatus::Eof;
                        if state.buffer.is_empty() {
                            (SIG_OK, Value::NIL)
                        } else {
                            let data: Vec<u8> = state.buffer.drain(..).collect();
                            Self::bytes_to_value(port, data)
                        }
                    }
                    Ok(n) => {
                        state.buffer.extend_from_slice(&tmp[..n]);
                        let take = count.min(state.buffer.len());
                        let data: Vec<u8> = state.buffer.drain(..take).collect();
                        Self::bytes_to_value(port, data)
                    }
                    Err(e) => {
                        state.status = FdStatus::Error(e.to_string());
                        if state.buffer.is_empty() {
                            (SIG_ERROR, error_val("io-error", e.to_string()))
                        } else {
                            let data: Vec<u8> = state.buffer.drain(..).collect();
                            Self::bytes_to_value(port, data)
                        }
                    }
                }
            }
        }
    }

    fn execute_read_all(&self, port: &Port) -> (SignalBits, Value) {
        if let Err(e) = Self::validate_readable(port) {
            return e;
        }
        let key = Self::port_key(port);
        let mut inner = self.inner.borrow_mut();
        let state = inner.states.entry(key).or_insert_with(FdState::new);

        // Read everything remaining
        if matches!(&state.status, FdStatus::Open) {
            let mut tmp = [0u8; 4096];
            loop {
                match Self::read_from_port(port, &mut tmp) {
                    Ok(0) => {
                        state.status = FdStatus::Eof;
                        break;
                    }
                    Ok(n) => {
                        state.buffer.extend_from_slice(&tmp[..n]);
                    }
                    Err(e) => {
                        state.status = FdStatus::Error(e.to_string());
                        break;
                    }
                }
            }
        }

        let data: Vec<u8> = state.buffer.drain(..).collect();
        Self::bytes_to_value(port, data)
    }

    fn execute_write(&self, port: &Port, data: &Value) -> (SignalBits, Value) {
        if let Err(e) = Self::validate_writable(port) {
            return e;
        }

        // Extract bytes from data
        let bytes: Vec<u8> = if let Some(s) = data.with_string(|s| s.as_bytes().to_vec()) {
            s
        } else if let Some(b) = data.as_bytes() {
            b.to_vec()
        } else if let Some(b) = data.as_blob() {
            b.borrow().clone()
        } else if let Some(b) = data.as_buffer() {
            b.borrow().clone()
        } else {
            // Fall back to Display representation
            let s = format!("{}", data);
            s.into_bytes()
        };

        match Self::write_to_port(port, &bytes) {
            Ok(n) => (SIG_OK, Value::int(n as i64)),
            Err(e) => (SIG_ERROR, error_val("io-error", e.to_string())),
        }
    }

    fn execute_flush(&self, port: &Port) -> (SignalBits, Value) {
        if let Err(e) = Self::validate_writable(port) {
            return e;
        }

        match Self::flush_port(port) {
            Ok(()) => (SIG_OK, Value::NIL),
            Err(e) => (SIG_ERROR, error_val("io-error", e.to_string())),
        }
    }

    /// Read from a port's underlying fd or stdio handle.
    ///
    /// Uses `libc::read` directly on the raw fd to avoid cloning the fd
    /// (which would create a separate file description with dup()).
    fn read_from_port(port: &Port, buf: &mut [u8]) -> io::Result<usize> {
        match port.kind() {
            PortKind::Stdin => io::stdin().lock().read(buf),
            PortKind::Stdout | PortKind::Stderr => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot read from output port",
            )),
            PortKind::File | PortKind::TcpStream | PortKind::UdpSocket | PortKind::UnixStream => {
                port.with_fd(|fd| {
                    let raw = fd.as_raw_fd();
                    let ret = unsafe {
                        libc::read(raw, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
                    };
                    if ret < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(ret as usize)
                    }
                })
                .unwrap_or_else(|| {
                    Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "port fd unavailable",
                    ))
                })
            }
            PortKind::TcpListener | PortKind::UnixListener => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot read from listener port",
            )),
        }
    }

    /// Write to a port's underlying fd or stdio handle.
    ///
    /// Uses `libc::write` directly on the raw fd.
    fn write_to_port(port: &Port, data: &[u8]) -> io::Result<usize> {
        match port.kind() {
            PortKind::Stdout => {
                let mut out = io::stdout().lock();
                let n = out.write(data)?;
                out.flush()?;
                Ok(n)
            }
            PortKind::Stderr => {
                let mut out = io::stderr().lock();
                let n = out.write(data)?;
                out.flush()?;
                Ok(n)
            }
            PortKind::Stdin => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot write to input port",
            )),
            PortKind::File | PortKind::TcpStream | PortKind::UdpSocket | PortKind::UnixStream => {
                port.with_fd(|fd| {
                    let raw = fd.as_raw_fd();
                    let ret = unsafe {
                        libc::write(raw, data.as_ptr() as *const libc::c_void, data.len())
                    };
                    if ret < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(ret as usize)
                    }
                })
                .unwrap_or_else(|| {
                    Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "port fd unavailable",
                    ))
                })
            }
            PortKind::TcpListener | PortKind::UnixListener => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot write to listener port",
            )),
        }
    }

    /// Flush a port's underlying fd or stdio handle.
    fn flush_port(port: &Port) -> io::Result<()> {
        match port.kind() {
            PortKind::Stdout => io::stdout().lock().flush(),
            PortKind::Stderr => io::stderr().lock().flush(),
            PortKind::Stdin => Ok(()), // no-op
            PortKind::File | PortKind::TcpStream | PortKind::UdpSocket | PortKind::UnixStream => {
                port.with_fd(|fd| {
                    let raw = fd.as_raw_fd();
                    let ret = unsafe { libc::fsync(raw) };
                    if ret < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                })
                .unwrap_or_else(|| {
                    Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "port fd unavailable",
                    ))
                })
            }
            PortKind::TcpListener | PortKind::UnixListener => Ok(()), // no-op for listeners
        }
    }

    // --- Network handlers ---

    fn execute_accept(&self, port: &Port) -> (SignalBits, Value) {
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

    fn execute_connect(&self, addr: &ConnectAddr) -> (SignalBits, Value) {
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

    fn execute_send_to(
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
        } else if let Some(b) = data.as_blob() {
            b.borrow().clone()
        } else if let Some(b) = data.as_buffer() {
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

    fn execute_recv_from(&self, port: &Port, count: usize) -> (SignalBits, Value) {
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

    fn execute_shutdown(&self, port: &Port, how: i32) -> (SignalBits, Value) {
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

    /// Convert raw bytes to the appropriate Value based on port encoding.
    fn bytes_to_value(port: &Port, data: Vec<u8>) -> (SignalBits, Value) {
        match port.encoding() {
            Encoding::Text => match String::from_utf8(data) {
                Ok(s) => (SIG_OK, Value::string(s)),
                Err(e) => {
                    // Lossy conversion for text ports
                    let s = String::from_utf8_lossy(e.as_bytes());
                    (SIG_OK, Value::string(s.as_ref()))
                }
            },
            Encoding::Binary => (SIG_OK, Value::bytes(data)),
        }
    }
}

/// Format a sockaddr_storage as a human-readable string.
fn format_sockaddr(addr: &libc::sockaddr_storage, len: libc::socklen_t) -> String {
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
fn parse_sockaddr_ip(addr: &libc::sockaddr_storage, _len: libc::socklen_t) -> (String, u16) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::request::{IoOp, IoRequest};
    use crate::port::{Direction, Encoding, Port};

    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn write_temp_file(content: &str) -> String {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = format!("/tmp/elle-test-backend-{}-{}", std::process::id(), n);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn open_read_port(path: &str) -> Value {
        let file = std::fs::File::open(path).unwrap();
        let fd: std::os::unix::io::OwnedFd = file.into();
        Value::external(
            "port",
            Port::new_file(fd, Direction::Read, Encoding::Text, path.to_string()),
        )
    }

    fn open_write_port(path: &str) -> Value {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap();
        let fd: std::os::unix::io::OwnedFd = file.into();
        Value::external(
            "port",
            Port::new_file(fd, Direction::Write, Encoding::Text, path.to_string()),
        )
    }

    #[test]
    fn test_read_line_basic() {
        let path = write_temp_file("hello\nworld\n");
        let port = open_read_port(&path);
        let backend = SyncBackend::new();

        let req = IoRequest {
            op: IoOp::ReadLine,
            port,
            timeout: None,
        };
        let (bits, val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK);
        val.with_string(|s| assert_eq!(s, "hello")).unwrap();

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_read_line_eof_returns_nil() {
        let path = write_temp_file("");
        let port = open_read_port(&path);
        let backend = SyncBackend::new();

        let req = IoRequest {
            op: IoOp::ReadLine,
            port,
            timeout: None,
        };
        let (bits, val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK);
        assert!(val.is_nil(), "expected nil for EOF, got {:?}", val);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_read_line_no_trailing_newline() {
        let path = write_temp_file("partial");
        let port = open_read_port(&path);
        let backend = SyncBackend::new();

        let req = IoRequest {
            op: IoOp::ReadLine,
            port,
            timeout: None,
        };
        let (bits, val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK);
        val.with_string(|s| assert_eq!(s, "partial")).unwrap();

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_read_all() {
        let path = write_temp_file("hello world");
        let port = open_read_port(&path);
        let backend = SyncBackend::new();

        let req = IoRequest {
            op: IoOp::ReadAll,
            port,
            timeout: None,
        };
        let (bits, val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK);
        val.with_string(|s| assert_eq!(s, "hello world")).unwrap();

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_write_basic() {
        let path = format!("/tmp/elle-test-write-{}", std::process::id());
        let port = open_write_port(&path);
        let backend = SyncBackend::new();

        let req = IoRequest {
            op: IoOp::Write {
                data: Value::string("hello"),
            },
            port,
            timeout: None,
        };
        let (bits, val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK);
        assert_eq!(val.as_int(), Some(5));

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_closed_port_errors() {
        let path = write_temp_file("test");
        let port_val = open_read_port(&path);
        let port = port_val.as_external::<Port>().unwrap();
        port.close();
        let backend = SyncBackend::new();

        let req = IoRequest {
            op: IoOp::ReadLine,
            port: port_val,
            timeout: None,
        };
        let (bits, _) = backend.execute(&req);
        assert_eq!(bits, SIG_ERROR);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_direction_validation() {
        let path = write_temp_file("test");
        let port = open_write_port(&path);
        let backend = SyncBackend::new();

        // Try to read from a write-only port
        let req = IoRequest {
            op: IoOp::ReadLine,
            port,
            timeout: None,
        };
        let (bits, _) = backend.execute(&req);
        assert_eq!(bits, SIG_ERROR);

        std::fs::remove_file(&path).ok();
    }

    // --- Network tests ---

    use crate::io::request::ConnectAddr;

    fn make_tcp_listener() -> (Value, std::net::SocketAddr) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let fd: std::os::unix::io::OwnedFd = listener.into();
        let port = Port::new_tcp_listener(fd, addr.to_string());
        (Value::external("port", port), addr)
    }

    #[test]
    fn test_tcp_connect_to_listener() {
        let (listener_val, addr) = make_tcp_listener();
        // Spawn a thread to accept so connect doesn't hang
        let listener_port = listener_val.as_external::<Port>().unwrap();
        let listener_fd = listener_port.with_fd(|fd| fd.as_raw_fd()).unwrap();
        let accept_thread = std::thread::spawn(move || unsafe {
            let mut sa: libc::sockaddr_storage = std::mem::zeroed();
            let mut sa_len: libc::socklen_t =
                std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
            libc::accept(
                listener_fd,
                &mut sa as *mut _ as *mut libc::sockaddr,
                &mut sa_len,
            )
        });

        let backend = SyncBackend::new();
        let connect_addr = ConnectAddr::Tcp {
            addr: "127.0.0.1".to_string(),
            port: addr.port(),
        };
        let req = IoRequest {
            op: IoOp::Connect { addr: connect_addr },
            port: Value::NIL,
            timeout: None,
        };
        let (bits, val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK, "connect failed");
        assert_eq!(val.external_type_name(), Some("port"));
        let connected = val.as_external::<Port>().unwrap();
        assert_eq!(connected.kind(), PortKind::TcpStream);

        let accepted_fd = accept_thread.join().unwrap();
        if accepted_fd >= 0 {
            unsafe { libc::close(accepted_fd) };
        }
    }

    #[test]
    fn test_tcp_connect_refused_errors() {
        let backend = SyncBackend::new();
        let req = IoRequest {
            op: IoOp::Connect {
                addr: ConnectAddr::Tcp {
                    addr: "127.0.0.1".to_string(),
                    port: 1, // privileged, nobody listening
                },
            },
            port: Value::NIL,
            timeout: None,
        };
        let (bits, _) = backend.execute(&req);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_tcp_accept_on_real_listener() {
        let (listener_val, addr) = make_tcp_listener();
        // Spawn a connecting thread
        let connect_thread = std::thread::spawn(move || {
            std::net::TcpStream::connect(addr).unwrap();
        });

        let backend = SyncBackend::new();
        let req = IoRequest {
            op: IoOp::Accept,
            port: listener_val,
            timeout: None,
        };
        let (bits, val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK, "accept failed");
        assert_eq!(val.external_type_name(), Some("port"));
        let accepted = val.as_external::<Port>().unwrap();
        assert_eq!(accepted.kind(), PortKind::TcpStream);

        connect_thread.join().unwrap();
    }

    #[test]
    fn test_tcp_echo_roundtrip() {
        let (listener_val, addr) = make_tcp_listener();
        let connect_thread = std::thread::spawn(move || {
            let mut stream = std::net::TcpStream::connect(addr).unwrap();
            use std::io::Write;
            stream.write_all(b"hello\n").unwrap();
        });

        let backend = SyncBackend::new();
        let req = IoRequest {
            op: IoOp::Accept,
            port: listener_val,
            timeout: None,
        };
        let (bits, accepted_val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK);

        // Read line from accepted connection
        let req = IoRequest {
            op: IoOp::ReadLine,
            port: accepted_val,
            timeout: None,
        };
        let (bits, val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK);
        val.with_string(|s| assert_eq!(s, "hello")).unwrap();

        connect_thread.join().unwrap();
    }

    #[test]
    fn test_unix_echo_roundtrip() {
        let sock_path = format!("/tmp/elle-test-unix-{}.sock", std::process::id());
        let _ = std::fs::remove_file(&sock_path);

        // Create Unix listener via libc
        let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
        assert!(fd >= 0);
        let mut sun: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        sun.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (i, b) in sock_path.bytes().enumerate() {
            sun.sun_path[i] = b as libc::c_char;
        }
        let addr_len =
            (std::mem::size_of::<libc::sa_family_t>() + sock_path.len() + 1) as libc::socklen_t;
        unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                &1i32 as *const i32 as *const libc::c_void,
                std::mem::size_of::<i32>() as libc::socklen_t,
            );
            libc::bind(fd, &sun as *const _ as *const libc::sockaddr, addr_len);
            libc::listen(fd, 128);
        }
        let owned_fd = unsafe { std::os::unix::io::OwnedFd::from_raw_fd(fd) };
        let listener_val =
            Value::external("port", Port::new_unix_listener(owned_fd, sock_path.clone()));

        let path_clone = sock_path.clone();
        let connect_thread = std::thread::spawn(move || {
            // Connect via libc
            let cfd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
            let mut csun: libc::sockaddr_un = unsafe { std::mem::zeroed() };
            csun.sun_family = libc::AF_UNIX as libc::sa_family_t;
            for (i, b) in path_clone.bytes().enumerate() {
                csun.sun_path[i] = b as libc::c_char;
            }
            let clen = (std::mem::size_of::<libc::sa_family_t>() + path_clone.len() + 1)
                as libc::socklen_t;
            unsafe {
                libc::connect(cfd, &csun as *const _ as *const libc::sockaddr, clen);
                libc::write(cfd, b"unix-hello\n".as_ptr() as *const libc::c_void, 11);
                libc::close(cfd);
            }
        });

        let backend = SyncBackend::new();
        let req = IoRequest {
            op: IoOp::Accept,
            port: listener_val,
            timeout: None,
        };
        let (bits, accepted_val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK);
        let accepted = accepted_val.as_external::<Port>().unwrap();
        assert_eq!(accepted.kind(), PortKind::UnixStream);

        let req = IoRequest {
            op: IoOp::ReadLine,
            port: accepted_val,
            timeout: None,
        };
        let (bits, val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK);
        val.with_string(|s| assert_eq!(s, "unix-hello")).unwrap();

        connect_thread.join().unwrap();
        std::fs::remove_file(&sock_path).ok();
    }

    #[test]
    fn test_udp_send_recv_roundtrip() {
        // Bind two UDP sockets
        let sock_a = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let sock_b = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr_b = sock_b.local_addr().unwrap();

        let fd_a: std::os::unix::io::OwnedFd = sock_a.into();
        let fd_b: std::os::unix::io::OwnedFd = sock_b.into();
        let port_a = Value::external(
            "port",
            Port::new_udp_socket(fd_a, "127.0.0.1:0".to_string()),
        );
        let port_b = Value::external("port", Port::new_udp_socket(fd_b, addr_b.to_string()));

        let backend = SyncBackend::new();

        // Send from A to B
        let req = IoRequest {
            op: IoOp::SendTo {
                addr: "127.0.0.1".to_string(),
                port_num: addr_b.port(),
                data: Value::string("udp-test"),
            },
            port: port_a,
            timeout: None,
        };
        let (bits, val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK);
        assert!(val.as_int().unwrap() > 0);

        // Recv on B
        let req = IoRequest {
            op: IoOp::RecvFrom { count: 1024 },
            port: port_b,
            timeout: None,
        };
        let (bits, val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK);
        // Result is a struct with :data, :addr, :port
        let fields = val.as_struct().unwrap();
        use crate::value::heap::TableKey;
        let data = fields.get(&TableKey::Keyword("data".into())).unwrap();
        let data_bytes = data.as_bytes().unwrap();
        assert_eq!(data_bytes, b"udp-test");
    }

    #[test]
    fn test_shutdown_on_tcp_stream() {
        let (listener_val, addr) = make_tcp_listener();
        let connect_thread = std::thread::spawn(move || {
            let _stream = std::net::TcpStream::connect(addr).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(100));
        });

        let backend = SyncBackend::new();
        let req = IoRequest {
            op: IoOp::Accept,
            port: listener_val,
            timeout: None,
        };
        let (bits, accepted_val) = backend.execute(&req);
        assert_eq!(bits, SIG_OK);

        // Shutdown write side
        let req = IoRequest {
            op: IoOp::Shutdown { how: libc::SHUT_WR },
            port: accepted_val,
            timeout: None,
        };
        let (bits, _) = backend.execute(&req);
        assert_eq!(bits, SIG_OK);

        connect_thread.join().unwrap();
    }

    #[test]
    fn test_stream_read_on_listener_errors() {
        let (listener_val, _addr) = make_tcp_listener();
        let backend = SyncBackend::new();
        let req = IoRequest {
            op: IoOp::ReadLine,
            port: listener_val,
            timeout: None,
        };
        let (bits, _) = backend.execute(&req);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_stream_write_on_udp_errors() {
        let sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let fd: std::os::unix::io::OwnedFd = sock.into();
        let port_val = Value::external("port", Port::new_udp_socket(fd, "127.0.0.1:0".to_string()));
        let backend = SyncBackend::new();
        let req = IoRequest {
            op: IoOp::Write {
                data: Value::string("test"),
            },
            port: port_val,
            timeout: None,
        };
        let (bits, _) = backend.execute(&req);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_accept_on_non_listener_errors() {
        let path = write_temp_file("test");
        let port = open_read_port(&path);
        let backend = SyncBackend::new();
        let req = IoRequest {
            op: IoOp::Accept,
            port,
            timeout: None,
        };
        let (bits, _) = backend.execute(&req);
        assert_eq!(bits, SIG_ERROR);
        std::fs::remove_file(&path).ok();
    }
}
