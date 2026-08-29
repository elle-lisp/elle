use super::*;

/// Regression test: wait() must not return 0 completions when an accept
/// SQE is in-flight and a connection arrives within the timeout window.
///
/// wait() loops until at least one completion arrives or the deadline passes,
/// so a spurious early return from submit_with_args() (EINTR or spurious
/// wakeup) cannot make it report 0 completions while the accept is still
/// in flight.
#[test]
fn test_accept_wait_does_not_return_zero_completions_spuriously() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        use std::os::unix::io::FromRawFd;
        use std::sync::{Arc, Barrier};

        let listener_fd = unsafe {
            let fd = tcp_listener_socket();
            assert!(fd >= 0);
            let opt: libc::c_int = 1;
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                &opt as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
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
        let bound_port = unsafe {
            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            let mut len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
            libc::getsockname(
                listener_fd,
                &mut addr as *mut _ as *mut libc::sockaddr,
                &mut len,
            );
            u16::from_be(addr.sin_port)
        };
        let listener_port = h.ctx().external(
            "port",
            Port::new_tcp_listener(
                unsafe { std::os::unix::io::OwnedFd::from_raw_fd(listener_fd) },
                format!("127.0.0.1:{}", bound_port),
            ),
        );

        let backend = AsyncBackend::new().unwrap();
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
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        // Use a barrier so the connect happens only after we're about to call wait().
        // This maximises the chance that wait() sees 0 completions on the first
        // drain and must block — the scenario where the spurious-return bug fires.
        let barrier = Arc::new(Barrier::new(2));
        let barrier2 = barrier.clone();
        let handle = std::thread::spawn(move || {
            barrier2.wait(); // released just before wait() is called
            std::net::TcpStream::connect(format!("127.0.0.1:{}", bound_port)).unwrap()
        });

        barrier.wait(); // release the connector thread
                        // wait() must return exactly 1 completion — the accept.
                        // If it returns 0, the bug is confirmed.
        let completions = backend.wait(5000).unwrap();
        assert_eq!(
            completions.len(),
            1,
            "wait() returned {} completions — expected 1 (spurious early return bug)",
            completions.len()
        );
        assert_eq!(completions[0].id, accept_id);
        assert!(completions[0].result.is_ok());
        handle.join().unwrap();
    });
}

#[test]
fn test_accept_via_uring() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        use std::os::unix::io::FromRawFd;

        // Create a TCP listener via libc
        let listener_fd = unsafe {
            let fd = tcp_listener_socket();
            assert!(fd >= 0, "socket() failed");

            let opt: libc::c_int = 1;
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                &opt as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );

            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            addr.sin_family = libc::AF_INET as libc::sa_family_t;
            addr.sin_port = 0; // ephemeral port
            addr.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::LOCALHOST).to_be();

            let ret = libc::bind(
                fd,
                &addr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            );
            assert_eq!(ret, 0, "bind() failed: {}", std::io::Error::last_os_error());

            let ret = libc::listen(fd, 128);
            assert_eq!(ret, 0, "listen() failed");

            fd
        };

        // Get the bound port
        let bound_port = unsafe {
            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            let mut len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
            libc::getsockname(
                listener_fd,
                &mut addr as *mut _ as *mut libc::sockaddr,
                &mut len,
            );
            u16::from_be(addr.sin_port)
        };

        let listener_port = h.ctx().external(
            "port",
            Port::new_tcp_listener(
                unsafe { std::os::unix::io::OwnedFd::from_raw_fd(listener_fd) },
                format!("127.0.0.1:{}", bound_port),
            ),
        );

        let backend = AsyncBackend::new().unwrap();

        // Submit Accept
        let accept_port_val = h.ctx().external(
            "port",
            Port::new_unopened(
                PortKind::TcpStream,
                Direction::ReadWrite,
                Encoding::Binary,
                String::new(),
            ),
        );
        let accept_req = IoRequest {
            op: PortOp::Accept {
                options: Default::default(),
                encoding: crate::port::Encoding::Binary,
                accept_port: accept_port_val,
            }
            .into(),
            port: listener_port,
            timeout: None,
        };
        let accept_id = backend
            .submit(&accept_req, crate::value::arena::leaked_test_heap())
            .unwrap();

        // Connect from a background thread
        let port_copy = bound_port;
        let handle = std::thread::spawn(move || {
            // Small delay to ensure accept is submitted
            std::thread::sleep(std::time::Duration::from_millis(10));
            let _stream = std::net::TcpStream::connect(format!("127.0.0.1:{}", port_copy)).unwrap();
        });

        // Wait for the accept completion
        let completions = backend.wait(5000).unwrap();
        assert_eq!(
            completions.len(),
            1,
            "expected 1 completion, got {}",
            completions.len()
        );
        assert_eq!(completions[0].id, accept_id);
        assert!(
            completions[0].result.is_ok(),
            "accept failed: {:?}",
            completions[0].result
        );

        // The result should be a port
        let accepted = completions[0].result.as_ref().unwrap();
        assert_eq!(
            accepted.external_type_name(),
            Some("port"),
            "expected a port value"
        );

        handle.join().unwrap();
    });
}

