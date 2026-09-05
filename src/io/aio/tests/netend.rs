// audited: 2026-09-05
// src/io/AGENTS.md
//! The endings a pool operation reaches with nobody cancelling it: a close on
//! the port beneath it, its own deadline, and a retirement.

use super::*;

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
                crate::io::pending::Submitter::for_test(),
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
                crate::io::pending::Submitter::for_test(),
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
                crate::io::pending::Submitter::for_test(),
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
            crate::value::sorted_struct_get(fields, &TableKey::keyword("error")).copied(),
            Some(crate::value::Value::keyword("timeout")),
            "a connect that ran out its deadline must report :timeout, not a \
             generic :io-error — `ev/timeout` and `timed-out?` match on the kind",
        );

        for c in queued {
            unsafe { libc::close(c) };
        }
        unsafe { libc::close(listener_fd) };
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
                    crate::io::pending::Submitter::detached(heap_ptr),
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
