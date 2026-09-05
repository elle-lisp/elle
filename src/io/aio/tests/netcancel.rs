// audited: 2026-09-05
// src/io/AGENTS.md
//! A cancelled pool operation ends rather than being abandoned — one test for
//! each operation that can wait on a peer who never comes.
//!
//! Each is built on `new_thread_pool` rather than `AsyncBackend::new`: on a
//! Linux host with io_uring the default backend is the ring, so these
//! properties would go unchecked on every dev box while only CI and the
//! non-Linux builds ran the code they are about.

use super::*;

/// A cancelled accept must END on the thread-pool backend, not be abandoned.
///
/// Closing a listener under a parked accept is how a program reaches the state
/// `assert_cancel_retires` describes: an entry whose worker is gone for good.
///
/// Built on `new_thread_pool` rather than `AsyncBackend::new` on purpose: on a
/// Linux host with io_uring the default backend is the ring, and this property
/// would go unchecked on every dev box while only CI (and every non-Linux
/// build, which has no other arm) ran the code it is about.
#[test]
fn a_cancelled_pool_accept_ends_rather_than_being_abandoned() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        use std::os::unix::io::FromRawFd;

        // A BLOCKING listener, deliberately. With SOCK_NONBLOCK the worker's
        // `accept` returns EAGAIN at once and the operation ends whatever the
        // cancel path does — the test would pass without ever putting a worker
        // in the blocking `accept` this is about.
        let listener_fd = unsafe {
            let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
            assert!(fd >= 0, "socket() failed");
            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            addr.sin_family = libc::AF_INET as libc::sa_family_t;
            addr.sin_port = 0;
            addr.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::LOCALHOST).to_be();
            assert_eq!(
                libc::bind(
                    fd,
                    &addr as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t
                ),
                0
            );
            assert_eq!(libc::listen(fd, 128), 0);
            fd
        };
        let listener_port = h.ctx().external(
            "port",
            Port::new_tcp_listener(
                unsafe { std::os::unix::io::OwnedFd::from_raw_fd(listener_fd) },
                "127.0.0.1:0".to_string(),
            ),
        );

        let backend = AsyncBackend::new_thread_pool().unwrap();
        let accept_port_val = h.ctx().external(
            "port",
            Port::new_unopened(
                PortKind::TcpStream,
                Direction::ReadWrite,
                Encoding::Binary,
                String::new(),
            ),
        );
        let accept_id = backend
            .submit(
                &IoRequest {
                    op: PortOp::Accept {
                        options: Default::default(),
                        encoding: crate::port::Encoding::Binary,
                        accept_port: accept_port_val,
                    }
                    .into(),
                    port: listener_port,
                    timeout: None,
                },
                crate::io::pending::Submitter::for_test(),
            )
            .unwrap();

        assert_cancel_retires(&backend, accept_id, "accept");
    });
}

/// A cancelled datagram receive must END on the thread-pool backend.
///
/// The accept test's twin on the other open-ended socket operation: a socket
/// nobody sends to waits exactly as long as a listener nobody calls. `ev/timeout`
/// around a `udp/recv-from` is the caller that meets it — `lib/dns.lisp` sends a
/// query and waits for a reply that a lossy network need never deliver.
///
/// The socket is deliberately BLOCKING, for the reason the accept test gives:
/// a non-blocking one returns EAGAIN at once and the operation ends whatever
/// the cancel path does.
#[test]
fn a_cancelled_pool_recvfrom_ends_rather_than_being_abandoned() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        use std::os::unix::io::FromRawFd;

        let sock_fd = unsafe {
            let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
            assert!(fd >= 0, "socket() failed");
            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            addr.sin_family = libc::AF_INET as libc::sa_family_t;
            addr.sin_port = 0;
            addr.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::LOCALHOST).to_be();
            assert_eq!(
                libc::bind(
                    fd,
                    &addr as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t
                ),
                0
            );
            fd
        };
        let sock_port = h.ctx().external(
            "port",
            Port::new_udp_socket(
                unsafe { std::os::unix::io::OwnedFd::from_raw_fd(sock_fd) },
                "127.0.0.1:0".to_string(),
            ),
        );

        let backend = AsyncBackend::new_thread_pool().unwrap();
        let recv_id = backend
            .submit(
                &IoRequest {
                    // The pool worker receives into its own buffer, so the
                    // destination struct a fiber would pass is not needed here.
                    op: PortOp::RecvFrom {
                        count: 64,
                        result: Value::NIL,
                    }
                    .into(),
                    port: sock_port,
                    timeout: None,
                },
                crate::io::pending::Submitter::for_test(),
            )
            .unwrap();

        assert_cancel_retires(&backend, recv_id, "recvfrom");
    });
}