#[test]
fn test_connect_via_uring() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        // Create a TCP listener via std
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let bound_addr = listener.local_addr().unwrap();

        // Accept from a background thread so we don't deadlock
        let handle = std::thread::spawn(move || {
            let _accepted = listener.accept().unwrap();
            // Keep the accepted connection alive until the test is done
            std::thread::sleep(std::time::Duration::from_secs(2));
        });

        let backend = AsyncBackend::new().unwrap();

        // Submit Connect
        let connect_port = h.ctx().external(
            "port",
            Port::new_unopened(
                PortKind::TcpStream,
                Direction::ReadWrite,
                Encoding::Binary,
                format!("127.0.0.1:{}", bound_addr.port()),
            ),
        );
        let connect_req = IoRequest {
            op: IoOp::Connect {
                addr: crate::io::request::ConnectAddr::Tcp {
                    addr: "127.0.0.1".parse().unwrap(),
                    port: bound_addr.port(),
                    options: Default::default(),
                    encoding: crate::port::Encoding::Binary,
                },
            },
            port: connect_port,
            timeout: None,
        };
        let connect_id = backend
            .submit(&connect_req, crate::value::arena::leaked_test_heap())
            .unwrap();

        // Wait for the connect completion
        let completions = backend.wait(5000).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, connect_id);
        assert!(
            completions[0].result.is_ok(),
            "connect failed: {:?}",
            completions[0].result
        );

        let connected = completions[0].result.as_ref().unwrap();
        assert_eq!(connected.external_type_name(), Some("port"));

        handle.join().unwrap();
    });
}

/// Accept + connect on the same io_uring ring — the scheduler scenario.
/// One fiber does tcp/accept, another does tcp/connect, both SQEs on
/// the same ring. Both completions must arrive.
#[test]
fn test_accept_and_connect_concurrent() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        use std::os::unix::io::FromRawFd;

        // Create a non-blocking TCP listener via libc
        let listener_fd = unsafe {
            let fd = tcp_listener_socket();
            assert!(fd >= 0);
            let opt: libc::c_int = 1;
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                &opt as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            addr.sin_family = libc::AF_INET as libc::sa_family_t;
            addr.sin_port = 0;
            addr.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::LOCALHOST).to_be();
            libc::bind(
                fd,
                &addr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            );
            libc::listen(fd, 128);
            fd
        };

        let bound_port = unsafe {
            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            let mut len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
            libc::getsockname(
                listener_fd,
                &mut addr as *mut _ as *mut libc::sockaddr,
                &mut len,
            );
            u16::from_be(addr.sin_port)
        };

        let listener_port = h.ctx().external(
            "port",
            Port::new_tcp_listener(
                unsafe { std::os::unix::io::OwnedFd::from_raw_fd(listener_fd) },
                format!("127.0.0.1:{}", bound_port),
            ),
        );

        let backend = AsyncBackend::new().unwrap();

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
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

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
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        // Collect completions — may arrive in 1 or 2 wait calls.
        let mut all = Vec::new();
        for _ in 0..5 {
            let cs = backend.wait(2000).unwrap();
            all.extend(cs);
            if all.len() >= 2 {
                break;
            }
        }

        assert_eq!(all.len(), 2, "expected 2 completions, got {}", all.len());
        for c in &all {
            assert!(c.result.is_ok(), "id={} failed: {:?}", c.id, c.result);
        }
        let ids: Vec<SubmissionId> = all.iter().map(|c| c.id).collect();
        assert!(ids.contains(&accept_id), "missing accept");
        assert!(ids.contains(&connect_id), "missing connect");
    });
}

