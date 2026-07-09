//! Socket options and connect address descriptors.
//!
//! Kept separate from the core request enum because these describe the *how* of
//! socket setup (setsockopt tuning, IP-vs-Unix addressing) rather than the
//! request lifecycle itself.

/// Socket options for connect operations.
#[derive(Debug, Default, Clone)]
pub struct SocketOptions {
    pub sndbuf: Option<i32>,
    pub rcvbuf: Option<i32>,
    pub nodelay: Option<bool>,
    pub keepalive: Option<bool>,
}

/// Apply socket options (SO_SNDBUF, SO_RCVBUF, TCP_NODELAY, SO_KEEPALIVE) to a socket fd.
pub(crate) fn apply_socket_options(fd: std::os::unix::io::RawFd, opts: &SocketOptions) {
    unsafe {
        if let Some(val) = opts.sndbuf {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                &val as *const i32 as *const libc::c_void,
                std::mem::size_of::<i32>() as libc::socklen_t,
            );
        }
        if let Some(val) = opts.rcvbuf {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                &val as *const i32 as *const libc::c_void,
                std::mem::size_of::<i32>() as libc::socklen_t,
            );
        }
        if let Some(val) = opts.nodelay {
            let opt: i32 = val as i32;
            libc::setsockopt(
                fd,
                libc::IPPROTO_TCP,
                libc::TCP_NODELAY,
                &opt as *const i32 as *const libc::c_void,
                std::mem::size_of::<i32>() as libc::socklen_t,
            );
        }
        if let Some(val) = opts.keepalive {
            let opt: i32 = val as i32;
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_KEEPALIVE,
                &opt as *const i32 as *const libc::c_void,
                std::mem::size_of::<i32>() as libc::socklen_t,
            );
        }
    }
}

/// Address for connect operations.
///
/// `Tcp.addr` is a **parsed IP** — connect is IP-only at this layer. Hostname
/// resolution lives in the stdlib `tcp/connect` wrapper, which calls `sys/resolve`
/// then the IP-only `tcp/connect-ip` primitive for each returned address. This
/// keeps the backend free of a blocking getaddrinfo fallback: an io_uring connect
/// always has an address it can hand the kernel directly.
#[derive(Debug)]
pub enum ConnectAddr {
    Tcp {
        addr: std::net::IpAddr,
        port: u16,
        options: SocketOptions,
        encoding: crate::port::Encoding,
    },
    Unix {
        path: String,
        options: SocketOptions,
        encoding: crate::port::Encoding,
    },
}

impl ConnectAddr {
    pub fn options(&self) -> &SocketOptions {
        match self {
            ConnectAddr::Tcp { options, .. } => options,
            ConnectAddr::Unix { options, .. } => options,
        }
    }

    pub fn encoding(&self) -> crate::port::Encoding {
        match self {
            ConnectAddr::Tcp { encoding, .. } => *encoding,
            ConnectAddr::Unix { encoding, .. } => *encoding,
        }
    }
}
