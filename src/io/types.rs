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
            | PortKind::UnixStream => match port.with_fd(|fd| fd.as_raw_fd()) {
                Some(raw) => PortKey::Fd(raw),
                None => PortKey::Fd(-1),
            },
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
    Error(String),
}

impl FdState {
    pub(crate) fn new() -> Self {
        FdState {
            buffer: Vec::new(),
            status: FdStatus::Open,
        }
    }
}