/// Take a descriptor non-blocking, whatever the platform spells the flag.
/// `SOCK_NONBLOCK` is a Linux extension to `socket(2)`; `fcntl` is everywhere.
fn set_nonblocking(fd: libc::c_int) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        assert!(flags >= 0, "F_GETFL failed");
        assert_eq!(libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK), 0);
    }
}

/// A loopback TCP listener with a backlog of one, and the port it took.
///
/// The small backlog is the point: paired with `fill_tcp_backlog`, it gives a
/// listener the kernel refuses further connections to, which is a peer that
/// never answers without needing a network to reach one.
fn full_backlog_listener() -> (libc::c_int, u16) {
    unsafe {
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
            0,
            "bind() failed"
        );
        assert_eq!(libc::listen(fd, 1), 0, "listen() failed");
        let mut bound: libc::sockaddr_in = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        libc::getsockname(fd, &mut bound as *mut _ as *mut libc::sockaddr, &mut len);
        (fd, u16::from_be(bound.sin_port))
    }
}

/// Fill the accept queue of the listener on `port`, and return the descriptors
/// that hold it full. Nobody accepts these, so the kernel has nowhere to put a
/// further connection and drops its SYNs.
///
/// The caller holds the descriptors for the length of its test: closing one
/// frees a queue slot, and the connect under test would then complete.
fn fill_tcp_backlog(port: u16) -> Vec<libc::c_int> {
    let mut queued = Vec::new();
    for _ in 0..8 {
        let c = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
        assert!(c >= 0, "socket() failed");
        set_nonblocking(c);
        unsafe {
            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            addr.sin_family = libc::AF_INET as libc::sa_family_t;
            addr.sin_port = port.to_be();
            addr.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::LOCALHOST).to_be();
            libc::connect(
                c,
                &addr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            );
        }
        queued.push(c);
    }
    queued
}

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
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        assert_cancel_retires(&backend, accept_id, "accept");
    });
}