/// A cancelled write must END on the thread-pool backend.
///
/// The full-write invariant makes a write run to the end of its payload, and a
/// payload larger than the send buffer only gets there as the peer takes what is
/// already in it. So a write waits on a peer exactly as an accept and a
/// `recvfrom` do, and a peer that stops reading is the wait that never ends.
/// `ev/timeout` around a `port/write` is the caller that meets it.
///
/// The trap: the peer socket must stay OPEN. Closing it ends the write with
/// `EPIPE` for a reason that has nothing to do with cancellation, and the test
/// would then pass with the stop pipe taken away again.
///
/// Counter-factual: with the write submitted under `Bounds::new(timeout, None)`,
/// `OpBound::new` finds neither a deadline nor a stop to enforce and leaves the
/// descriptor blocking, so the worker sits inside `write(2)` where no poll and no
/// stop can reach it. `assert_cancel_retires` then fails on the worker that never
/// comes back.
#[test]
fn a_cancelled_pool_write_ends_rather_than_being_abandoned() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        use std::os::unix::io::FromRawFd;

        // A socketpair is the smallest peer that can stop reading: one end is
        // the port, the other is a peer this test simply never touches.
        let mut fds = [0 as libc::c_int; 2];
        assert_eq!(
            unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) },
            0,
            "socketpair() failed"
        );
        let (write_fd, peer_fd) = (fds[0], fds[1]);

        // Shrink the send buffer so the payload below cannot fit in it whatever
        // the platform's default happens to be. The kernel is free to round this
        // up, which is why the payload is larger than any plausible rounding.
        let want: libc::c_int = 4096;
        unsafe {
            libc::setsockopt(
                write_fd,
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                &want as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
            libc::setsockopt(
                peer_fd,
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                &want as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
        }

        let port = h.ctx().external(
            "port",
            Port::new_unix_stream(
                unsafe { std::os::unix::io::OwnedFd::from_raw_fd(write_fd) },
                "socketpair".to_string(),
            ),
        );

        let backend = AsyncBackend::new_thread_pool().unwrap();
        let data = h.ctx().bytes(vec![b'x'; 4 << 20]);
        let write_id = backend
            .submit(
                &IoRequest {
                    // No `:timeout`: the deadline is the ending this test must
                    // not be able to reach for. Cancellation is the only one
                    // left, and it is the one under test.
                    op: PortOp::Write { data }.into(),
                    port,
                    timeout: None,
                },
                crate::io::pending::Submitter::for_test(),
            )
            .unwrap();

        assert_cancel_retires(&backend, write_id, "write");

        unsafe { libc::close(peer_fd) };
    });
}

/// A cancelled TCP connect must END on the thread-pool backend.
///
/// The stall is a listener whose accept queue is full: the kernel drops further
/// SYNs, so the handshake never completes and the connect waits on a peer that
/// will not answer. A blocking `connect(2)` holds its worker through the whole
/// SYN-retry sequence — minutes, with no way for a cancel to reach it.
#[test]
fn a_cancelled_pool_tcp_connect_ends_rather_than_being_abandoned() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();

        let (listener_fd, bound_port) = full_backlog_listener();
        let queued = fill_tcp_backlog(bound_port);

        let backend = AsyncBackend::new_thread_pool().unwrap();
        let connect_port = h.ctx().external(
            "port",
            Port::new_unopened(
                PortKind::TcpStream,
                Direction::ReadWrite,
                Encoding::Binary,
                format!("127.0.0.1:{}", bound_port),
            ),
        );
        let connect_id = backend
            .submit(
                &IoRequest {
                    op: IoOp::Connect {
                        addr: crate::io::request::ConnectAddr::Tcp {
                            addr: "127.0.0.1".parse().unwrap(),
                            port: bound_port,
                            options: Default::default(),
                            encoding: crate::port::Encoding::Binary,
                        },
                    },
                    port: connect_port,
                    timeout: None,
                },
                crate::io::pending::Submitter::for_test(),
            )
            .unwrap();

        assert_cancel_retires(&backend, connect_id, "tcp connect");

        for c in queued {
            unsafe { libc::close(c) };
        }
        unsafe { libc::close(listener_fd) };
    });
}

