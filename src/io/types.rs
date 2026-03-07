//! Shared types for I/O backends.

use std::os::unix::io::RawFd;

/// Identifies a port's underlying resource for state lookup.
#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub(crate) enum PortKey {
    Stdin,
    Stdout,
    Stderr,
    Fd(RawFd),
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
