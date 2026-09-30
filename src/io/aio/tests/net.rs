// audited: 2026-09-30
// src/io/AGENTS.md
//! Accept and connect on the platform's default backend, including both in
//! flight on one backend at once.

use super::*;

/// A timed wait() returns the accept's completion when a connection arrives
/// inside its window.
///
/// The trap: the ring's `submit_with_args` can return early on `EINTR`, and a
/// wait that took that for the end of its window would report no completions
/// while the accept is still in flight.
#[test]
fn a_timed_wait_returns_the_accept_a_peer_arrives_for() {
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
                &IoRequest::unbounded(
                    PortOp::Accept {
                        options: Default::default(),
                        encoding: crate::port::Encoding::Binary,
                        accept_port: accept_port_val,
                    }
                    .into(),
                    listener_port,
                ),
                crate::io::pending::Submitter::for_test(),
            )
            .unwrap();

        // A barrier, so the connect happens only once wait() is about to run.
        // That makes wait() see no completion on its first drain and block,
        // which is the wait under test.
        let barrier = Arc::new(Barrier::new(2));
        let barrier2 = barrier.clone();
        let handle = std::thread::spawn(move || {
            barrier2.wait(); // released just before wait() is called
            std::net::TcpStream::connect(format!("127.0.0.1:{}", bound_port)).unwrap()
        });

        barrier.wait(); // release the connector thread
        let completions = backend.wait(5000).unwrap();
        assert_eq!(
            completions.len(),
            1,
            "wait() returned {} completions — expected the accept's one",
            completions.len()
        );
        assert_eq!(completions[0].id, accept_id);
        assert!(completions[0].result.is_ok());
        handle.join().unwrap();
    });
}

#[test]
fn an_accept_completes_when_a_peer_connects() {
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
        let accept_req = IoRequest::unbounded(
            PortOp::Accept {
                options: Default::default(),
                encoding: crate::port::Encoding::Binary,
                accept_port: accept_port_val,
            }
            .into(),
            listener_port,
        );
        let accept_id = backend
            .submit(&accept_req, crate::io::pending::Submitter::for_test())
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
fn a_connect_completes_when_the_listener_accepts() {
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
        let connect_req = IoRequest::unbounded(
            IoOp::Connect {
                addr: crate::io::request::ConnectAddr::Tcp {
                    addr: "127.0.0.1".parse().unwrap(),
                    port: bound_addr.port(),
                    options: Default::default(),
                    encoding: crate::port::Encoding::Binary,
                },
            },
            connect_port,
        );
        let connect_id = backend
            .submit(&connect_req, crate::io::pending::Submitter::for_test())
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

/// Accept and connect in flight on one backend at once — the scheduler's case.
/// One fiber does tcp/accept, another does tcp/connect, both SQEs on
/// the same ring. Both completions must arrive.
#[test]
fn test_accept_and_connect_concurrent() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        use std::os::unix::io::FromRawFd;

        // A TCP listener via libc
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
                &IoRequest::unbounded(
                    PortOp::Accept {
                        options: Default::default(),
                        encoding: crate::port::Encoding::Binary,
                        accept_port: accept_port_val,
                    }
                    .into(),
                    listener_port,
                ),
                crate::io::pending::Submitter::for_test(),
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
                &IoRequest::unbounded(
                    IoOp::Connect {
                        addr: crate::io::request::ConnectAddr::Tcp {
                            addr: "127.0.0.1".parse().unwrap(),
                            port: bound_port,
                            options: Default::default(),
                            encoding: crate::port::Encoding::Binary,
                        },
                    },
                    connect_port,
                ),
                crate::io::pending::Submitter::for_test(),
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