/// Closing a listener must END its parked pool accept, on every platform.
///
/// `port/close` is the only unblocking mechanism a program has for an accept
/// nobody cancels — an accept loop parked in a live process, closed by another
/// process at teardown (tests/elle/process-accept-close.lisp is the scheduler
/// shape). The close path may not lean on `shutdown(2)` for this: shutdown of
/// a LISTENING socket is a Linux extension — macOS and the BSDs return
/// ENOTCONN and wake nothing, and the accept's worker then polls the retired
/// descriptor forever while the scheduler waits on a completion that never
/// comes. The wake must come from the operation's stop pipe instead.
///
/// Built on `new_thread_pool` for the reason the cancellation tests give: on a
/// Linux dev box the default backend is the ring, and this property would go
/// unchecked everywhere it can regress.
#[test]
fn closing_a_listener_ends_its_parked_pool_accept() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        use std::os::unix::io::FromRawFd;

        // A BLOCKING listener, deliberately — see the cancellation test above.
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
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        // Let the worker reach its wait before the close arrives.
        let mut submitted = false;
        for _ in 0..200 {
            if backend.workers() > 0 {
                submitted = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(submitted, "the pool never took the accept out to a worker");
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Close the listener. The close itself completes immediately; the
        // parked accept must then complete too — as an error, within a bound.
        let close_id = backend
            .submit(
                &IoRequest {
                    op: IoOp::Close,
                    port: listener_port,
                    timeout: None,
                },
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        let mut accept_completion = None;
        for _ in 0..40 {
            for c in backend.wait(50).unwrap() {
                if c.id == accept_id {
                    accept_completion = Some(c);
                } else {
                    assert_eq!(c.id, close_id, "unexpected completion");
                }
            }
            if accept_completion.is_some() {
                break;
            }
        }
        let accept_completion = accept_completion.expect(
            "the parked accept never completed after its listener closed — \
             the fiber waiting on it would wait forever",
        );
        assert!(
            accept_completion.result.is_err(),
            "an accept on a closed listener must not report success",
        );
        assert_eq!(
            backend.workers(),
            0,
            "the accept's worker never came back after the close",
        );
        assert!(
            !backend.has_pending(),
            "the accept is still pending after the close",
        );
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
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        assert_cancel_retires(&backend, recv_id, "recvfrom");
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
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        assert_cancel_retires(&backend, connect_id, "tcp connect");

        for c in queued {
            unsafe { libc::close(c) };
        }
        unsafe { libc::close(listener_fd) };
    });
}

