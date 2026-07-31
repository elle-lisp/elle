//! Unit tests (`super` is the parent impl module).

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

/// The backends re-derive an fd from its key on every resubmission, so the
/// stdio keys must map to the POSIX numbers they name.
#[test]
fn port_key_raw_fd_maps_stdio_to_posix_numbers() {
    assert_eq!(PortKey::Stdin.raw_fd(), 0);
    assert_eq!(PortKey::Stdout.raw_fd(), 1);
    assert_eq!(PortKey::Stderr.raw_fd(), 2);
    assert_eq!(PortKey::Fd(37).raw_fd(), 37);
}
