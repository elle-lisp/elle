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

/// A share holds the descriptor number past the port's close, and gives it back
/// when it goes.
///
/// This is what keeps a number out of circulation while a worker still holds it
/// — a number reissued under a running worker gets read by that worker, and its
/// bytes reach no fiber. The port must report closed at once even so, because
/// that is what Elle promised the caller. See src/io/AGENTS.md § "Descriptor
/// retirement".
///
/// The trap: `F_GETFD` on a number a test just gave up says nothing on its own.
/// The suite shares a process and runs in parallel, so another thread can be
/// handed the number between the drop and the check, and the number reads open
/// either way. `fstat` says WHICH file the number names, and the file here is a
/// scratch file of this test's own so no other thread can be holding it.
#[test]
fn a_descriptor_share_holds_the_number_until_it_drops() {
    use std::os::unix::io::AsRawFd;

    let path = std::env::temp_dir().join(format!("elle-port-share-{}", std::process::id()));
    let file = File::create(&path).expect("create the scratch file");
    let fd: OwnedFd = file.into();
    let raw = fd.as_raw_fd();
    let scratch = file_identity(raw).expect("fstat the scratch file");
    let port = Port::new_file(
        fd,
        Direction::Read,
        Encoding::Text,
        path.display().to_string(),
    );

    let share = port.fd_share().expect("an open port shares its descriptor");
    assert_eq!(
        share.as_raw_fd(),
        raw,
        "the share names the same descriptor"
    );

    port.close();
    assert!(port.is_closed(), "the port reports closed at once");
    assert!(
        port.with_fd(|_| ()).is_none(),
        "and stops answering with it"
    );
    assert_eq!(
        file_identity(raw),
        Some(scratch),
        "the number went back to the OS while a share of it was still out"
    );

    drop(share);
    assert_ne!(
        file_identity(raw),
        Some(scratch),
        "the number stayed out of circulation after its last share went — a \
         share that outlives every holder costs one descriptor per port"
    );
    std::fs::remove_file(&path).ok();
}

/// Stdio ports do not own their descriptor, so there is no share to give.
#[test]
fn a_stdio_port_shares_no_descriptor() {
    assert!(Port::stdin().fd_share().is_none());
}

/// A closed port shares nothing: the number may already belong to somebody
/// else, and a share minted from it would name whatever that is.
#[test]
fn a_closed_port_shares_no_descriptor() {
    let closed = Port::new_file(
        File::open("/dev/null").unwrap().into(),
        Direction::Read,
        Encoding::Text,
        "/dev/null".to_string(),
    );
    closed.close();
    assert!(closed.fd_share().is_none());
}

/// What descriptor number `fd` currently names — its device and inode — or
/// `None` when the number is not open. Two numbers naming one file answer
/// alike, which is what makes this an identity rather than a liveness check.
fn file_identity(fd: std::os::unix::io::RawFd) -> Option<(u64, u64)> {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut st) } != 0 {
        return None;
    }
    Some((st.st_dev as u64, st.st_ino as u64))
}
