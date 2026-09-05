// audited: 2026-09-05
// src/io/AGENTS.md
//! What a connect makes of a refusal, and the platform behavior it rests on.

use super::file_path;
use crate::io::threadpool::net::Refusal;

/// An AF_UNIX socket bound at `path`, listening, with a backlog of one.
/// The caller closes the descriptor and removes the path.
fn bound_unix_listener(path: &str) -> libc::c_int {
    let (sun, addr_len) = crate::io::sockaddr::build_unix(path).expect("build_unix");
    unsafe {
        let fd = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0);
        assert!(fd >= 0, "socket() failed");
        assert_eq!(
            libc::bind(fd, &sun as *const _ as *const libc::sockaddr, addr_len),
            0,
            "bind({path}) failed: {}",
            std::io::Error::last_os_error()
        );
        assert_eq!(libc::listen(fd, 1), 0, "listen() failed");
        fd
    }
}

/// A refused Unix connect paces only while the path still names a socket.
///
/// The trap: macOS and the BSDs spend one errno on both readings of a refusal
/// — a listener whose backlog is full, and no listener at all. Nothing in the
/// errno separates them, so the socket the path names is what decides, and
/// this is where that decision is made. A Linux test cannot reach the connect
/// loop that consumes it, because Linux reports the full backlog as `EAGAIN`
/// and never asks.
///
/// Counter-factual: a refusal that answered from the errno alone would pace
/// every one of them, and `unix/connect` to a path whose daemon died would
/// wait out the caller's whole `:timeout` instead of answering at once.
#[test]
fn a_unix_refusal_paces_only_while_the_path_names_a_socket() {
    let path = file_path("unix-refusal");
    let _ = std::fs::remove_file(&path);

    assert!(
        !Refusal::WhileBound(path.clone()).may_clear(),
        "nothing holds {path}, so a refusal from it is the peer's own answer"
    );

    std::fs::write(&path, b"not a socket").expect("write");
    assert!(
        !Refusal::WhileBound(path.clone()).may_clear(),
        "a regular file at {path} is not a listener that might be busy"
    );
    std::fs::remove_file(&path).expect("remove");

    let listener = bound_unix_listener(&path);
    assert!(
        Refusal::WhileBound(path.clone()).may_clear(),
        "a live listener at {path} refuses when its backlog is full, and that \
         clears as soon as it accepts — the connect must ask again"
    );

    // The listener goes, and the socket file goes with it. A connect that was
    // pacing against this path has nothing left to wait for.
    unsafe { libc::close(listener) };
    std::fs::remove_file(&path).expect("remove");
    assert!(
        !Refusal::WhileBound(path.clone()).may_clear(),
        "the socket at {path} is gone, so the refusal it was pacing is final"
    );
}

/// A Unix connect reads a refusal the way its own platform reports a backlog.
///
/// Linux carries a full backlog in `EAGAIN`, so `ECONNREFUSED` there is the
/// peer's whole answer and pacing it would delay every dead-socket connect for
/// nothing. Elsewhere the one errno covers both, and the path decides.
///
/// This assertion has no counter-factual on Linux, and cannot: the change this
/// pins is a no-op there, so the right answer and the unchanged answer are the
/// same. It fails on macOS and the BSDs, which is where the behavior lives.
#[test]
fn a_unix_refusal_is_final_exactly_where_a_full_backlog_reports_eagain() {
    let path = file_path("unix-refusal-platform");
    let _ = std::fs::remove_file(&path);
    let listener = bound_unix_listener(&path);

    assert_eq!(
        Refusal::for_unix(&path).may_clear(),
        !cfg!(target_os = "linux"),
        "a Unix connect must pace a refusal exactly where the platform spends \
         ECONNREFUSED on a full backlog as well as on a missing listener"
    );

    unsafe { libc::close(listener) };
    let _ = std::fs::remove_file(&path);
}

/// A TCP connect never paces a refusal, on any platform.
///
/// A refused TCP connect is a reset from a host with no listener there. A full
/// accept queue is not reported that way — the kernel drops the SYN and the
/// connect waits — so there is no second reading for a path to settle.
#[test]
fn a_tcp_connect_never_paces_a_refusal() {
    assert!(
        !Refusal::for_tcp().may_clear(),
        "a TCP refusal is the peer's answer; pacing it would hold every \
         connection to a dead port open until the caller's deadline"
    );
}

/// A refused AF_UNIX connect leaves its descriptor connectable.
///
/// The trap: the pace loop asks again on the descriptor it already has, rather
/// than opening a fresh socket per attempt. A platform that latched the
/// refusal onto the socket would answer the second ask with some other errno,
/// and the connect would report that instead of the refusal it was pacing.
///
/// A socket that is bound and never listened on refuses on every platform, so
/// this runs everywhere — which is the point, because the assumption has to
/// hold on the platforms whose connects will do the pacing.
#[test]
fn a_refused_unix_connect_can_be_asked_again_on_the_same_descriptor() {
    let path = file_path("unix-refuse-retry");
    let _ = std::fs::remove_file(&path);
    let (sun, addr_len) = crate::io::sockaddr::build_unix(&path).expect("build_unix");

    // Bound, and deliberately never listened on: `connect(2)` refuses.
    let listener = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    assert!(listener >= 0, "socket() failed");
    assert_eq!(
        unsafe {
            libc::bind(
                listener,
                &sun as *const _ as *const libc::sockaddr,
                addr_len,
            )
        },
        0,
        "bind({path}) failed: {}",
        std::io::Error::last_os_error()
    );

    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");
    let mut errnos = Vec::new();
    for _ in 0..2 {
        let r = unsafe { libc::connect(fd, &sun as *const _ as *const libc::sockaddr, addr_len) };
        assert_eq!(r, -1, "a connect to a socket nobody listens on must fail");
        errnos.push(std::io::Error::last_os_error().raw_os_error().unwrap_or(1));
    }

    assert_eq!(
        errnos[0],
        libc::ECONNREFUSED,
        "a connect to a bound socket with no listener must report ECONNREFUSED"
    );
    assert_eq!(
        errnos[1], errnos[0],
        "the second ask on the same descriptor reported errno {} where the \
         first reported {} — a paced connect would answer with that instead \
         of the refusal it was pacing",
        errnos[1], errnos[0]
    );

    unsafe {
        libc::close(fd);
        libc::close(listener);
    }
    let _ = std::fs::remove_file(&path);
}
