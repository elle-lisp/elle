//! Tests for SyncBackend.

use super::*;
use crate::io::request::{ConnectAddr, IoOp, IoRequest};
use crate::port::{Direction, Encoding, Port, PortKind};
use std::os::unix::io::FromRawFd;

use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn write_temp_file(content: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = format!("/tmp/elle-test-backend-{}-{}", std::process::id(), n);
    std::fs::write(&path, content).unwrap();
    path
}

fn open_read_port(path: &str) -> Value {
    let file = std::fs::File::open(path).unwrap();
    let fd: std::os::unix::io::OwnedFd = file.into();
    Value::external(
        "port",
        Port::new_file(fd, Direction::Read, Encoding::Text, path.to_string()),
    )
}

fn open_write_port(path: &str) -> Value {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .unwrap();
    let fd: std::os::unix::io::OwnedFd = file.into();
    Value::external(
        "port",
        Port::new_file(fd, Direction::Write, Encoding::Text, path.to_string()),
    )
}

#[test]
fn test_read_line_basic() {
    let path = write_temp_file("hello\nworld\n");
    let port = open_read_port(&path);
    let backend = SyncBackend::new();

    let req = IoRequest {
        op: IoOp::ReadLine,
        port,
        timeout: None,
    };
    let (bits, val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK);
    val.with_string(|s| assert_eq!(s, "hello")).unwrap();

    std::fs::remove_file(&path).ok();
}

#[test]
fn test_read_line_eof_returns_nil() {
    let path = write_temp_file("");
    let port = open_read_port(&path);
    let backend = SyncBackend::new();

    let req = IoRequest {
        op: IoOp::ReadLine,
        port,
        timeout: None,
    };
    let (bits, val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK);
    assert!(val.is_nil(), "expected nil for EOF, got {:?}", val);

    std::fs::remove_file(&path).ok();
}

#[test]
fn test_read_line_no_trailing_newline() {
    let path = write_temp_file("partial");
    let port = open_read_port(&path);
    let backend = SyncBackend::new();

    let req = IoRequest {
        op: IoOp::ReadLine,
        port,
        timeout: None,
    };
    let (bits, val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK);
    val.with_string(|s| assert_eq!(s, "partial")).unwrap();

    std::fs::remove_file(&path).ok();
}

#[test]
fn test_read_all() {
    let path = write_temp_file("hello world");
    let port = open_read_port(&path);
    let backend = SyncBackend::new();

    let req = IoRequest {
        op: IoOp::ReadAll,
        port,
        timeout: None,
    };
    let (bits, val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK);
    val.with_string(|s| assert_eq!(s, "hello world")).unwrap();

    std::fs::remove_file(&path).ok();
}

#[test]
fn test_write_basic() {
    let path = format!("/tmp/elle-test-write-{}", std::process::id());
    let port = open_write_port(&path);
    let backend = SyncBackend::new();

    let req = IoRequest {
        op: IoOp::Write {
            data: Value::string("hello"),
        },
        port,
        timeout: None,
    };
    let (bits, val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK);
    assert_eq!(val.as_int(), Some(5));

    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content, "hello");

    std::fs::remove_file(&path).ok();
}

#[test]
fn test_closed_port_errors() {
    let path = write_temp_file("test");
    let port_val = open_read_port(&path);
    let port = port_val.as_external::<Port>().unwrap();
    port.close();
    let backend = SyncBackend::new();

    let req = IoRequest {
        op: IoOp::ReadLine,
        port: port_val,
        timeout: None,
    };
    let (bits, _) = backend.execute(&req);
    assert_eq!(bits, SIG_ERROR);

    std::fs::remove_file(&path).ok();
}

#[test]
fn test_direction_validation() {
    let path = write_temp_file("test");
    let port = open_write_port(&path);
    let backend = SyncBackend::new();

    // Try to read from a write-only port
    let req = IoRequest {
        op: IoOp::ReadLine,
        port,
        timeout: None,
    };
    let (bits, _) = backend.execute(&req);
    assert_eq!(bits, SIG_ERROR);

    std::fs::remove_file(&path).ok();
}

