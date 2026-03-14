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

mod network;
#[cfg(test)]
mod tests;

use crate::io::request::{IoOp, IoRequest};
use crate::io::types::{FdState, FdStatus, PortKey};
use crate::port::{Direction, Encoding, Port, PortKind};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;

struct SyncBackendInner {
    states: HashMap<PortKey, FdState>,
}

/// Synchronous I/O backend. Wrapped as ExternalObject "io-backend".
pub(crate) struct SyncBackend {
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
        // Portless operations — no existing port required.
        if let IoOp::Connect { ref addr } = request.op {
            return self.execute_connect(addr);
        }
        if let IoOp::Sleep { duration } = request.op {
            std::thread::sleep(duration);
            return (SIG_OK, Value::NIL);
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
                IoOp::Connect { .. } | IoOp::Sleep { .. } => unreachable!(), // handled above
                IoOp::SendTo { .. } | IoOp::RecvFrom { .. } => (
                    SIG_ERROR,
                    error_val("io-error", "UDP operations require a UDP socket"),
                ),
            },
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
        let key = PortKey::from_port(port);
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
        let key = PortKey::from_port(port);
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
        let key = PortKey::from_port(port);
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
        } else if let Some(b) = data.as_bytes_mut() {
            b.borrow().clone()
        } else if let Some(b) = data.as_string_mut() {
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
            // Sockets: fsync(2) returns EINVAL on socket fds. TCP and Unix
            // stream sockets have kernel-managed buffers; flush is a no-op.
            PortKind::TcpStream | PortKind::UnixStream => Ok(()),
            PortKind::UdpSocket => Ok(()), // no meaningful flush for UDP
            PortKind::File => port
                .with_fd(|fd| {
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
                }),
            PortKind::TcpListener | PortKind::UnixListener => Ok(()), // no-op for listeners
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