/// A cancelled Unix connect must END on the thread-pool backend.
///
/// AF_UNIX reports a full backlog differently from TCP, which is why it is
/// pinned separately: a non-blocking connect returns EAGAIN with no readiness
/// to poll for, so the operation paces its retries and watches for the stop
/// between them. A blocking one waits inside the kernel until the listener
/// accepts, which a listener that has stopped accepting never does.
///
/// The two platforms name the full backlog differently — Linux `EAGAIN`, macOS
/// and the BSDs `ECONNREFUSED` — and the connect paces both, so this runs
/// everywhere rather than on Linux alone.
#[test]
fn a_cancelled_pool_unix_connect_ends_rather_than_being_abandoned() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();

        let path = temp_path("unix-connect-cancel");
        let (sun, addr_len) = crate::io::sockaddr::build_unix(&path).unwrap();
        let listener_fd = unsafe {
            let fd = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0);
            assert!(fd >= 0, "socket() failed");
            assert_eq!(
                libc::bind(fd, &sun as *const _ as *const libc::sockaddr, addr_len),
                0,
                "bind({}) failed",
                path
            );
            assert_eq!(libc::listen(fd, 1), 0);
            fd
        };

        // Fill the backlog. AF_UNIX says so directly — a non-blocking connect
        // to a full queue reports it rather than waiting — so the setup can
        // prove the connect under test really has nothing to complete against.
        let mut queued: Vec<libc::c_int> = Vec::new();
        let mut full = false;
        for _ in 0..8 {
            let c = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
            assert!(c >= 0, "socket() failed");
            set_nonblocking(c);
            let r =
                unsafe { libc::connect(c, &sun as *const _ as *const libc::sockaddr, addr_len) };
            if r == 0 {
                queued.push(c);
                continue;
            }
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            unsafe { libc::close(c) };
            // Each platform is held to its own errno rather than to the union
            // of both. A union would still pass if one platform started
            // answering the other's way, which is the change worth catching.
            #[cfg(target_os = "linux")]
            let (queue_is_full, expected) = (
                errno == libc::EAGAIN || errno == libc::EWOULDBLOCK,
                "EAGAIN/EWOULDBLOCK",
            );
            #[cfg(not(target_os = "linux"))]
            let (queue_is_full, expected) = (errno == libc::ECONNREFUSED, "ECONNREFUSED");
            assert!(
                queue_is_full,
                "connect({}) failed with errno {}, expected {}",
                path, errno, expected
            );
            full = true;
            break;
        }
        assert!(
            full,
            "the listener's backlog never filled, so the connect under test \
             would complete instead of waiting"
        );

        let backend = AsyncBackend::new_thread_pool().unwrap();
        let connect_port = h.ctx().external(
            "port",
            Port::new_unopened(
                PortKind::UnixStream,
                Direction::ReadWrite,
                Encoding::Binary,
                path.clone(),
            ),
        );
        let connect_id = backend
            .submit(
                &IoRequest {
                    op: IoOp::Connect {
                        addr: crate::io::request::ConnectAddr::Unix {
                            path: path.clone(),
                            options: Default::default(),
                            encoding: crate::port::Encoding::Binary,
                        },
                    },
                    port: connect_port,
                    timeout: None,
                },
                crate::io::pending::Submitter::for_test(),
            )
            .unwrap();

        assert_cancel_retires(&backend, connect_id, "unix connect");

        for c in queued {
            unsafe { libc::close(c) };
        }
        unsafe { libc::close(listener_fd) };
        let _ = std::fs::remove_file(&path);
    });
}