// --- Network tests ---

fn make_tcp_listener() -> (Value, std::net::SocketAddr) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let fd: std::os::unix::io::OwnedFd = listener.into();
    let port = Port::new_tcp_listener(fd, addr.to_string());
    (Value::external("port", port), addr)
}

#[test]
fn test_tcp_connect_to_listener() {
    let (listener_val, addr) = make_tcp_listener();
    // Spawn a thread to accept so connect doesn't hang
    let listener_port = listener_val.as_external::<Port>().unwrap();
    let listener_fd = listener_port.with_fd(|fd| fd.as_raw_fd()).unwrap();
    let accept_thread = std::thread::spawn(move || unsafe {
        let mut sa: libc::sockaddr_storage = std::mem::zeroed();
        let mut sa_len: libc::socklen_t =
            std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        libc::accept(
            listener_fd,
            &mut sa as *mut _ as *mut libc::sockaddr,
            &mut sa_len,
        )
    });

    let backend = SyncBackend::new();
    let connect_addr = ConnectAddr::Tcp {
        addr: "127.0.0.1".to_string(),
        port: addr.port(),
    };
    let req = IoRequest {
        op: IoOp::Connect { addr: connect_addr },
        port: Value::NIL,
        timeout: None,
    };
    let (bits, val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK, "connect failed");
    assert_eq!(val.external_type_name(), Some("port"));
    let connected = val.as_external::<Port>().unwrap();
    assert_eq!(connected.kind(), PortKind::TcpStream);

    let accepted_fd = accept_thread.join().unwrap();
    if accepted_fd >= 0 {
        unsafe { libc::close(accepted_fd) };
    }
}

#[test]
fn test_tcp_connect_refused_errors() {
    let backend = SyncBackend::new();
    let req = IoRequest {
        op: IoOp::Connect {
            addr: ConnectAddr::Tcp {
                addr: "127.0.0.1".to_string(),
                port: 1, // privileged, nobody listening
            },
        },
        port: Value::NIL,
        timeout: None,
    };
    let (bits, _) = backend.execute(&req);
    assert_eq!(bits, SIG_ERROR);
}

#[test]
fn test_tcp_accept_on_real_listener() {
    let (listener_val, addr) = make_tcp_listener();
    // Spawn a connecting thread
    let connect_thread = std::thread::spawn(move || {
        std::net::TcpStream::connect(addr).unwrap();
    });

    let backend = SyncBackend::new();
    let req = IoRequest {
        op: IoOp::Accept,
        port: listener_val,
        timeout: None,
    };
    let (bits, val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK, "accept failed");
    assert_eq!(val.external_type_name(), Some("port"));
    let accepted = val.as_external::<Port>().unwrap();
    assert_eq!(accepted.kind(), PortKind::TcpStream);

    connect_thread.join().unwrap();
}

#[test]
fn test_tcp_echo_roundtrip() {
    let (listener_val, addr) = make_tcp_listener();
    let connect_thread = std::thread::spawn(move || {
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        use std::io::Write;
        stream.write_all(b"hello\n").unwrap();
    });

    let backend = SyncBackend::new();
    let req = IoRequest {
        op: IoOp::Accept,
        port: listener_val,
        timeout: None,
    };
    let (bits, accepted_val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK);

    // Read line from accepted connection
    let req = IoRequest {
        op: IoOp::ReadLine,
        port: accepted_val,
        timeout: None,
    };
    let (bits, val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK);
    val.with_string(|s| assert_eq!(s, "hello")).unwrap();

    connect_thread.join().unwrap();
}

