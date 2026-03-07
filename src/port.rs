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
pub enum PortKind {
    File,
    Stdin,
    Stdout,
    Stderr,
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
pub struct Port {
    fd: RefCell<Option<OwnedFd>>,
    kind: PortKind,
    direction: Direction,
    encoding: Encoding,
    closed: Cell<bool>,
    /// Original path for file ports (display and error messages).
    path: Option<String>,
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

    /// The port kind.
    pub fn kind(&self) -> PortKind {
        self.kind
    }

    /// The port direction.
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
}
