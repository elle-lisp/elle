//! Port type — Elle's abstraction for file descriptors.
//!
//! A port wraps an OS file descriptor with metadata (direction, encoding,
//! kind) and lifecycle management. Ports are represented as ExternalObject
//! values with type_name "port".

use std::cell::{Cell, RefCell};
use std::fmt;
use std::os::unix::io::OwnedFd;

/// The kind of underlying OS resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortKind {
    File,
    Stdin,
    Stdout,
    Stderr,
    TcpListener,
    TcpStream,
    UdpSocket,
    UnixListener,
    UnixStream,
    Pipe, // subprocess stdin/stdout/stderr pipe fd
}

/// Which operations are permitted on this port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Read,
    Write,
    ReadWrite,
}

/// How bytes are interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Text,   // UTF-8
    Binary, // raw bytes
}

/// A port wrapping an OS file descriptor.
///
/// Wrapped in `ExternalObject` via `Value::external("port", port)`.
/// Access from primitives via `value.as_external::<Port>()`.
///
/// # Lifecycle
///
/// File ports own their fd via `OwnedFd` in `RefCell<Option<OwnedFd>>`.
/// `port/close` takes the fd out (dropping it closes the fd). Default
/// Drop does the same if close wasn't called explicitly.
///
/// Stdio ports do NOT own their fd — `fd` is `None` from construction.
/// `port/close` on a stdio port just sets the `closed` flag. Drop is
/// a no-op (nothing to drop when `fd` is `None`).
pub(crate) struct Port {
    fd: RefCell<Option<OwnedFd>>,
    kind: PortKind,
    direction: Direction,
    encoding: Encoding,
    closed: Cell<bool>,
    /// Original path for file ports (display and error messages).
    path: Option<String>,
    timeout: Cell<Option<u64>>, // milliseconds, set by port/set-options
}

impl Port {
    /// Create a file port from an owned fd.
    pub fn new_file(fd: OwnedFd, direction: Direction, encoding: Encoding, path: String) -> Self {
        Port {
            fd: RefCell::new(Some(fd)),
            kind: PortKind::File,
            direction,
            encoding,
            closed: Cell::new(false),
            path: Some(path),
            timeout: Cell::new(None),
        }
    }

    /// Create a stdin port. Does not own the fd.
    pub fn stdin() -> Self {
        Port {
            fd: RefCell::new(None),
            kind: PortKind::Stdin,
            direction: Direction::Read,
            encoding: Encoding::Text,
            closed: Cell::new(false),
            path: None,
            timeout: Cell::new(None),
        }
    }

    /// Create a stdout port. Does not own the fd.
    pub fn stdout() -> Self {
        Port {
            fd: RefCell::new(None),
            kind: PortKind::Stdout,
            direction: Direction::Write,
            encoding: Encoding::Text,
            closed: Cell::new(false),
            path: None,
            timeout: Cell::new(None),
        }
    }

    /// Create a stderr port. Does not own the fd.
    pub fn stderr() -> Self {
        Port {
            fd: RefCell::new(None),
            kind: PortKind::Stderr,
            direction: Direction::Write,
            encoding: Encoding::Text,
            closed: Cell::new(false),
            path: None,
            timeout: Cell::new(None),
        }
    }

    pub fn new_tcp_listener(fd: OwnedFd, bound_addr: String) -> Self {
        Port {
            fd: RefCell::new(Some(fd)),
            kind: PortKind::TcpListener,
            direction: Direction::Read,
            encoding: Encoding::Text,
            closed: Cell::new(false),
            path: Some(bound_addr),
            timeout: Cell::new(None),
        }
    }

    pub fn new_tcp_stream(fd: OwnedFd, peer_addr: String) -> Self {
        Port {
            fd: RefCell::new(Some(fd)),
            kind: PortKind::TcpStream,
            direction: Direction::ReadWrite,
            // Binary encoding: TCP is a byte stream. port/read returns bytes,
            // enabling binary protocols (TLS, msgpack, custom wire formats).
            // port/write accepts both bytes and strings.
            // port/read-line always returns a string regardless of encoding.
            encoding: Encoding::Binary,
            closed: Cell::new(false),
            path: Some(peer_addr),
            timeout: Cell::new(None),
        }
    }