#[test]
fn test_unix_echo_roundtrip() {
    let sock_path = format!("/tmp/elle-test-unix-{}.sock", std::process::id());
    let _ = std::fs::remove_file(&sock_path);

    // Create Unix listener via libc
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0);
    let mut sun: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    sun.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (i, b) in sock_path.bytes().enumerate() {
        sun.sun_path[i] = b as libc::c_char;
    }
    let addr_len =
        (std::mem::size_of::<libc::sa_family_t>() + sock_path.len() + 1) as libc::socklen_t;
    unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            &1i32 as *const i32 as *const libc::c_void,
            std::mem::size_of::<i32>() as libc::socklen_t,
        );
        libc::bind(fd, &sun as *const _ as *const libc::sockaddr, addr_len);
        libc::listen(fd, 128);
    }
    let owned_fd = unsafe { std::os::unix::io::OwnedFd::from_raw_fd(fd) };
    let listener_val =
        Value::external("port", Port::new_unix_listener(owned_fd, sock_path.clone()));

    let path_clone = sock_path.clone();
    let connect_thread = std::thread::spawn(move || {
        // Connect via libc
        let cfd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
        let mut csun: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        csun.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (i, b) in path_clone.bytes().enumerate() {
            csun.sun_path[i] = b as libc::c_char;
        }
        let clen =
            (std::mem::size_of::<libc::sa_family_t>() + path_clone.len() + 1) as libc::socklen_t;
        unsafe {
            libc::connect(cfd, &csun as *const _ as *const libc::sockaddr, clen);
            libc::write(cfd, b"unix-hello\n".as_ptr() as *const libc::c_void, 11);
            libc::close(cfd);
        }
    });

    let backend = SyncBackend::new();
    let req = IoRequest {
        op: IoOp::Accept,
        port: listener_val,
        timeout: None,
    };
    let (bits, accepted_val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK);
    let accepted = accepted_val.as_external::<Port>().unwrap();
    assert_eq!(accepted.kind(), PortKind::UnixStream);

    let req = IoRequest {
        op: IoOp::ReadLine,
        port: accepted_val,
        timeout: None,
    };
    let (bits, val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK);
    val.with_string(|s| assert_eq!(s, "unix-hello")).unwrap();

    connect_thread.join().unwrap();
    std::fs::remove_file(&sock_path).ok();
}

#[test]
fn test_udp_send_recv_roundtrip() {
    // Bind two UDP sockets
    let sock_a = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let sock_b = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr_b = sock_b.local_addr().unwrap();

    let fd_a: std::os::unix::io::OwnedFd = sock_a.into();
    let fd_b: std::os::unix::io::OwnedFd = sock_b.into();
    let port_a = Value::external(
        "port",
        Port::new_udp_socket(fd_a, "127.0.0.1:0".to_string()),
    );
    let port_b = Value::external("port", Port::new_udp_socket(fd_b, addr_b.to_string()));

    let backend = SyncBackend::new();

    // Send from A to B
    let req = IoRequest {
        op: IoOp::SendTo {
            addr: "127.0.0.1".to_string(),
            port_num: addr_b.port(),
            data: Value::string("udp-test"),
        },
        port: port_a,
        timeout: None,
    };
    let (bits, val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK);
    assert!(val.as_int().unwrap() > 0);

    // Recv on B
    let req = IoRequest {
        op: IoOp::RecvFrom { count: 1024 },
        port: port_b,
        timeout: None,
    };
    let (bits, val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK);
    // Result is a struct with :data, :addr, :port
    let fields = val.as_struct().unwrap();
    use crate::value::heap::TableKey;
    let data = fields.get(&TableKey::Keyword("data".into())).unwrap();
    let data_bytes = data.as_bytes().unwrap();
    assert_eq!(data_bytes, b"udp-test");
}

#[test]
fn test_shutdown_on_tcp_stream() {
    let (listener_val, addr) = make_tcp_listener();
    let connect_thread = std::thread::spawn(move || {
        let _stream = std::net::TcpStream::connect(addr).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
    });

    let backend = SyncBackend::new();
    let req = IoRequest {
        op: IoOp::Accept,
        port: listener_val,
        timeout: None,
    };
    let (bits, accepted_val) = backend.execute(&req);
    assert_eq!(bits, SIG_OK);

    // Shutdown write side
    let req = IoRequest {
        op: IoOp::Shutdown { how: libc::SHUT_WR },
        port: accepted_val,
        timeout: None,
    };
    let (bits, _) = backend.execute(&req);
    assert_eq!(bits, SIG_OK);

    connect_thread.join().unwrap();
}

