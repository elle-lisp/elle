//! Unit tests (`super` is the parent impl module).

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

fn devnull_fd() -> OwnedFd {
    File::open("/dev/null").unwrap().into()
}

#[test]
fn test_new_tcp_listener_kind() {
    let p = Port::new_tcp_listener(devnull_fd(), "127.0.0.1:8080".into());
    assert_eq!(p.kind(), PortKind::TcpListener);
    assert_eq!(p.direction(), Direction::Read);
}

#[test]
fn test_new_tcp_stream_kind() {
    let p = Port::new_tcp_stream(devnull_fd(), "127.0.0.1:8080".into());
    assert_eq!(p.kind(), PortKind::TcpStream);
    assert_eq!(p.direction(), Direction::ReadWrite);
    assert_eq!(p.encoding(), Encoding::Binary);
}

#[test]
fn test_new_udp_socket_kind() {
    let p = Port::new_udp_socket(devnull_fd(), "0.0.0.0:9000".into());
    assert_eq!(p.kind(), PortKind::UdpSocket);
    assert_eq!(p.encoding(), Encoding::Binary);
}

#[test]
fn test_new_unix_listener_kind() {
    let p = Port::new_unix_listener(devnull_fd(), "/nonexistent/test.sock".into());
    assert_eq!(p.kind(), PortKind::UnixListener);
}

#[test]
fn test_new_unix_stream_kind() {
    let p = Port::new_unix_stream(devnull_fd(), "/nonexistent/test.sock".into());
    assert_eq!(p.kind(), PortKind::UnixStream);
    assert_eq!(p.encoding(), Encoding::Binary);
}

#[test]
fn test_tcp_listener_display() {
    let p = Port::new_tcp_listener(devnull_fd(), "127.0.0.1:8080".into());
    assert!(format!("{}", p).contains("tcp-listener"));
}

#[test]
fn test_port_timeout_default_none() {
    let p = Port::new_tcp_stream(devnull_fd(), "x".into());
    assert_eq!(p.timeout_ms(), None);
}

#[test]
fn test_port_timeout_get_set() {
    let p = Port::new_tcp_stream(devnull_fd(), "x".into());
    p.set_timeout_ms(Some(5000));
    assert_eq!(p.timeout_ms(), Some(5000));
    p.set_timeout_ms(None);
    assert_eq!(p.timeout_ms(), None);
}

#[test]
fn test_new_pipe_kind() {
    let file = File::open("/dev/null").unwrap();
    let fd: OwnedFd = file.into();
    let p = Port::new_pipe(
        fd,
        Direction::Read,
        Encoding::Binary,
        "pid:42:stdout".to_string(),
    );
    assert_eq!(p.kind(), PortKind::Pipe);
    assert_eq!(p.direction(), Direction::Read);
    assert_eq!(p.encoding(), Encoding::Binary);
    assert_eq!(p.path(), Some("pid:42:stdout"));
}

#[test]
fn test_pipe_display_binary() {
    let file = File::open("/dev/null").unwrap();
    let fd: OwnedFd = file.into();
    let p = Port::new_pipe(
        fd,
        Direction::Read,
        Encoding::Binary,
        "pid:1234:stdout".to_string(),
    );
    let s = format!("{}", p);
    assert!(s.contains("pipe"), "display: {}", s);
    assert!(s.contains("pid:1234:stdout"), "display: {}", s);
    assert!(s.contains(":read"), "display: {}", s);
    assert!(s.contains(":binary"), "display: {}", s);
}

#[test]
fn test_pipe_display_write() {
    let file = File::open("/dev/null").unwrap();
    let fd: OwnedFd = file.into();
    let p = Port::new_pipe(
        fd,
        Direction::Write,
        Encoding::Binary,
        "pid:5:stdin".to_string(),
    );
    let s = format!("{}", p);
    assert!(s.contains(":write"), "display: {}", s);
}

#[test]
fn test_pipe_display_closed() {
    let file = File::open("/dev/null").unwrap();
    let fd: OwnedFd = file.into();
    let p = Port::new_pipe(
        fd,
        Direction::Read,
        Encoding::Binary,
        "pid:1:stdout".to_string(),
    );
    p.close();
    assert!(format!("{}", p).contains("[closed]"));
}

/// Retiring a port hands its descriptor out instead of closing it.
///
/// The I/O backend uses this to keep a descriptor number out of circulation
/// while a worker still holds it — a number reissued under a running worker
/// gets read by that worker, and its bytes reach no fiber. So the port must
/// report closed at once while the descriptor stays open in the caller's
/// hands. See src/io/AGENTS.md § "Descriptor retirement".
#[test]
fn retire_fd_hands_out_the_descriptor_and_closes_the_port() {
    use std::os::unix::io::AsRawFd;

    let file = File::open("/dev/null").unwrap();
    let fd: OwnedFd = file.into();
    let raw = fd.as_raw_fd();
    let port = Port::new_file(fd, Direction::Read, Encoding::Text, "/dev/null".to_string());

    let retired = port
        .retire_fd()
        .expect("an open port yields its descriptor");
    assert_eq!(retired.as_raw_fd(), raw, "the same descriptor comes back");
    assert!(port.is_closed(), "the port reports closed once retired");
    assert!(
        port.with_fd(|_| ()).is_none(),
        "the port no longer holds it"
    );

    // Still open: the caller owns it now, and closing is the caller's to do.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
    assert!(flags >= 0, "the descriptor is still open after retirement");
}

#[test]
fn retire_fd_yields_nothing_twice_or_after_close() {
    let file = File::open("/dev/null").unwrap();
    let fd: OwnedFd = file.into();
    let port = Port::new_file(fd, Direction::Read, Encoding::Text, "/dev/null".to_string());
    assert!(port.retire_fd().is_some());
    assert!(
        port.retire_fd().is_none(),
        "a retired port has no second descriptor to give"
    );

    let file = File::open("/dev/null").unwrap();
    let closed = Port::new_file(
        file.into(),
        Direction::Read,
        Encoding::Text,
        "/dev/null".to_string(),
    );
    closed.close();
    assert!(
        closed.retire_fd().is_none(),
        "a closed port's descriptor is already gone"
    );
}

/// Stdio ports do not own their descriptor, so there is nothing to retire.
#[test]
fn retire_fd_yields_nothing_for_a_stdio_port() {
    assert!(Port::stdin().retire_fd().is_none());
}
