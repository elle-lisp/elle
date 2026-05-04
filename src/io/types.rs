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
}

/// Per-fd buffered state.
pub(crate) struct FdState {
    pub(crate) buffer: Vec<u8>,
}

impl FdState {
    pub(crate) fn new() -> Self {
        FdState { buffer: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port::{Direction, Encoding, Port};
    use std::fs::File;
    use std::os::unix::io::OwnedFd;

    #[test]
    fn test_port_key_from_pipe() {
        let file = File::open("/dev/null").unwrap();
        let fd: OwnedFd = file.into();
        let p = Port::new_pipe(
            fd,
            Direction::Read,
            Encoding::Binary,
            "pid:1:stdout".to_string(),
        );
        // Must not panic; must return an Fd variant (not Stdin/Stdout/Stderr).
        let key = PortKey::from_port(&p);
        assert!(matches!(key, PortKey::Fd(_)));
    }
}