    pub fn new_udp_socket(fd: OwnedFd, bound_addr: String) -> Self {
        Port {
            fd: RefCell::new(Some(fd)),
            kind: PortKind::UdpSocket,
            direction: Direction::ReadWrite,
            encoding: Encoding::Binary,
            closed: Cell::new(false),
            path: Some(bound_addr),
            timeout: Cell::new(None),
        }
    }

    pub fn new_unix_listener(fd: OwnedFd, path: String) -> Self {
        Port {
            fd: RefCell::new(Some(fd)),
            kind: PortKind::UnixListener,
            direction: Direction::Read,
            encoding: Encoding::Text,
            closed: Cell::new(false),
            path: Some(path),
            timeout: Cell::new(None),
        }
    }

    pub fn new_unix_stream(fd: OwnedFd, peer_path: String) -> Self {
        Port {
            fd: RefCell::new(Some(fd)),
            kind: PortKind::UnixStream,
            direction: Direction::ReadWrite,
            encoding: Encoding::Text,
            closed: Cell::new(false),
            path: Some(peer_path),
            timeout: Cell::new(None),
        }
    }

    /// Create a pipe port from a subprocess stdio fd.
    ///
    /// `label` is displayed as the path: `"pid:1234:stdout"` etc.
    /// Encoding is always Binary — subprocess output is an arbitrary byte
    /// stream. Text decoding is the caller's responsibility.
    pub fn new_pipe(fd: OwnedFd, direction: Direction, encoding: Encoding, label: String) -> Self {
        Port {
            fd: RefCell::new(Some(fd)),
            kind: PortKind::Pipe,
            direction,
            encoding,
            closed: Cell::new(false),
            path: Some(label),
            timeout: Cell::new(None),
        }
    }

    /// Close the port. Idempotent.
    ///
    /// For file ports: takes the `OwnedFd` out, dropping it (closes fd).
    /// For stdio ports: sets `closed` flag only (does NOT close the OS fd).
    pub fn close(&self) {
        if !self.closed.get() {
            // For file ports, take the OwnedFd out (drop closes it).
            // For stdio ports, fd is already None — take() is a no-op.
            self.fd.borrow_mut().take();
            self.closed.set(true);
        }
    }

    /// Whether this port has been closed.
    pub fn is_closed(&self) -> bool {
        self.closed.get()
    }

    /// Whether this port owns a file descriptor.
    /// Stdio ports don't own their fd (fd is None from construction).
    pub fn has_fd(&self) -> bool {
        self.fd.borrow().is_some()
    }

    /// The port kind.
    pub fn kind(&self) -> PortKind {
        self.kind
    }

    /// The port direction.
    #[cfg(test)]
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// The port encoding.
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// The original file path, if this is a file port.
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    #[cfg(test)]
    pub fn timeout_ms(&self) -> Option<u64> {
        self.timeout.get()
    }

    pub fn set_timeout_ms(&self, ms: Option<u64>) {
        self.timeout.set(ms);
    }

    /// Borrow the fd for I/O operations.
    ///
    /// Returns `None` if the port is closed or is a stdio port (stdio
    /// ports have `fd: None` — callers should use `std::io::stdin()` /
    /// `stdout()` / `stderr()` handles directly for those).
    pub fn with_fd<R>(&self, f: impl FnOnce(&OwnedFd) -> R) -> Option<R> {
        if self.closed.get() {
            return None;
        }
        let borrow = self.fd.borrow();
        borrow.as_ref().map(f)
    }
}

