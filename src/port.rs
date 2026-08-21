//! Port type — Elle's abstraction for file descriptors.
//!
//! A port wraps an OS file descriptor with metadata (direction, encoding,
//! kind) and lifecycle management. Ports are represented as ExternalObject
//! values with type_name "port".

use std::cell::{Cell, RefCell};
use std::fmt;
use std::os::unix::io::OwnedFd;
use std::sync::atomic::{AtomicU64, Ordering};

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

/// A port's identity, minted once per `Port` and never reused.
///
/// A descriptor NUMBER is not an identity: the OS hands it out again as soon as
/// the descriptor closes, so two ports that never coexisted can carry the same
/// number. Per-descriptor state the I/O backend holds (`io::types::FdState`, the
/// bytes a read left over) is keyed by number *and* identity, so the next port on
/// a recycled number cannot reach what the previous one left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct PortId(u64);

impl PortId {
    /// The next unused identity. Process-wide and monotonic, so two instances on
    /// two threads never mint the same one.
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        PortId(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    /// A standalone identity for a test that builds a `PortKey` without a port.
    #[cfg(test)]
    pub(crate) fn fresh() -> Self {
        Self::next()
    }
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
    id: PortId,
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
    /// The one place a `Port` is built. Every named constructor below differs
    /// only in its descriptor, kind, direction, encoding, and path, so each of
    /// them is a call to this — which is what makes a port's identity
    /// unforgeable: there is no other way to make one.
    fn build(
        fd: Option<OwnedFd>,
        kind: PortKind,
        direction: Direction,
        encoding: Encoding,
        path: Option<String>,
    ) -> Self {
        Port {
            id: PortId::next(),
            fd: RefCell::new(fd),
            kind,
            direction,
            encoding,
            closed: Cell::new(false),
            path,
            timeout: Cell::new(None),
        }
    }

    /// This port's identity — see [`PortId`].
    pub(crate) fn id(&self) -> PortId {
        self.id
    }

    /// Create a file port from an owned fd.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new_file(fd: OwnedFd, direction: Direction, encoding: Encoding, path: String) -> Self {
        Self::build(Some(fd), PortKind::File, direction, encoding, Some(path))
    }

    /// Create an unopened port (fd filled in later by IO completion).
    pub fn new_unopened(
        kind: PortKind,
        direction: Direction,
        encoding: Encoding,
        path: String,
    ) -> Self {
        Self::build(None, kind, direction, encoding, Some(path))
    }

    /// Set the fd on an unopened port (called by IO completion).
    pub fn set_fd(&self, fd: OwnedFd) {
        *self.fd.borrow_mut() = Some(fd);
    }

    /// Create a stdin port. Does not own the fd.
    pub fn stdin() -> Self {
        Self::build(None, PortKind::Stdin, Direction::Read, Encoding::Text, None)
    }

    /// Create a stdout port. Does not own the fd.
    pub fn stdout() -> Self {
        Self::build(
            None,
            PortKind::Stdout,
            Direction::Write,
            Encoding::Text,
            None,
        )
    }

    /// Create a stderr port. Does not own the fd.
    pub fn stderr() -> Self {
        Self::build(
            None,
            PortKind::Stderr,
            Direction::Write,
            Encoding::Text,
            None,
        )
    }

    pub fn new_tcp_listener(fd: OwnedFd, bound_addr: String) -> Self {
        Self::build(
            Some(fd),
            PortKind::TcpListener,
            Direction::Read,
            Encoding::Text,
            Some(bound_addr),
        )
    }

    /// Binary encoding: TCP is a byte stream. `port/read` returns bytes,
    /// enabling binary protocols (TLS, msgpack, custom wire formats).
    /// `port/write` accepts both bytes and strings, and `port/read-line`
    /// always returns a string regardless of encoding.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new_tcp_stream(fd: OwnedFd, peer_addr: String) -> Self {
        Self::build(
            Some(fd),
            PortKind::TcpStream,
            Direction::ReadWrite,
            Encoding::Binary,
            Some(peer_addr),
        )
    }

    pub fn new_udp_socket(fd: OwnedFd, bound_addr: String) -> Self {
        Self::build(
            Some(fd),
            PortKind::UdpSocket,
            Direction::ReadWrite,
            Encoding::Binary,
            Some(bound_addr),
        )
    }

    pub fn new_unix_listener(fd: OwnedFd, path: String) -> Self {
        Self::build(
            Some(fd),
            PortKind::UnixListener,
            Direction::Read,
            Encoding::Text,
            Some(path),
        )
    }

    /// Binary encoding: Unix streams are byte streams, same as TCP, so
    /// `port/read` returns bytes for binary protocols (h2, gRPC, and the like).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new_unix_stream(fd: OwnedFd, peer_path: String) -> Self {
        Self::build(
            Some(fd),
            PortKind::UnixStream,
            Direction::ReadWrite,
            Encoding::Binary,
            Some(peer_path),
        )
    }

    /// Create a pipe port from a subprocess stdio fd.
    ///
    /// `label` is displayed as the path: `"pid:1234:stdout"` etc.
    /// Encoding is always Binary — subprocess output is an arbitrary byte
    /// stream. Text decoding is the caller's responsibility.
    pub fn new_pipe(fd: OwnedFd, direction: Direction, encoding: Encoding, label: String) -> Self {
        Self::build(Some(fd), PortKind::Pipe, direction, encoding, Some(label))
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

    /// Mark the port closed and hand its descriptor to the caller instead of
    /// dropping it. The caller then decides when the descriptor is handed back
    /// to the OS — which the I/O backend delays while an operation still names
    /// it, so the number cannot be given to a new port under a running worker.
    /// Returns `None` for a port that owns no descriptor (stdio) or is already
    /// closed.
    pub(crate) fn retire_fd(&self) -> Option<OwnedFd> {
        if self.closed.get() {
            return None;
        }
        let fd = self.fd.borrow_mut().take()?;
        self.closed.set(true);
        Some(fd)
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

    /// Builder: set the port's encoding.  Used by `tcp/connect` /
    /// `tcp/accept` / `unix/connect` / `unix/accept` to honor an
    /// explicit `:encoding text|binary` keyword override; the raw
    /// stream constructors default to `Binary` (the bytes-stream
    /// interpretation of POSIX sockets) but line-oriented text
    /// protocols (SMTP, IRC, plain HTTP/1.x) want `Text`.
    #[allow(dead_code)]
    pub fn with_encoding(mut self, enc: Encoding) -> Self {
        self.encoding = enc;
        self
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
mod tests;
