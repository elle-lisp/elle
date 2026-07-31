//! Shared types for I/O backends.

use crate::port::{Port, PortKind};
use std::os::unix::io::{AsRawFd, RawFd};

/// Identifies a port's underlying resource for state lookup.
#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub(crate) enum PortKey {
    Stdin,
    Stdout,
    Stderr,
    Fd(RawFd),
}

impl PortKey {
    pub(crate) fn from_port(port: &Port) -> PortKey {
        match port.kind() {
            PortKind::Stdin => PortKey::Stdin,
            PortKind::Stdout => PortKey::Stdout,
            PortKind::Stderr => PortKey::Stderr,
            PortKind::File
            | PortKind::TcpListener
            | PortKind::TcpStream
            | PortKind::UdpSocket
            | PortKind::UnixListener
            | PortKind::UnixStream
            | PortKind::Pipe => match port.with_fd(|fd| fd.as_raw_fd()) {
                Some(raw) => PortKey::Fd(raw),
                None => PortKey::Fd(-1),
            },
        }
    }

    /// The underlying file descriptor. The three stdio keys stand for the
    /// POSIX numbers they name; `Fd` carries its own. Backends re-derive the
    /// fd from the key whenever they resubmit an operation, so this mapping
    /// lives in one place.
    pub(crate) fn raw_fd(&self) -> RawFd {
        match self {
            PortKey::Stdin => 0,
            PortKey::Stdout => 1,
            PortKey::Stderr => 2,
            PortKey::Fd(raw) => *raw,
        }
    }
}

/// Per-fd buffered state.
pub(crate) struct FdState {
    pub(crate) buffer: Vec<u8>,
    pub(crate) status: FdStatus,
}

/// Fd lifecycle status.
pub(crate) enum FdStatus {
    Open,
    Eof,
    Error,
}

impl FdState {
    pub(crate) fn new() -> Self {
        FdState {
            buffer: Vec::new(),
            status: FdStatus::Open,
        }
    }
}

#[cfg(test)]
mod tests;
