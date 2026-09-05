// audited: 2026-09-05
// src/io/AGENTS.md
//! The socket operations a worker runs.
//!
//! `Accept`, `RecvFrom` and both connects each wait on a peer that may never
//! appear, so each takes the descriptor non-blocking and waits under its bound
//! rather than in the kernel. `SendTo` and `Shutdown` hand what they were given
//! to the kernel and return.
//!
//! # A full AF_UNIX backlog arrives under two errnos
//!
//! Linux reports a connect to an AF_UNIX listener whose backlog is full as
//! `EAGAIN`. macOS and the BSDs report it as `ECONNREFUSED`, which is also the
//! errno they return when nobody is listening. So one peer state arrives under
//! two names, and on the BSDs that name carries two readings.
//!
//! A connect paces `EAGAIN` on every platform. It paces `ECONNREFUSED` only
//! for an AF_UNIX peer, only where the errno is spent both ways, and only
//! while the path still names a socket. That last condition is what keeps a
//! dead peer fast: a listener that has gone takes its socket with it, so the
//! path stops naming one and the connect reports the refusal it was pacing. A
//! TCP connect never paces a refusal, because a reset from a host with no
//! listener is the peer's whole answer.
//!
//! What the path cannot separate is a socket that is bound and never listened
//! on. It refuses exactly as a full backlog does and leaves its file in place,
//! so such a connect is refused at once on Linux and paces to the caller's
//! `:timeout` elsewhere. That is the price of pacing the common case, and
//! `:timeout` is what bounds it.

use super::*;
use std::time::Instant;

