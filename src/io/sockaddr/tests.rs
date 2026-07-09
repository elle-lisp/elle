//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_build_inet_v4() {
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let (bytes, len) = build_inet(&addr);
    assert_eq!(len as usize, std::mem::size_of::<libc::sockaddr_in>());
    assert!(!bytes.is_empty());
    // Verify round-trip through format
    let mut storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            &mut storage as *mut _ as *mut u8,
            bytes.len(),
        );
    }
    assert_eq!(format(&storage, len), "127.0.0.1:8080");
}

#[test]
fn test_build_inet_v6() {
    let addr: SocketAddr = "[::1]:443".parse().unwrap();
    let (bytes, len) = build_inet(&addr);
    assert_eq!(len as usize, std::mem::size_of::<libc::sockaddr_in6>());
    let mut storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            &mut storage as *mut _ as *mut u8,
            bytes.len(),
        );
    }
    assert_eq!(format(&storage, len), "[::1]:443");
}

#[test]
fn test_build_unix_regular() {
    let (sun, len) = build_unix("/nonexistent/test.sock").unwrap();
    assert_eq!(sun.sun_family, libc::AF_UNIX as libc::sa_family_t);
    assert!(len > 0);
}

#[test]
fn test_build_unix_abstract() {
    let (sun, len) = build_unix("@myapp").unwrap();
    assert_eq!(sun.sun_family, libc::AF_UNIX as libc::sa_family_t);
    assert_eq!(sun.sun_path[0], 0);
    assert!(len > 0);
}

#[test]
fn test_build_unix_too_long() {
    let path = "x".repeat(200);
    assert!(build_unix(&path).is_err());
}

#[test]
fn test_parse_v4() {
    let addr: SocketAddr = "10.0.0.1:3000".parse().unwrap();
    let (bytes, len) = build_inet(&addr);
    let mut storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            &mut storage as *mut _ as *mut u8,
            bytes.len(),
        );
    }
    let (ip, port) = parse(&storage, len);
    assert_eq!(ip, "10.0.0.1");
    assert_eq!(port, 3000);
}

#[test]
fn test_format_host_port_v4() {
    assert_eq!(format_host_port("127.0.0.1", 80), "127.0.0.1:80");
}

#[test]
fn test_format_host_port_v6() {
    assert_eq!(format_host_port("::1", 443), "[::1]:443");
}

#[test]
fn test_format_host_port_hostname() {
    assert_eq!(format_host_port("example.com", 8080), "example.com:8080");
}
