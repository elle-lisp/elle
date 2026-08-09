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
    ///
    /// Only `io::uring::drain` resubmits from a key, so a release build on the
    /// pool platform links no caller; the unit tests below still cover it
    /// everywhere.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn raw_fd(&self) -> RawFd {
        match self {
            PortKey::Stdin => 0,
            PortKey::Stdout => 1,
            PortKey::Stderr => 2,
            PortKey::Fd(raw) => *raw,
        }
    }
}

/// Bytes read past what an operation asked for, held for the next read on
/// the same descriptor. A `ReadLine` that overshoots the newline and a
/// `ReadExact` that overshoots the grapheme count both leave a remainder
/// here, and the next operation on the key consumes it before it reaches
/// the kernel. The entry is dropped when the descriptor closes, so the
/// remainder never spans two owners of one descriptor number.
pub(crate) struct FdState {
    pub(crate) buffer: Vec<u8>,
}

impl FdState {
    pub(crate) fn new() -> Self {
        FdState { buffer: Vec::new() }
    }
}

#[cfg(test)]
mod tests;
