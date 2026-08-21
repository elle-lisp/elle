//! Shared types for I/O backends.

use crate::port::{Port, PortId, PortKind};
use std::collections::HashMap;
use std::os::unix::io::{AsRawFd, RawFd};

/// Identifies a port's underlying resource for state lookup.
///
/// A `Fd` key names the port instance as well as the descriptor number, because
/// the number alone is not an identity: the OS hands it out again as soon as the
/// descriptor closes ([`PortId`]). The three stdio keys carry no identity — their
/// descriptors are process-wide and outlive every `Port` object that names them,
/// so two stdin ports genuinely share one read position.
#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub(crate) enum PortKey {
    Stdin,
    Stdout,
    Stderr,
    Fd(RawFd, PortId),
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
            | PortKind::Pipe => {
                PortKey::Fd(port.with_fd(|fd| fd.as_raw_fd()).unwrap_or(-1), port.id())
            }
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
            PortKey::Fd(raw, _) => *raw,
        }
    }

    /// Does this key name descriptor number `raw` through a port of its own?
    /// The stdio keys answer `false` for the numbers they stand for: closing a
    /// port on fd 0/1/2 does not hand the number back to the OS, so nothing
    /// about it is stale.
    pub(crate) fn names_fd(&self, raw: RawFd) -> bool {
        matches!(self, PortKey::Fd(f, _) if *f == raw)
    }
}

/// Bytes read past what an operation asked for, held for the next read on
/// the same descriptor. A `ReadLine` that overshoots the newline and a
/// `ReadExact` that overshoots the grapheme count both leave a remainder
/// here, and the next operation on the key consumes it before it reaches
/// the kernel.
pub(crate) struct FdState {
    pub(crate) buffer: Vec<u8>,
}

impl FdState {
    pub(crate) fn new() -> Self {
        FdState { buffer: Vec::new() }
    }
}

/// The remainder held for `key`, created empty when there is none.
///
/// Creating one first discards every entry naming the same descriptor NUMBER,
/// and that is what keeps a remainder from crossing between two ports. A number
/// only comes back to the OS once its descriptor is closed, so a key that is new
/// for a number the map already knows proves the previous owner is gone — and
/// that owner may have gone without saying so, since a port dropped rather than
/// `port/close`d closes its descriptor through `OwnedFd` with no backend in
/// reach. At most one entry per live descriptor survives, and the newcomer never
/// reads the previous owner's bytes (`tests/elle/io.lisp` § "a recycled
/// descriptor number carries no remainder").
pub(crate) fn fd_state_mut<'a>(
    states: &'a mut HashMap<PortKey, FdState>,
    key: &PortKey,
) -> &'a mut FdState {
    if !states.contains_key(key) {
        if let PortKey::Fd(raw, _) = *key {
            discard_fd_state(states, raw);
        }
    }
    states.entry(key.clone()).or_insert_with(FdState::new)
}

/// Discard the remainder held for descriptor number `raw`, whichever port left
/// it. Called where a close is routed through the backend and the number is
/// known to be going back to the OS, and by [`fd_state_mut`] where a new key for
/// a known number says the same thing after the fact.
pub(crate) fn discard_fd_state(states: &mut HashMap<PortKey, FdState>, raw: RawFd) {
    states.retain(|k, _| !k.names_fd(raw));
}

#[cfg(test)]
mod tests;
