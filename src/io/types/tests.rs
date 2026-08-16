//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::port::{Direction, Encoding, Port, PortId};
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
    assert!(matches!(key, PortKey::Fd(_, _)));
}

/// The backends re-derive an fd from its key on every resubmission, so the
/// stdio keys must map to the POSIX numbers they name.
#[test]
fn port_key_raw_fd_maps_stdio_to_posix_numbers() {
    assert_eq!(PortKey::Stdin.raw_fd(), 0);
    assert_eq!(PortKey::Stdout.raw_fd(), 1);
    assert_eq!(PortKey::Stderr.raw_fd(), 2);
    assert_eq!(PortKey::Fd(37, PortId::fresh()).raw_fd(), 37);
}

/// Two ports on one descriptor number are two owners, never one: the number
/// only comes back to the OS after the first port's descriptor closed. So the
/// remainder the first left is unreachable to the second, and the map keeps one
/// entry per live descriptor rather than one per port that ever held the number.
#[test]
fn a_remainder_does_not_cross_between_two_ports_on_one_descriptor_number() {
    let mut states: HashMap<PortKey, FdState> = HashMap::new();
    let first = PortKey::Fd(7, PortId::fresh());
    let second = PortKey::Fd(7, PortId::fresh());

    fd_state_mut(&mut states, &first)
        .buffer
        .extend_from_slice(b"leftover");

    assert!(
        fd_state_mut(&mut states, &second).buffer.is_empty(),
        "the second port on descriptor 7 starts with no remainder"
    );
    assert_eq!(
        states.len(),
        1,
        "the first port's entry goes with its descriptor"
    );
}

/// The eviction is keyed on the descriptor NUMBER, so a port on a different
/// number keeps its own remainder across the newcomer's arrival.
#[test]
fn a_remainder_survives_a_new_port_on_a_different_number() {
    let mut states: HashMap<PortKey, FdState> = HashMap::new();
    let seven = PortKey::Fd(7, PortId::fresh());
    let eight = PortKey::Fd(8, PortId::fresh());

    fd_state_mut(&mut states, &seven)
        .buffer
        .extend_from_slice(b"seven");
    fd_state_mut(&mut states, &eight)
        .buffer
        .extend_from_slice(b"eight");

    assert_eq!(fd_state_mut(&mut states, &seven).buffer, b"seven");
    assert_eq!(fd_state_mut(&mut states, &eight).buffer, b"eight");
}

/// Re-reading the same port's state hands back what that port left, which is
/// the whole point of holding a remainder: a `ReadLine` that overshoots the
/// newline funds the next read on the same port.
#[test]
fn a_port_reads_back_the_remainder_it_left() {
    let mut states: HashMap<PortKey, FdState> = HashMap::new();
    let key = PortKey::Fd(7, PortId::fresh());

    fd_state_mut(&mut states, &key)
        .buffer
        .extend_from_slice(b"line2\n");

    assert_eq!(fd_state_mut(&mut states, &key).buffer, b"line2\n");
}

/// Closing a stdio port does not hand its number back to the OS, so a port that
/// happens to own descriptor 0/1/2 must not evict the stdio remainder — and
/// `names_fd` is where that distinction lives.
#[test]
fn a_stdio_key_names_no_recyclable_descriptor() {
    assert!(!PortKey::Stdin.names_fd(0));
    assert!(!PortKey::Stdout.names_fd(1));
    assert!(!PortKey::Stderr.names_fd(2));
    assert!(PortKey::Fd(0, PortId::fresh()).names_fd(0));
    assert!(!PortKey::Fd(0, PortId::fresh()).names_fd(1));
}

/// The explicit discard — the path a `port/close` routed through the backend
/// takes — drops the number's entry and leaves every other number alone.
#[test]
fn discarding_one_number_leaves_the_others() {
    let mut states: HashMap<PortKey, FdState> = HashMap::new();
    let seven = PortKey::Fd(7, PortId::fresh());
    let eight = PortKey::Fd(8, PortId::fresh());
    fd_state_mut(&mut states, &seven)
        .buffer
        .extend_from_slice(b"seven");
    fd_state_mut(&mut states, &eight)
        .buffer
        .extend_from_slice(b"eight");

    discard_fd_state(&mut states, 7);

    assert!(!states.contains_key(&seven));
    assert_eq!(
        states.get(&eight).map(|s| s.buffer.as_slice()),
        Some(&b"eight"[..])
    );
}
