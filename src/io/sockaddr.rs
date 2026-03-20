//! Sockaddr construction and formatting.
//!
//! Single source of truth for building, formatting, and parsing
//! `sockaddr_in`, `sockaddr_in6`, and `sockaddr_un` structures.

use std::net::SocketAddr;

// ── Construction ───────────────────────────────────────────────────

/// Build a sockaddr for an IP address as raw bytes.
///
/// Returns `(bytes, sockaddr_len)`. Used by io_uring (bytes stashed in
/// buffer pool) and thread pool (cast back to sockaddr pointer).
pub(crate) fn build_inet(addr: &SocketAddr) -> (Vec<u8>, libc::socklen_t) {
    match addr {
        SocketAddr::V4(v4) => {
            let mut sin: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            sin.sin_family = libc::AF_INET as libc::sa_family_t;
            sin.sin_port = v4.port().to_be();
            sin.sin_addr.s_addr = u32::from(*v4.ip()).to_be();
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    &sin as *const _ as *const u8,
                    std::mem::size_of::<libc::sockaddr_in>(),
                )
                .to_vec()
            };
            (
                bytes,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        }
        SocketAddr::V6(v6) => {
            let mut sin6: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            sin6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            sin6.sin6_port = v6.port().to_be();
            sin6.sin6_addr.s6_addr = v6.ip().octets();
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    &sin6 as *const _ as *const u8,
                    std::mem::size_of::<libc::sockaddr_in6>(),
                )
                .to_vec()
            };
            (
                bytes,
                std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
            )
        }
    }
}

/// Build a `sockaddr_un` from a path string.
///
/// Paths prefixed with `@` are abstract sockets (Linux-specific).
/// Returns the struct and the correct `addr_len` for `connect()`/`bind()`.
pub(crate) fn build_unix(path: &str) -> Result<(libc::sockaddr_un, libc::socklen_t), String> {
    let mut sun: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    sun.sun_family = libc::AF_UNIX as libc::sa_family_t;

    if let Some(name) = path.strip_prefix('@') {
        let max = sun.sun_path.len() - 1;
        if name.len() > max {
            return Err("abstract socket name too long".into());
        }
        sun.sun_path[0] = 0;
        for (i, b) in name.bytes().enumerate() {
            sun.sun_path[i + 1] = b as libc::c_char;
        }
        let len = std::mem::size_of::<libc::sa_family_t>() + 1 + name.len();
        Ok((sun, len as libc::socklen_t))
    } else {
        let max = sun.sun_path.len() - 1;
        if path.len() > max {
            return Err("unix path too long".into());
        }
        for (i, b) in path.bytes().enumerate() {
            sun.sun_path[i] = b as libc::c_char;
        }
        let len = std::mem::size_of::<libc::sa_family_t>() + path.len() + 1;
        Ok((sun, len as libc::socklen_t))
    }
}