impl fmt::Display for Port {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            PortKind::Stdin => {
                write!(f, "#<port:stdin")?;
                if self.closed.get() {
                    write!(f, " [closed]")?;
                }
                write!(f, ">")
            }
            PortKind::Stdout => {
                write!(f, "#<port:stdout")?;
                if self.closed.get() {
                    write!(f, " [closed]")?;
                }
                write!(f, ">")
            }
            PortKind::Stderr => {
                write!(f, "#<port:stderr")?;
                if self.closed.get() {
                    write!(f, " [closed]")?;
                }
                write!(f, ">")
            }
            PortKind::File => {
                write!(f, "#<port:file")?;
                if let Some(ref path) = self.path {
                    write!(f, " \"{}\"", path)?;
                }
                match self.direction {
                    Direction::Read => write!(f, " :read")?,
                    Direction::Write => write!(f, " :write")?,
                    Direction::ReadWrite => write!(f, " :read-write")?,
                }
                match self.encoding {
                    Encoding::Text => write!(f, " :text")?,
                    Encoding::Binary => write!(f, " :binary")?,
                }
                if self.closed.get() {
                    write!(f, " [closed]")?;
                }
                write!(f, ">")
            }
            PortKind::TcpListener => {
                write!(f, "#<port:tcp-listener")?;
                if let Some(ref addr) = self.path {
                    write!(f, " \"{}\"", addr)?;
                }
                if self.closed.get() {
                    write!(f, " [closed]")?;
                }
                write!(f, ">")
            }
            PortKind::TcpStream => {
                write!(f, "#<port:tcp-stream")?;
                if let Some(ref addr) = self.path {
                    write!(f, " \"{}\"", addr)?;
                }
                write!(f, " :read-write :text")?;
                if self.closed.get() {
                    write!(f, " [closed]")?;
                }
                write!(f, ">")
            }
            PortKind::UdpSocket => {
                write!(f, "#<port:udp")?;
                if let Some(ref addr) = self.path {
                    write!(f, " \"{}\"", addr)?;
                }
                write!(f, " :read-write :binary")?;
                if self.closed.get() {
                    write!(f, " [closed]")?;
                }
                write!(f, ">")
            }
            PortKind::UnixListener => {
                write!(f, "#<port:unix-listener")?;
                if let Some(ref path) = self.path {
                    write!(f, " \"{}\"", path)?;
                }
                if self.closed.get() {
                    write!(f, " [closed]")?;
                }
                write!(f, ">")
            }
            PortKind::UnixStream => {
                write!(f, "#<port:unix-stream")?;
                if let Some(ref path) = self.path {
                    write!(f, " \"{}\"", path)?;
                }
                write!(f, " :read-write :text")?;
                if self.closed.get() {
                    write!(f, " [closed]")?;
                }
                write!(f, ">")
            }
            PortKind::Pipe => {
                write!(f, "#<port:pipe")?;
                if let Some(ref path) = self.path {
                    write!(f, " \"{}\"", path)?;
                }
                match self.direction {
                    Direction::Read => write!(f, " :read")?,
                    Direction::Write => write!(f, " :write")?,
                    Direction::ReadWrite => write!(f, " :read-write")?,
                }
                match self.encoding {
                    Encoding::Text => write!(f, " :text")?,
                    Encoding::Binary => write!(f, " :binary")?,
                }
                if self.closed.get() {
                    write!(f, " [closed]")?;
                }
                write!(f, ">")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::os::unix::io::OwnedFd;

    #[test]
    fn test_with_fd_file_port() {
        let file = File::open("/dev/null").unwrap();
        let fd: OwnedFd = file.into();
        let port = Port::new_file(fd, Direction::Read, Encoding::Text, "/dev/null".to_string());

        // with_fd should return Some
        let result = port.with_fd(|fd| {
            use std::os::unix::io::AsRawFd;
            fd.as_raw_fd()
        });
        assert!(result.is_some());
    }

    #[test]
    fn test_with_fd_closed_port() {
        let file = File::open("/dev/null").unwrap();
        let fd: OwnedFd = file.into();
        let port = Port::new_file(fd, Direction::Read, Encoding::Text, "/dev/null".to_string());
        port.close();
        assert!(port.with_fd(|_| ()).is_none());
    }

    #[test]
    fn test_with_fd_stdio_port() {
        let port = Port::stdin();
        // Stdio ports have fd: None, so with_fd returns None
        assert!(port.with_fd(|_| ()).is_none());
    }

    fn devnull_fd() -> OwnedFd {
        File::open("/dev/null").unwrap().into()
    }

    #[test]
    fn test_new_tcp_listener_kind() {
        let p = Port::new_tcp_listener(devnull_fd(), "127.0.0.1:8080".into());
        assert_eq!(p.kind(), PortKind::TcpListener);
        assert_eq!(p.direction(), Direction::Read);
    }

    #[test]
    fn test_new_tcp_stream_kind() {
        let p = Port::new_tcp_stream(devnull_fd(), "127.0.0.1:8080".into());
        assert_eq!(p.kind(), PortKind::TcpStream);
        assert_eq!(p.direction(), Direction::ReadWrite);
        assert_eq!(p.encoding(), Encoding::Binary);
    }

    #[test]
    fn test_new_udp_socket_kind() {
        let p = Port::new_udp_socket(devnull_fd(), "0.0.0.0:9000".into());
        assert_eq!(p.kind(), PortKind::UdpSocket);
        assert_eq!(p.encoding(), Encoding::Binary);
    }

    #[test]
    fn test_new_unix_listener_kind() {
        let p = Port::new_unix_listener(devnull_fd(), "/tmp/test.sock".into());
        assert_eq!(p.kind(), PortKind::UnixListener);
    }

    #[test]
    fn test_new_unix_stream_kind() {
        let p = Port::new_unix_stream(devnull_fd(), "/tmp/test.sock".into());
        assert_eq!(p.kind(), PortKind::UnixStream);
        assert_eq!(p.encoding(), Encoding::Text);
    }

    #[test]
    fn test_tcp_listener_display() {
        let p = Port::new_tcp_listener(devnull_fd(), "127.0.0.1:8080".into());
        assert!(format!("{}", p).contains("tcp-listener"));
    }

    #[test]
    fn test_port_timeout_default_none() {
        let p = Port::new_tcp_stream(devnull_fd(), "x".into());
        assert_eq!(p.timeout_ms(), None);
    }

    #[test]
    fn test_port_timeout_get_set() {
        let p = Port::new_tcp_stream(devnull_fd(), "x".into());
        p.set_timeout_ms(Some(5000));
        assert_eq!(p.timeout_ms(), Some(5000));
        p.set_timeout_ms(None);
        assert_eq!(p.timeout_ms(), None);
    }

    #[test]
    fn test_new_pipe_kind() {
        let file = File::open("/dev/null").unwrap();
        let fd: OwnedFd = file.into();
        let p = Port::new_pipe(
            fd,
            Direction::Read,
            Encoding::Binary,
            "pid:42:stdout".to_string(),
        );
        assert_eq!(p.kind(), PortKind::Pipe);
        assert_eq!(p.direction(), Direction::Read);
        assert_eq!(p.encoding(), Encoding::Binary);
        assert_eq!(p.path(), Some("pid:42:stdout"));
    }

    #[test]
    fn test_pipe_display_binary() {
        let file = File::open("/dev/null").unwrap();
        let fd: OwnedFd = file.into();
        let p = Port::new_pipe(
            fd,
            Direction::Read,
            Encoding::Binary,
            "pid:1234:stdout".to_string(),
        );
        let s = format!("{}", p);
        assert!(s.contains("pipe"), "display: {}", s);
        assert!(s.contains("pid:1234:stdout"), "display: {}", s);
        assert!(s.contains(":read"), "display: {}", s);
        assert!(s.contains(":binary"), "display: {}", s);
    }

    #[test]
    fn test_pipe_display_write() {
        let file = File::open("/dev/null").unwrap();
        let fd: OwnedFd = file.into();
        let p = Port::new_pipe(
            fd,
            Direction::Write,
            Encoding::Binary,
            "pid:5:stdin".to_string(),
        );
        let s = format!("{}", p);
        assert!(s.contains(":write"), "display: {}", s);
    }

    #[test]
    fn test_pipe_display_closed() {
        let file = File::open("/dev/null").unwrap();
        let fd: OwnedFd = file.into();
        let p = Port::new_pipe(
            fd,
            Direction::Read,
            Encoding::Binary,
            "pid:1:stdout".to_string(),
        );
        p.close();
        assert!(format!("{}", p).contains("[closed]"));
    }
}
