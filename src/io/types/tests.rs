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