/// Format a host and port as a string suitable for `TcpStream::connect`
/// and `SocketAddr::parse`. IPv6 addresses are wrapped in brackets.
pub(crate) fn format_host_port(host: &str, port: u16) -> String {
    if host.contains(':') {
        // IPv6 — needs brackets
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}

// ── Formatting ─────────────────────────────────────────────────────

/// Format a `sockaddr_storage` as `"ip:port"`, `"[ipv6]:port"`, or unix path.
///
/// Uses `std::net::Ipv4Addr`/`Ipv6Addr` for canonical formatting
/// (proper IPv6 shortening, no raw hex octets).
pub(crate) fn format(addr: &libc::sockaddr_storage, len: libc::socklen_t) -> String {
    match addr.ss_family as libc::c_int {
        libc::AF_INET => {
            let sin =
                unsafe { &*(addr as *const libc::sockaddr_storage as *const libc::sockaddr_in) };
            let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
            let port = u16::from_be(sin.sin_port);
            format!("{}:{}", ip, port)
        }
        libc::AF_INET6 => {
            let sin6 =
                unsafe { &*(addr as *const libc::sockaddr_storage as *const libc::sockaddr_in6) };
            let ip = std::net::Ipv6Addr::from(sin6.sin6_addr.s6_addr);
            let port = u16::from_be(sin6.sin6_port);
            format!("[{}]:{}", ip, port)
        }
        libc::AF_UNIX => {
            let sun =
                unsafe { &*(addr as *const libc::sockaddr_storage as *const libc::sockaddr_un) };
            let path_offset = std::mem::size_of::<libc::sa_family_t>();
            let path_len = (len as usize).saturating_sub(path_offset);
            if path_len == 0 {
                return "unix:unnamed".to_string();
            }
            if sun.sun_path[0] == 0 {
                let name_bytes: Vec<u8> =
                    sun.sun_path[1..path_len].iter().map(|&c| c as u8).collect();
                format!("@{}", String::from_utf8_lossy(&name_bytes))
            } else {
                let name_bytes: Vec<u8> = sun.sun_path[..path_len]
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8)
                    .collect();
                String::from_utf8_lossy(&name_bytes).to_string()
            }
        }
        _ => "unknown".to_string(),
    }
}

/// Parse a `sockaddr_storage` into `(address_string, port)`.
///
/// For AF_INET/AF_INET6, returns the IP string (without port) and the port
/// number separately. For AF_UNIX, returns the path and port 0.
pub(crate) fn parse(addr: &libc::sockaddr_storage, len: libc::socklen_t) -> (String, u16) {
    match addr.ss_family as libc::c_int {
        libc::AF_INET => {
            let sin =
                unsafe { &*(addr as *const libc::sockaddr_storage as *const libc::sockaddr_in) };
            let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
            let port = u16::from_be(sin.sin_port);
            (ip.to_string(), port)
        }
        libc::AF_INET6 => {
            let sin6 =
                unsafe { &*(addr as *const libc::sockaddr_storage as *const libc::sockaddr_in6) };
            let ip = std::net::Ipv6Addr::from(sin6.sin6_addr.s6_addr);
            let port = u16::from_be(sin6.sin6_port);
            (format!("[{}]", ip), port)
        }
        libc::AF_UNIX => {
            let sun =
                unsafe { &*(addr as *const libc::sockaddr_storage as *const libc::sockaddr_un) };
            let path_offset = std::mem::size_of::<libc::sa_family_t>();
            let path_len = (len as usize).saturating_sub(path_offset);
            if path_len == 0 {
                return ("unix:unnamed".to_string(), 0);
            }
            if sun.sun_path[0] == 0 {
                let name_bytes: Vec<u8> =
                    sun.sun_path[1..path_len].iter().map(|&c| c as u8).collect();
                (format!("@{}", String::from_utf8_lossy(&name_bytes)), 0)
            } else {
                let name_bytes: Vec<u8> = sun.sun_path[..path_len]
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8)
                    .collect();
                (String::from_utf8_lossy(&name_bytes).to_string(), 0)
            }
        }
        _ => ("unknown".to_string(), 0),
    }
}

/// Get peer address of a connected socket as a formatted string.
pub(crate) fn peer_address(fd: i32) -> String {
    unsafe {
        let mut storage: libc::sockaddr_storage = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if libc::getpeername(fd, &mut storage as *mut _ as *mut libc::sockaddr, &mut len) == 0 {
            format(&storage, len)
        } else {
            "unknown".to_string()
        }
    }
}

/// Get local bound address of a socket as a formatted string.
pub(crate) fn local_address(fd: i32) -> String {
    unsafe {
        let mut storage: libc::sockaddr_storage = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if libc::getsockname(fd, &mut storage as *mut _ as *mut libc::sockaddr, &mut len) == 0 {
            format(&storage, len)
        } else {
            "unknown".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
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
        let (sun, len) = build_unix("/tmp/test.sock").unwrap();
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
}