#[test]
fn test_stream_read_on_listener_errors() {
    let (listener_val, _addr) = make_tcp_listener();
    let backend = SyncBackend::new();
    let req = IoRequest {
        op: IoOp::ReadLine,
        port: listener_val,
        timeout: None,
    };
    let (bits, _) = backend.execute(&req);
    assert_eq!(bits, SIG_ERROR);
}

#[test]
fn test_stream_write_on_udp_errors() {
    let sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let fd: std::os::unix::io::OwnedFd = sock.into();
    let port_val = Value::external("port", Port::new_udp_socket(fd, "127.0.0.1:0".to_string()));
    let backend = SyncBackend::new();
    let req = IoRequest {
        op: IoOp::Write {
            data: Value::string("test"),
        },
        port: port_val,
        timeout: None,
    };
    let (bits, _) = backend.execute(&req);
    assert_eq!(bits, SIG_ERROR);
}

#[test]
fn test_accept_on_non_listener_errors() {
    let path = write_temp_file("test");
    let port = open_read_port(&path);
    let backend = SyncBackend::new();
    let req = IoRequest {
        op: IoOp::Accept,
        port,
        timeout: None,
    };
    let (bits, _) = backend.execute(&req);
    assert_eq!(bits, SIG_ERROR);
    std::fs::remove_file(&path).ok();
}

// --- bytes_to_value tests ---

#[test]
fn test_bytes_to_value_valid_utf8_text_port() {
    use std::os::unix::io::OwnedFd;
    let file = std::fs::File::open("/dev/null").unwrap();
    let fd: OwnedFd = file.into();
    let port = Port::new_file(fd, Direction::Read, Encoding::Text, "/dev/null".to_string());
    let (sig, val) = SyncBackend::bytes_to_value_pub(&port, b"hello".to_vec());
    assert_eq!(sig, SIG_OK);
    assert!(val.with_string(|_| ()).is_some(), "expected string value");
}

#[test]
fn test_bytes_to_value_invalid_utf8_text_port() {
    use crate::value::heap::TableKey;
    use std::os::unix::io::OwnedFd;
    let file = std::fs::File::open("/dev/null").unwrap();
    let fd: OwnedFd = file.into();
    let port = Port::new_file(fd, Direction::Read, Encoding::Text, "/dev/null".to_string());
    let (sig, val) = SyncBackend::bytes_to_value_pub(&port, vec![0xFF, 0xFE, 0x00]);
    assert_eq!(sig, SIG_ERROR);
    // Error kind is stored in the `:error` key as a keyword
    // (error_val uses {:error :encoding-error :message "..."})
    let error_keyword = val
        .as_struct()
        .and_then(|f| f.get(&TableKey::Keyword("error".into())))
        .and_then(|v| v.as_keyword_name().map(|s| s.to_string()));
    assert_eq!(error_keyword.as_deref(), Some("encoding-error"));
}

#[test]
fn test_bytes_to_value_binary_port() {
    use std::os::unix::io::OwnedFd;
    let file = std::fs::File::open("/dev/null").unwrap();
    let fd: OwnedFd = file.into();
    let port = Port::new_file(
        fd,
        Direction::Read,
        Encoding::Binary,
        "/dev/null".to_string(),
    );
    let (sig, val) = SyncBackend::bytes_to_value_pub(&port, vec![0xFF, 0x00, 0xAB]);
    assert_eq!(sig, SIG_OK);
    assert!(val.as_bytes().is_some(), "binary port → bytes value");
}

// --- subprocess tests ---

