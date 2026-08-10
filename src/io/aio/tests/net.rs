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
            let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0);
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
            let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0);
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
            let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0);
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

/// A cancelled accept must END on the thread-pool backend, not be abandoned.
///
/// The pool's `wait` blocks on the hub channel only `if hub.in_flight() > 0`.
/// The io_uring arm has no such guard — it waits on the ring unconditionally,
/// and says so: "a genuinely lost wakeup hangs rather than being downgraded to
/// a bounded stall". So the pool has a state the ring does not: an operation
/// still in `pending` while no worker is out for it. `wait` then returns
/// nothing without blocking, and whoever is parked on that operation is never
/// woken again.
///
/// Closing a listener under a parked accept is how a program reaches that
/// state, so this pins that such an accept RETIRES: its worker comes back and
/// its `pending` entry goes, within a bounded number of waits.
///
/// No completion is delivered for it, and that is the design rather than an
/// omission — `cook_raw` discards a cancelled op before cooking it, because the
/// fiber that requested it is already gone and cooking a read would write the
/// worker's bytes into a freed heap. What must not happen is the entry
/// outliving the worker.
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

        // Let the worker actually pick the accept up and park in accept(), so
        // the cancel below interrupts a blocked syscall rather than racing the
        // hand-off.
        let picked_up = {
            let mut spun = false;
            for _ in 0..200 {
                if backend.workers() > 0 {
                    spun = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            spun
        };
        assert!(picked_up, "the pool never took the accept out to a worker");

        backend.cancel(accept_id).unwrap();

        // A bounded number of waits, because the property under test is exactly
        // that this terminates: an operation left in `pending` with no worker
        // out would leave `wait` returning nothing for as long as it is asked.
        for _ in 0..40 {
            let _ = backend.wait(50).unwrap();
            if !backend.has_pending() && backend.workers() == 0 {
                break;
            }
        }

        assert!(
            !backend.has_pending(),
            "the cancelled accept is still pending with {} worker(s) out — an \
             operation that keeps its `pending` entry after its worker is gone \
             can never be reaped, because the pool's `wait` blocks only while \
             `in_flight() > 0`",
            backend.workers(),
        );
        assert_eq!(
            backend.workers(),
            0,
            "the cancelled accept never gave its worker back",
        );
    });
}
