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

use crate::io::request::{
    IoOp, IoRequest, ProcessHandle, ProcessState, SpawnRequest, StdioDisposition,
};
use crate::io::types::{FdState, FdStatus, PortKey};
use crate::port::{Direction, Encoding, Port, PortKind};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};
use std::os::unix::io::{FromRawFd, OwnedFd};

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

        // Subprocess ops — dispatched before the port guard.
        // Spawn: request.port is Value::NIL (no port needed).
        // ProcessWait: request.port carries a ProcessHandle (not a Port).
        if let IoOp::Spawn(ref req) = request.op {
            return self.execute_spawn(req);
        }
        if let IoOp::ProcessWait = request.op {
            return self.execute_process_wait(&request.port);
        }

        // Task: run closure inline (blocking). Sync backend is single-fiber.
        if let IoOp::Task(ref task_fn) = request.op {
            let closure = match task_fn.take() {
                Some(f) => f,
                None => {
                    return (
                        SIG_ERROR,
                        error_val("task-error", "task closure already consumed"),
                    )
                }
            };
            let (result_code, data) = closure();
            if result_code < 0 {
                let msg = String::from_utf8_lossy(&data).to_string();
                return (SIG_ERROR, error_val("task-error", msg));
            } else {
                return (SIG_OK, Value::bytes(data));
            }
        }

        // Open is portless — it creates a new port rather than operating on one.
        // Sync backend: blocking openat() is fine — single-fiber, no concurrent work to
        // protect. The timeout from IoRequest is intentionally ignored here.
        if let IoOp::Open {
            ref path,
            flags,
            mode,
            direction,
            encoding,
        } = request.op
        {
            return self.execute_open(path, flags, mode, direction, encoding);
        }

        // Resolve is portless — blocking getaddrinfo is fine in the sync backend.
        if let IoOp::Resolve { ref hostname } = request.op {
            use std::net::ToSocketAddrs;
            return match (hostname.as_str(), 0u16).to_socket_addrs() {
                Ok(addrs) => {
                    let ips: Vec<Value> =
                        addrs.map(|a| Value::string(a.ip().to_string())).collect();
                    (SIG_OK, Value::array(ips))
                }
                Err(e) => (
                    SIG_ERROR,
                    error_val("dns-error", format!("resolve {}: {}", hostname, e)),
                ),
            };
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
                IoOp::Seek { offset, whence } => self.execute_seek(port, *offset, *whence),
                IoOp::Tell => self.execute_tell(port),
                IoOp::Accept => (
                    SIG_ERROR,
                    error_val("io-error", "accept: port is not a listener"),
                ),
                IoOp::Connect { .. }
                | IoOp::Sleep { .. }
                | IoOp::Spawn(_)
                | IoOp::ProcessWait
                | IoOp::Open { .. }
                | IoOp::Task(_)
                | IoOp::Resolve { .. }
                | IoOp::WatchNext => unreachable!(), // handled above
                IoOp::SendTo { .. } | IoOp::RecvFrom { .. } => (
                    SIG_ERROR,
                    error_val("io-error", "UDP operations require a UDP socket"),
                ),
                IoOp::Close => {
                    port.close();
                    (SIG_OK, Value::NIL)
                }
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

    fn execute_seek(&self, port: &Port, offset: i64, whence: i32) -> (SignalBits, Value) {
        if port.kind() != PortKind::File {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("port/seek: expected file port, got {:?}", port.kind()),
                ),
            );
        }

        let key = PortKey::from_port(port);
        {
            let mut inner = self.inner.borrow_mut();
            if let Some(state) = inner.states.get_mut(&key) {
                state.buffer.clear();
                state.status = FdStatus::Open;
            }
        }

        match port.with_fd(|fd| {
            let raw = fd.as_raw_fd();
            let ret = unsafe { libc::lseek(raw, offset, whence) };
            if ret < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(ret as i64)
            }
        }) {
            Some(Ok(new_offset)) => (SIG_OK, Value::int(new_offset)),
            Some(Err(e)) => (SIG_ERROR, error_val("io-error", e.to_string())),
            None => (
                SIG_ERROR,
                error_val("io-error", "port/seek: fd unavailable"),
            ),
        }
    }

    fn execute_tell(&self, port: &Port) -> (SignalBits, Value) {
        if port.kind() != PortKind::File {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("port/tell: expected file port, got {:?}", port.kind()),
                ),
            );
        }

        let key = PortKey::from_port(port);
        let buffer_len: i64 = {
            let inner = self.inner.borrow();
            inner
                .states
                .get(&key)
                .map(|state| state.buffer.len() as i64)
                .unwrap_or(0)
        };

        match port.with_fd(|fd| {
            let raw = fd.as_raw_fd();
            let ret = unsafe { libc::lseek(raw, 0, libc::SEEK_CUR) };
            if ret < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(ret as i64)
            }
        }) {
            Some(Ok(kernel_offset)) => (SIG_OK, Value::int(kernel_offset - buffer_len)),
            Some(Err(e)) => (SIG_ERROR, error_val("io-error", e.to_string())),
            None => (
                SIG_ERROR,
                error_val("io-error", "port/tell: fd unavailable"),
            ),
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
            PortKind::File
            | PortKind::TcpStream
            | PortKind::UdpSocket
            | PortKind::UnixStream
            | PortKind::Pipe => port
                .with_fd(|fd| {
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
                }),
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
            PortKind::File
            | PortKind::TcpStream
            | PortKind::UdpSocket
            | PortKind::UnixStream
            | PortKind::Pipe => port
                .with_fd(|fd| {
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
                }),
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
            // Pipes have kernel-managed buffers; flush is a no-op (like sockets).
            PortKind::TcpStream | PortKind::UnixStream | PortKind::Pipe => Ok(()),
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
                    let offset = e.utf8_error().valid_up_to();
                    (
                        SIG_ERROR,
                        error_val(
                            "encoding-error",
                            format!("invalid UTF-8 at byte {}", offset),
                        ),
                    )
                }
            },
            Encoding::Binary => (SIG_OK, Value::bytes(data)),
        }
    }

    #[cfg(test)]
    pub(crate) fn bytes_to_value_pub(port: &Port, data: Vec<u8>) -> (SignalBits, Value) {
        Self::bytes_to_value(port, data)
    }

    fn execute_spawn(&self, req: &SpawnRequest) -> (SignalBits, Value) {
        use crate::value::heap::TableKey;
        use std::process::{Command, Stdio};

        let mut cmd = Command::new(&req.program);
        cmd.args(&req.args);
        if let Some(ref env_pairs) = req.env {
            cmd.env_clear();
            for (k, v) in env_pairs {
                cmd.env(k, v);
            }
        }
        if let Some(ref dir) = req.cwd {
            cmd.current_dir(dir);
        }
        cmd.stdin(match req.stdin {
            StdioDisposition::Pipe => Stdio::piped(),
            StdioDisposition::Inherit => Stdio::inherit(),
            StdioDisposition::Null => Stdio::null(),
        });
        cmd.stdout(match req.stdout {
            StdioDisposition::Pipe => Stdio::piped(),
            StdioDisposition::Inherit => Stdio::inherit(),
            StdioDisposition::Null => Stdio::null(),
        });
        cmd.stderr(match req.stderr {
            StdioDisposition::Pipe => Stdio::piped(),
            StdioDisposition::Inherit => Stdio::inherit(),
            StdioDisposition::Null => Stdio::null(),
        });

        match cmd.spawn() {
            Ok(mut child) => {
                let pid = child.id();
                let stdin_val = child
                    .stdin
                    .take()
                    .map(|s| pipe_to_port(s, Direction::Write, Encoding::Binary, pid, "stdin"))
                    .unwrap_or(Value::NIL);
                let stdout_val = child
                    .stdout
                    .take()
                    .map(|s| pipe_to_port(s, Direction::Read, Encoding::Binary, pid, "stdout"))
                    .unwrap_or(Value::NIL);
                let stderr_val = child
                    .stderr
                    .take()
                    .map(|s| pipe_to_port(s, Direction::Read, Encoding::Binary, pid, "stderr"))
                    .unwrap_or(Value::NIL);

                let handle = ProcessHandle::new(pid, child);
                let handle_val = Value::external("process", handle);

                let mut fields = std::collections::BTreeMap::new();
                fields.insert(TableKey::Keyword("pid".into()), Value::int(pid as i64));
                fields.insert(TableKey::Keyword("stdin".into()), stdin_val);
                fields.insert(TableKey::Keyword("stdout".into()), stdout_val);
                fields.insert(TableKey::Keyword("stderr".into()), stderr_val);
                fields.insert(TableKey::Keyword("process".into()), handle_val);
                (SIG_OK, Value::struct_from(fields))
            }
            Err(e) => (
                SIG_ERROR,
                error_val(
                    "exec-error",
                    format!("subprocess/exec: {}: {}", req.program, e),
                ),
            ),
        }
    }

    fn execute_process_wait(&self, handle_val: &Value) -> (SignalBits, Value) {
        let handle = match handle_val.as_external::<ProcessHandle>() {
            Some(h) => h,
            None => {
                return (
                    SIG_ERROR,
                    error_val("type-error", "subprocess/wait: expected process handle"),
                )
            }
        };
        let mut state = handle.inner.borrow_mut();
        match &mut *state {
            ProcessState::Running(child) => match child.wait() {
                Ok(status) => {
                    let code = status.code().unwrap_or(-1);
                    *state = ProcessState::Exited(code);
                    (SIG_OK, Value::int(code as i64))
                }
                Err(e) => (
                    SIG_ERROR,
                    error_val("exec-error", format!("subprocess/wait: {}", e)),
                ),
            },
            ProcessState::Exited(code) => (SIG_OK, Value::int(*code as i64)),
        }
    }

    /// Execute a file open synchronously via libc::openat.
    ///
    /// The sync backend is used in single-fiber contexts (VM run loop fallback,
    /// test harnesses). Blocking on openat() is acceptable — nothing else is
    /// waiting. The timeout from IoRequest is intentionally ignored.
    fn execute_open(
        &self,
        path: &str,
        flags: i32,
        mode: u32,
        direction: Direction,
        encoding: Encoding,
    ) -> (SignalBits, Value) {
        let c_path = match std::ffi::CString::new(path) {
            Ok(p) => p,
            Err(_) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "io-error",
                        format!("port/open: {}: invalid path (contains null byte)", path),
                    ),
                )
            }
        };
        let fd =
            unsafe { libc::openat(libc::AT_FDCWD, c_path.as_ptr(), flags, mode as libc::c_uint) };
        if fd < 0 {
            let err = std::io::Error::last_os_error();
            return (
                SIG_ERROR,
                error_val("io-error", format!("port/open: {}: {}", path, err)),
            );
        }
        // SAFETY: fd is a valid file descriptor returned by openat on success.
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        let port = Port::new_file(owned, direction, encoding, path.to_string());
        (SIG_OK, Value::external("port", port))
    }
}

/// Convert a subprocess pipe (ChildStdin, ChildStdout, ChildStderr) to a Port Value.
///
/// `T: Into<OwnedFd>` covers ChildStdin, ChildStdout, ChildStderr — each implements
/// `From<T> for OwnedFd` on Unix via IntoRawFd.
pub(crate) fn pipe_to_port<T: Into<std::os::unix::io::OwnedFd>>(
    pipe: T,
    direction: Direction,
    encoding: Encoding,
    pid: u32,
    name: &str,
) -> Value {
    let fd: std::os::unix::io::OwnedFd = pipe.into();
    let label = format!("pid:{}:{}", pid, name);
    Value::external("port", Port::new_pipe(fd, direction, encoding, label))
}
