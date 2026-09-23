//! audited: 2026-09-23
//! What a socket option asks of the kernel reaches the socket: one test per
//! option `apply_socket_options` sets.
//!
//! src/io/AGENTS.md

use crate::io::request::{apply_socket_options, SocketOptions};

/// The smallest value getsockopt can report after a successful SO_SNDBUF or
/// SO_RCVBUF setsockopt: the kernel clamps the request to the net.core
/// ceiling (`wmem_max` or `rmem_max`), stores double the clamped value to
/// account for bookkeeping overhead, and reports the doubled figure.
///
/// Asserting against this bound measures the code on any host. Asserting
/// against the raw request measures the host's sysctl instead: the stock
/// ceiling is 212992, so a megabyte-scale request comes back as 425984 on a
/// default-configured kernel — including GitHub CI runners. The bound stays
/// counter-factual even when fully clamped, because a socket that never saw
/// the setsockopt reports the un-doubled default and fails it.
fn min_reported_bufsize(requested: i32, ceiling_knob: &str) -> i32 {
    let path = format!("/proc/sys/net/core/{}", ceiling_knob);
    let ceiling: i32 = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {}", path, e))
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("parse {}: {}", path, e));
    2 * requested.min(ceiling)
}

/// Verify apply_socket_options actually sets SO_SNDBUF on a socket fd.
#[test]
fn test_apply_sndbuf() {
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let requested = 1048576;
    let opts = SocketOptions {
        sndbuf: Some(requested),
        ..Default::default()
    };
    apply_socket_options(fd, &opts);

    let mut val: i32 = 0;
    let mut len: libc::socklen_t = std::mem::size_of::<i32>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &mut val as *mut i32 as *mut libc::c_void,
            &mut len,
        )
    };
    unsafe { libc::close(fd) };
    assert_eq!(ret, 0, "getsockopt failed");
    let want = min_reported_bufsize(requested, "wmem_max");
    assert!(
        val >= want,
        "SO_SNDBUF should be >= 2 x min(requested, wmem_max) = {}: got {}",
        want,
        val
    );
}

/// Verify apply_socket_options actually sets SO_RCVBUF.
#[test]
fn test_apply_rcvbuf() {
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let requested = 524288;
    let opts = SocketOptions {
        rcvbuf: Some(requested),
        ..Default::default()
    };
    apply_socket_options(fd, &opts);

    let mut val: i32 = 0;
    let mut len: libc::socklen_t = std::mem::size_of::<i32>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            &mut val as *mut i32 as *mut libc::c_void,
            &mut len,
        )
    };
    unsafe { libc::close(fd) };
    assert_eq!(ret, 0, "getsockopt failed");
    let want = min_reported_bufsize(requested, "rmem_max");
    assert!(
        val >= want,
        "SO_RCVBUF should be >= 2 x min(requested, rmem_max) = {}: got {}",
        want,
        val
    );
}

/// Verify SO_KEEPALIVE is actually enabled.
#[test]
fn test_apply_keepalive() {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let opts = SocketOptions {
        keepalive: Some(true),
        ..Default::default()
    };
    apply_socket_options(fd, &opts);

    let mut val: i32 = 0;
    let mut len: libc::socklen_t = std::mem::size_of::<i32>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_KEEPALIVE,
            &mut val as *mut i32 as *mut libc::c_void,
            &mut len,
        )
    };
    unsafe { libc::close(fd) };
    assert_eq!(ret, 0, "getsockopt failed");
    assert_eq!(val, 1, "SO_KEEPALIVE should be enabled");
}

/// Verify TCP_NODELAY is actually enabled.
#[test]
fn test_apply_nodelay() {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let opts = SocketOptions {
        nodelay: Some(true),
        ..Default::default()
    };
    apply_socket_options(fd, &opts);

    let mut val: i32 = 0;
    let mut len: libc::socklen_t = std::mem::size_of::<i32>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::IPPROTO_TCP,
            libc::TCP_NODELAY,
            &mut val as *mut i32 as *mut libc::c_void,
            &mut len,
        )
    };
    unsafe { libc::close(fd) };
    assert_eq!(ret, 0, "getsockopt failed");
    assert_eq!(val, 1, "TCP_NODELAY should be enabled");
}

/// TCP_NODELAY on a Unix socket doesn't panic (silently ignored).
#[test]
fn test_nodelay_on_unix_is_harmless() {
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let opts = SocketOptions {
        nodelay: Some(true),
        ..Default::default()
    };
    apply_socket_options(fd, &opts);
    unsafe { libc::close(fd) };
}

/// Default SocketOptions is a no-op.
#[test]
fn test_default_is_noop() {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let mut before: i32 = 0;
    let mut len: libc::socklen_t = std::mem::size_of::<i32>() as libc::socklen_t;
    unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &mut before as *mut i32 as *mut libc::c_void,
            &mut len,
        );
    }

    apply_socket_options(fd, &SocketOptions::default());

    let mut after: i32 = 0;
    unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &mut after as *mut i32 as *mut libc::c_void,
            &mut len,
        );
    }
    unsafe { libc::close(fd) };
    assert_eq!(before, after, "default options should not change SO_SNDBUF");
}

/// All four options can be set together without conflict.
#[test]
fn test_all_combined() {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let sndbuf_req = 2097152;
    let rcvbuf_req = 1048576;
    let opts = SocketOptions {
        sndbuf: Some(sndbuf_req),
        rcvbuf: Some(rcvbuf_req),
        nodelay: Some(true),
        keepalive: Some(true),
    };
    apply_socket_options(fd, &opts);

    let read_opt = |level: i32, optname: i32| -> i32 {
        let mut val: i32 = 0;
        let mut len: libc::socklen_t = std::mem::size_of::<i32>() as libc::socklen_t;
        unsafe {
            libc::getsockopt(
                fd,
                level,
                optname,
                &mut val as *mut i32 as *mut libc::c_void,
                &mut len,
            );
        }
        val
    };

    assert!(
        read_opt(libc::SOL_SOCKET, libc::SO_SNDBUF) >= min_reported_bufsize(sndbuf_req, "wmem_max")
    );
    assert!(
        read_opt(libc::SOL_SOCKET, libc::SO_RCVBUF) >= min_reported_bufsize(rcvbuf_req, "rmem_max")
    );
    assert_eq!(read_opt(libc::IPPROTO_TCP, libc::TCP_NODELAY), 1);
    assert_eq!(read_opt(libc::SOL_SOCKET, libc::SO_KEEPALIVE), 1);
    unsafe { libc::close(fd) };
}