/// Take one connection from a listener. Reports the new descriptor, and the
/// peer's address encoded as `addr_len` (4 bytes, little-endian) followed by
/// the `sockaddr_storage` bytes.
pub(super) fn accept(bound: OpBound, fd: RawFd) -> (i32, Vec<u8>) {
    let mut addr_storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut addr_len: libc::socklen_t =
        std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    let new_fd = take_when_ready(&bound, libc::POLLIN, || {
        let mut len: libc::socklen_t =
            std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        let r = unsafe {
            libc::accept(
                fd,
                &mut addr_storage as *mut _ as *mut libc::sockaddr,
                &mut len,
            )
        };
        if r >= 0 {
            addr_len = len;
        }
        r as isize
    }) as i32;
    if new_fd < 0 {
        return (new_fd, Vec::new());
    }
    unsafe {
        libc::fcntl(new_fd, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    (new_fd, encode_addr(addr_len, &addr_storage, &[]))
}

/// Take one datagram. Reports the byte count, and `addr_len` + the
/// `sockaddr_storage` + the payload.
pub(super) fn recv_from(bound: OpBound, fd: RawFd, size: usize) -> (i32, Vec<u8>) {
    let mut buf = vec![0u8; size];
    let mut addr_storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut addr_len: libc::socklen_t =
        std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    let ret = take_when_ready(&bound, libc::POLLIN, || {
        let mut len: libc::socklen_t =
            std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        let r = unsafe {
            libc::recvfrom(
                fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                0,
                &mut addr_storage as *mut _ as *mut libc::sockaddr,
                &mut len,
            )
        };
        if r >= 0 {
            addr_len = len;
        }
        r
    });
    if ret < 0 {
        return (ret as i32, Vec::new());
    }
    buf.truncate(ret as usize);
    (ret as i32, encode_addr(addr_len, &addr_storage, &buf))
}

/// The wire form both `Accept` and `RecvFrom` completions parse: the address
/// length as 4 little-endian bytes, the whole `sockaddr_storage`, then `tail`.
fn encode_addr(
    addr_len: libc::socklen_t,
    addr_storage: &libc::sockaddr_storage,
    tail: &[u8],
) -> Vec<u8> {
    let mut out =
        Vec::with_capacity(4 + std::mem::size_of::<libc::sockaddr_storage>() + tail.len());
    out.extend_from_slice(&addr_len.to_le_bytes());
    // SAFETY: `sockaddr_storage` is plain data with no padding requirements
    // the reader depends on; the completion reads back only `addr_len` of it.
    let addr_bytes = unsafe {
        std::slice::from_raw_parts(
            addr_storage as *const _ as *const u8,
            std::mem::size_of::<libc::sockaddr_storage>(),
        )
    };
    out.extend_from_slice(addr_bytes);
    out.extend_from_slice(tail);
    out
}

/// Send one datagram to `addr:port`.
pub(super) fn send_to(fd: RawFd, addr: &str, port: u16, data: &[u8]) -> (i32, Vec<u8>) {
    let addr_str = crate::io::sockaddr::format_host_port(addr, port);
    let dest = match addr_str.parse::<std::net::SocketAddr>() {
        Ok(dest) => dest,
        Err(e) => return (-1, format!("bad address: {}", e).into_bytes()),
    };
    let (sa_bytes, sa_len) = crate::io::sockaddr::build_inet(&dest);
    let ret = unsafe {
        libc::sendto(
            fd,
            data.as_ptr() as *const libc::c_void,
            data.len(),
            0,
            sa_bytes.as_ptr() as *const libc::sockaddr,
            sa_len,
        )
    };
    if ret < 0 {
        return (
            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
            Vec::new(),
        );
    }
    (ret as i32, Vec::new())
}

/// Shut down one or both directions of a connected socket.
pub(super) fn shutdown(fd: RawFd, how: i32) -> (i32, Vec<u8>) {
    if unsafe { libc::shutdown(fd, how) } < 0 {
        return (
            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
            Vec::new(),
        );
    }
    (0, Vec::new())
}

/// Connect to an IP peer.
pub(super) fn connect_tcp(
    addr: std::net::SocketAddr,
    options: &SocketOptions,
    bounds: Bounds,
) -> (i32, Vec<u8>) {
    let family = if addr.is_ipv6() {
        libc::AF_INET6
    } else {
        libc::AF_INET
    };
    let (sa, sa_len) = crate::io::sockaddr::build_inet(&addr);
    connect_socket(
        family,
        sa.as_ptr() as *const libc::sockaddr,
        sa_len,
        options,
        bounds,
        addr.to_string(),
    )
}

/// Connect to an AF_UNIX peer.
pub(super) fn connect_unix(path: &str, options: &SocketOptions, bounds: Bounds) -> (i32, Vec<u8>) {
    match crate::io::sockaddr::build_unix(path) {
        // A path the kernel could never accept, caught before a descriptor is
        // opened for it. Dropping `bounds` closes the stop pipe with it.
        Err(msg) => (-libc::EINVAL, msg.into_bytes()),
        Ok((sun, addr_len)) => connect_socket(
            libc::AF_UNIX,
            &sun as *const _ as *const libc::sockaddr,
            addr_len,
            options,
            bounds,
            path.to_string(),
        ),
    }
}

/// How long a connect that met a full AF_UNIX backlog waits before trying
/// again. The kernel offers no readiness for that state, whichever errno it
/// reports it under, so the retry is paced rather than driven by an event.
const CONNECT_RETRY_PACE: Duration = Duration::from_millis(10);

/// Open a socket, connect it to `sa`, and report the connected descriptor —
/// or `-errno`. `label` describes the peer for the completion's data.
///
/// The descriptor belongs to this operation, so the bound can take it
/// non-blocking for the whole connect. That is what makes the connect
/// answerable: a blocking `connect(2)` holds this worker through the peer's
/// entire retry sequence, where neither the caller's deadline nor a
/// cancellation can reach it.
fn connect_socket(
    family: libc::c_int,
    sa: *const libc::sockaddr,
    sa_len: libc::socklen_t,
    options: &SocketOptions,
    bounds: Bounds,
    label: String,
) -> (i32, Vec<u8>) {
    let fd = unsafe { libc::socket(family, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        // Dropping `bounds` closes the stop pipe this operation will not use.
        return (
            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
            Vec::new(),
        );
    }
    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    // Before the handshake, as the io_uring path does: a TCP window is
    // negotiated while the handshake runs.
    crate::io::request::apply_socket_options(fd, options);

    // The bound is dropped before the failure close below. It clears the
    // non-blocking flag through the descriptor number, which a closed
    // descriptor could have handed to another thread's socket by then.
    let outcome = {
        let bound = OpBound::new(fd, bounds);
        connect_bounded(fd, sa, sa_len, &bound)
    };
    if outcome < 0 {
        unsafe { libc::close(fd) };
        return (outcome, Vec::new());
    }
    (fd, label.into_bytes())
}

/// Drive one non-blocking connect to its outcome under `bound`: 0 once the
/// peer answers, or `-errno` — `-ECANCELED` when stopped, `-ETIMEDOUT` at the
/// caller's deadline.
///
/// The deadline spans the whole connect rather than each retry, because a
/// connect is one operation however many times the kernel makes us ask.
fn connect_bounded(
    fd: RawFd,
    sa: *const libc::sockaddr,
    sa_len: libc::socklen_t,
    bound: &OpBound,
) -> i32 {
    let deadline = bound.timeout().map(|t| Instant::now() + t);
    loop {
        if unsafe { libc::connect(fd, sa, sa_len) } == 0 {
            return 0;
        }
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        match errno {
            // The handshake is under way, and reports its outcome by making
            // the socket writable.
            libc::EINPROGRESS | libc::EALREADY | libc::EINTR => {
                return match bound.wait(libc::POLLOUT) {
                    Wake::Ready => connect_result(fd),
                    Wake::Stopped => -libc::ECANCELED,
                    Wake::TimedOut => -libc::ETIMEDOUT,
                }
            }
            // A second call after the connection is up says so this way.
            libc::EISCONN => return 0,
            // The peer's backlog is full (AF_UNIX). There is no readiness to
            // wait for, so pace the retry — the pause still watches the stop.
            libc::EAGAIN => match pace_retry(bound, deadline, CONNECT_RETRY_PACE) {
                Wake::Ready => {}
                Wake::Stopped => return -libc::ECANCELED,
                Wake::TimedOut => return -libc::ETIMEDOUT,
            },
            e => return -e,
        }
    }
}

/// The outcome of a connect that reported writability, read from the socket's
/// own error slot — where a handshake that failed leaves its reason.
fn connect_result(fd: RawFd) -> i32 {
    let mut err: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    let got = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            &mut err as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if got < 0 {
        return -std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
    }
    if err == 0 {
        0
    } else {
        -err
    }
}