#[test]
fn test_execute_spawn_echo() {
    use crate::io::request::StdioDisposition;
    let backend = SyncBackend::new();
    let request = IoRequest {
        op: IoOp::Spawn {
            program: "/bin/echo".to_string(),
            args: vec!["hello".to_string()],
            env: None,
            cwd: None,
            stdin: StdioDisposition::Null,
            stdout: StdioDisposition::Pipe,
            stderr: StdioDisposition::Null,
        },
        port: Value::NIL,
        timeout: None,
    };
    let (sig, val) = backend.execute(&request);
    assert_eq!(sig, SIG_OK, "spawn failed: {:?}", val);
    let fields = val.as_struct().expect("expected struct");
    use crate::value::heap::TableKey;
    assert!(
        fields
            .get(&TableKey::Keyword("pid".into()))
            .unwrap()
            .as_int()
            .unwrap()
            > 0
    );
    assert_eq!(
        fields
            .get(&TableKey::Keyword("stdout".into()))
            .unwrap()
            .external_type_name(),
        Some("port")
    );
    assert!(fields
        .get(&TableKey::Keyword("stdin".into()))
        .unwrap()
        .is_nil()); // Null disposition → nil
    assert_eq!(
        fields
            .get(&TableKey::Keyword("process".into()))
            .unwrap()
            .external_type_name(),
        Some("process")
    );
}

#[test]
fn test_execute_spawn_nonexistent() {
    use crate::io::request::StdioDisposition;
    let backend = SyncBackend::new();
    let request = IoRequest {
        op: IoOp::Spawn {
            program: "/nonexistent/command".to_string(),
            args: vec![],
            env: None,
            cwd: None,
            stdin: StdioDisposition::Null,
            stdout: StdioDisposition::Null,
            stderr: StdioDisposition::Null,
        },
        port: Value::NIL,
        timeout: None,
    };
    let (sig, _) = backend.execute(&request);
    assert_eq!(sig, SIG_ERROR);
}

#[test]
fn test_execute_process_wait_exit_zero() {
    use crate::io::request::{IoOp, IoRequest, ProcessHandle};
    use std::process::Command;
    let child = Command::new("/bin/true").spawn().unwrap();
    let pid = child.id();
    let handle = ProcessHandle::new(pid, child);
    let handle_val = Value::external("process", handle);
    let request = IoRequest {
        op: IoOp::ProcessWait,
        port: handle_val,
        timeout: None,
    };
    let backend = SyncBackend::new();
    let (sig, val) = backend.execute(&request);
    assert_eq!(sig, SIG_OK);
    assert_eq!(val.as_int(), Some(0));
}

#[test]
fn test_execute_process_wait_exit_nonzero() {
    use crate::io::request::{IoOp, IoRequest, ProcessHandle};
    use std::process::Command;
    let child = Command::new("/bin/false").spawn().unwrap();
    let pid = child.id();
    let handle = ProcessHandle::new(pid, child);
    let handle_val = Value::external("process", handle);
    let request = IoRequest {
        op: IoOp::ProcessWait,
        port: handle_val,
        timeout: None,
    };
    let backend = SyncBackend::new();
    let (sig, val) = backend.execute(&request);
    assert_eq!(sig, SIG_OK);
    assert_ne!(val.as_int(), Some(0));
}

#[test]
fn test_execute_process_wait_idempotent() {
    // Second wait returns cached status, does not panic.
    use crate::io::request::{IoOp, IoRequest, ProcessHandle};
    use std::process::Command;
    let child = Command::new("/bin/true").spawn().unwrap();
    let pid = child.id();
    let handle = ProcessHandle::new(pid, child);
    let handle_val = Value::external("process", handle);
    let backend = SyncBackend::new();
    let req = IoRequest {
        op: IoOp::ProcessWait,
        port: handle_val,
        timeout: None,
    };
    let (_, v1) = backend.execute(&req);
    let (_, v2) = backend.execute(&req);
    assert_eq!(v1.as_int(), Some(0));
    assert_eq!(v2.as_int(), Some(0));
}