/// A pool connect must stop at the caller's `:timeout`, and say so.
///
/// The same full accept queue as the cancellation test above, waited on with a
/// deadline instead of cancelled. Two things are pinned: the connect ends near
/// its deadline rather than at the kernel's own, minutes later; and it reports
/// `:timeout`, the kind `ev/timeout` and every caller that distinguishes a
/// deadline from a broken connection matches on.
#[test]
fn a_pool_connect_reports_its_own_deadline_as_a_timeout() {
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
        let started = std::time::Instant::now();
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
                    timeout: Some(std::time::Duration::from_millis(200)),
                },
                crate::value::arena::leaked_test_heap(),
            )
            .unwrap();

        let mut completions = Vec::new();
        for _ in 0..40 {
            completions.extend(backend.wait(200).unwrap());
            if !completions.is_empty() {
                break;
            }
        }
        assert_eq!(
            completions.len(),
            1,
            "the connect never completed within 8s of a 200ms deadline",
        );
        assert_eq!(completions[0].id, connect_id);
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "the connect took {:?} against a 200ms deadline — it waited on the \
             kernel's own retry sequence instead of its own bound",
            elapsed,
        );

        let err = completions[0]
            .result
            .as_ref()
            .expect_err("a connect to a full accept queue must not succeed");
        let fields = err.as_struct().expect("an io error is a struct");
        assert_eq!(
            crate::value::sorted_struct_get(fields, &TableKey::Keyword("error".into()))
                .unwrap()
                .as_keyword_name()
                .as_deref(),
            Some("timeout"),
            "a connect that ran out its deadline must report :timeout, not a \
             generic :io-error — `ev/timeout` and `timed-out?` match on the kind",
        );

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
        // to a full queue reports EAGAIN — so the setup can prove the connect
        // under test really has nothing to complete against.
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
            // A full AF_UNIX backlog names itself differently per platform:
            // `EAGAIN` on Linux, `ECONNREFUSED` on macOS and the BSDs. Each
            // platform is pinned to its own answer rather than to the union of
            // both, so a platform that changes its answer still fails here.
            #[cfg(target_os = "linux")]
            let (expected, expected_name): (&[libc::c_int], &str) =
                (&[libc::EAGAIN, libc::EWOULDBLOCK], "EAGAIN/EWOULDBLOCK");
            #[cfg(not(target_os = "linux"))]
            let (expected, expected_name): (&[libc::c_int], &str) =
                (&[libc::ECONNREFUSED], "ECONNREFUSED");
            assert!(
                expected.contains(&errno),
                "connect({}) failed with errno {}, expected {}",
                path,
                errno,
                expected_name
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
                crate::value::arena::leaked_test_heap(),
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

/// A retired accept gives back the connection it took.
///
/// An accept that succeeded owns a descriptor: the connection the kernel
/// handed it. Every other retiring arm gives its descriptor back — a connect
/// closes the socket it pre-created, an open closes the file it opened — and
/// an accept must too, or a server whose accept loop ends leaks one socket per
/// round it had in flight.
///
/// The trap: an fd count cannot say this. Tests share a process and run in
/// parallel, so the number moves under the measurement. The peer can say it
/// instead — a connection nobody closed leaves the peer's read waiting, while
/// a closed one ends it.
#[test]
fn a_retired_accept_closes_the_connection_it_took() {
    use std::io::Read;
    crate::value::arena::with_test_region(|| {
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            let addr = listener.local_addr().expect("local_addr");

            let heap_ptr = crate::value::arena::leaked_test_heap();
            // SAFETY: the heap is leaked for the process.
            let heap = unsafe { &mut *heap_ptr };

            // The listener outlives the operation; only the fiber's own region
            // goes, which is what makes the entry retire unread.
            let kept = heap.new_runtime_region();
            let listener_port = crate::value::build::external(
                heap,
                "port",
                Port::new_tcp_listener(listener.into(), addr.to_string()),
                kept,
            );
            let region = heap.new_runtime_region();
            let accept_port = crate::value::build::external(
                heap,
                "port",
                Port::new_unopened(
                    PortKind::TcpStream,
                    Direction::ReadWrite,
                    Encoding::Binary,
                    String::new(),
                ),
                region,
            );

            // The peer connects BEFORE the accept is submitted, so the kernel
            // has a connection queued and the operation takes one the moment it
            // runs. An accept retired before it has a connection owns no
            // descriptor and would close nothing, which is not the case under
            // test.
            let client = std::net::TcpStream::connect(addr).expect("connect");

            backend
                .submit(
                    &IoRequest {
                        op: PortOp::Accept {
                            options: Default::default(),
                            encoding: Encoding::Binary,
                            accept_port,
                        }
                        .into(),
                        port: listener_port,
                        timeout: None,
                    },
                    heap_ptr,
                )
                .unwrap();

            // Let the operation reach its completion before the region goes.
            // Nothing consumes a completion outside `wait`/`poll`, so the answer
            // sits in the ring or the hub across this settle and is taken below
            // — after the release, which is the order that makes the entry
            // retire with a descriptor in hand.
            wait_for_worker(&backend);

            // The fiber ends: its region goes, and with it the port the accept
            // would have filled.
            heap.decref_region(region);

            for _ in 0..40 {
                let _ = backend.wait(50).unwrap();
                if !backend.has_pending() && backend.workers() == 0 {
                    break;
                }
            }

            client
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set_read_timeout");
            // The trap: this read is interrupted, not just bounded. The runtime
            // signals its own threads, and `std::net`'s `read` reports `EINTR`
            // rather than retrying — so a single call reports "the peer's read
            // never ended" whatever the descriptor did. Retry until the
            // descriptor answers or the deadline passes, and let only the
            // deadline mean the connection is still open.
            let mut buf = [0u8; 1];
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let ended = loop {
                if std::time::Instant::now() >= deadline {
                    break false;
                }
                match (&client).read(&mut buf) {
                    Ok(0) => break true,
                    Ok(n) => panic!("{which}: the peer read {n} bytes from a retired accept"),
                    Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break true,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break false,
                    Err(e) => panic!("{which}: the peer's read failed with {e}"),
                }
            };
            assert!(
                ended,
                "{which}: the peer's read never ended — the accept was retired \
                 but the connection it took was never closed",
            );
        }
    });
}
