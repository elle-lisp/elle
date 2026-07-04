use super::*;
use crate::io::pool::BufferPool;
use crate::io::request::SocketOptions;
use crate::io::sigfd::SignalReceiver;

/// End-to-end regression: a SIGUSR1 delivered to the process must
/// surface as a CQE on the io_uring instance via the dedicated
/// `submit_uring_sig_next` helper, with no threadpool worker
/// involved on the elle side.
///
/// This is the production Linux path: `submit_sig_next` (in
/// `src/io/aio.rs`) on the `PlatformBackend::Uring` arm calls
/// `submit_uring_sig_next`, the kernel completes the signalfd read
/// asynchronously, and the resulting CQE flows through
/// `drain_cqes` → `PendingOp::SigNext` → `parse_events`. The diff
/// for #856 used `submit_uring_watch_next` (the fs-watcher helper)
/// here, which was structurally correct but hid the io_uring +
/// signalfd path inside an unrelated abstraction. This test pins
/// the dedicated path so a future refactor can't quietly drop us
/// back onto the threadpool fallback without the build breaking.
///
/// Forks so the child has a clean thread topology: post-fork there
/// is one thread, SIGUSR1 is blocked on it (via
/// `SignalReceiver::new`), and the kernel parks the kill() on the
/// process pending queue where signalfd can read it. In the cargo
/// test runner without the fork, peer threads with SIGUSR1
/// unmasked would absorb the kill before our io_uring read sees
/// it. Child exits 0 on success, small positive code on failure.
#[test]
fn sig_next_via_uring_returns_after_kill_to_self() {
    use std::time::{Duration, Instant};

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        panic!("fork failed: {}", std::io::Error::last_os_error());
    }
    if pid == 0 {
        let code = sig_next_uring_child_logic();
        unsafe { libc::_exit(code) };
    }

    // PARENT: bounded waitpid so an io_uring regression (CQE never
    // arrives, ring fd closed early, helper rewires onto something
    // that doesn't actually submit, etc.) surfaces as a hung child
    // panic rather than wedging the whole `cargo test` run.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut status: libc::c_int = 0;
    loop {
        let wret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if wret == pid {
            break;
        }
        if wret < 0 {
            let errno = std::io::Error::last_os_error();
            panic!("waitpid({}): {}", pid, errno);
        }
        if Instant::now() >= deadline {
            unsafe { libc::kill(pid, libc::SIGKILL) };
            let _ = unsafe { libc::waitpid(pid, &mut status, 0) };
            panic!("sig_next via uring child hung past 10s");
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    if libc::WIFSIGNALED(status) {
        panic!(
            "sig_next via uring child died from signal {}",
            libc::WTERMSIG(status)
        );
    }
    let code = libc::WEXITSTATUS(status);
    assert_eq!(
        code, 0,
        "sig_next via uring child failed with code {} (see codes in sig_next_uring_child_logic)",
        code
    );
}

/// Body of the forked child for the io_uring sig-next test.
/// Returns small positive codes identifying the failing step so
/// the parent's panic message points at the broken kernel call.
fn sig_next_uring_child_logic() -> i32 {
    let r = match SignalReceiver::new(vec![libc::SIGUSR1]) {
        Ok(r) => r,
        Err(_) => return 31,
    };
    let fd = match r.raw_fd() {
        Ok(f) => f,
        Err(_) => return 32,
    };

    let mut ring = match io_uring::IoUring::new(8) {
        Ok(ring) => ring,
        // If io_uring_setup fails on this host kernel we have
        // nothing to test — skip with success rather than
        // pretending we covered the path.
        Err(_) => return 0,
    };
    let mut pool = BufferPool::new();
    // 1024 bytes ≈ 8 signalfd_siginfo entries; matches the size
    // submit_sig_next in aio.rs allocates.
    let buf_handle = pool.alloc(1024);

    if super::submit_uring_sig_next(
        &mut ring,
        SubmissionId::from_raw(1),
        fd,
        &mut pool,
        buf_handle,
    )
    .is_err()
    {
        return 33;
    }

    if unsafe { libc::kill(libc::getpid(), libc::SIGUSR1) } != 0 {
        return 34;
    }

    // Bounded wait via io_uring's own timespec — if no CQE
    // arrives within 5 s the helper is broken (or io_uring on
    // this host doesn't poll signalfd correctly, which would be a
    // real bug to surface).
    let ts = io_uring::types::Timespec::new().sec(5).nsec(0);
    let args = io_uring::types::SubmitArgs::new().timespec(&ts);
    match ring.submitter().submit_with_args(1, &args) {
        Ok(_) => {}
        Err(e) if e.raw_os_error() == Some(libc::ETIME) => return 35,
        Err(_) => return 36,
    }

    let cqe = match ring.completion().next() {
        Some(c) => c,
        None => return 37,
    };
    let n = cqe.result();
    if cqe.user_data() != 1 {
        return 38;
    }
    if n <= 0 {
        return 39;
    }

    let buf = pool.get_mut(buf_handle);
    let events = r.parse_events(&buf[..n as usize]);
    if events.is_empty() {
        return 40;
    }
    if events[0].signum != libc::SIGUSR1 {
        return 41;
    }
    r.close();
    0
}

/// Verify apply_socket_options actually sets SO_SNDBUF on a socket fd.
#[test]
fn test_apply_sndbuf() {
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let opts = SocketOptions {
        sndbuf: Some(1048576),
        ..Default::default()
    };
    apply_socket_options(fd, &opts);

    let mut val: i32 = 0;
    let mut len: libc::socklen_t = std::mem::size_of::<i32>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &mut val as *mut i32 as *mut libc::c_void,
            &mut len,
        )
    };
    unsafe { libc::close(fd) };
    assert_eq!(ret, 0, "getsockopt failed");
    // Linux doubles the value (adds overhead accounting)
    assert!(
        val >= 1048576,
        "SO_SNDBUF should be >= requested: got {}",
        val
    );
}

/// Verify apply_socket_options actually sets SO_RCVBUF.
#[test]
fn test_apply_rcvbuf() {
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let opts = SocketOptions {
        rcvbuf: Some(524288),
        ..Default::default()
    };
    apply_socket_options(fd, &opts);

    let mut val: i32 = 0;
    let mut len: libc::socklen_t = std::mem::size_of::<i32>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            &mut val as *mut i32 as *mut libc::c_void,
            &mut len,
        )
    };
    unsafe { libc::close(fd) };
    assert_eq!(ret, 0, "getsockopt failed");
    assert!(
        val >= 524288,
        "SO_RCVBUF should be >= requested: got {}",
        val
    );
}

/// Verify SO_KEEPALIVE is actually enabled.
#[test]
fn test_apply_keepalive() {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let opts = SocketOptions {
        keepalive: Some(true),
        ..Default::default()
    };
    apply_socket_options(fd, &opts);

    let mut val: i32 = 0;
    let mut len: libc::socklen_t = std::mem::size_of::<i32>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_KEEPALIVE,
            &mut val as *mut i32 as *mut libc::c_void,
            &mut len,
        )
    };
    unsafe { libc::close(fd) };
    assert_eq!(ret, 0, "getsockopt failed");
    assert_eq!(val, 1, "SO_KEEPALIVE should be enabled");
}

/// Verify TCP_NODELAY is actually enabled.
#[test]
fn test_apply_nodelay() {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let opts = SocketOptions {
        nodelay: Some(true),
        ..Default::default()
    };
    apply_socket_options(fd, &opts);

    let mut val: i32 = 0;
    let mut len: libc::socklen_t = std::mem::size_of::<i32>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::IPPROTO_TCP,
            libc::TCP_NODELAY,
            &mut val as *mut i32 as *mut libc::c_void,
            &mut len,
        )
    };
    unsafe { libc::close(fd) };
    assert_eq!(ret, 0, "getsockopt failed");
    assert_eq!(val, 1, "TCP_NODELAY should be enabled");
}

/// TCP_NODELAY on a Unix socket doesn't panic (silently ignored).
#[test]
fn test_nodelay_on_unix_is_harmless() {
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let opts = SocketOptions {
        nodelay: Some(true),
        ..Default::default()
    };
    apply_socket_options(fd, &opts);
    unsafe { libc::close(fd) };
}

/// Default SocketOptions is a no-op.
#[test]
fn test_default_is_noop() {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let mut before: i32 = 0;
    let mut len: libc::socklen_t = std::mem::size_of::<i32>() as libc::socklen_t;
    unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &mut before as *mut i32 as *mut libc::c_void,
            &mut len,
        );
    }

    apply_socket_options(fd, &SocketOptions::default());

    let mut after: i32 = 0;
    unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &mut after as *mut i32 as *mut libc::c_void,
            &mut len,
        );
    }
    unsafe { libc::close(fd) };
    assert_eq!(before, after, "default options should not change SO_SNDBUF");
}

/// All four options can be set together without conflict.
#[test]
fn test_all_combined() {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "socket() failed");

    let opts = SocketOptions {
        sndbuf: Some(2097152),
        rcvbuf: Some(1048576),
        nodelay: Some(true),
        keepalive: Some(true),
    };
    apply_socket_options(fd, &opts);

    let read_opt = |level: i32, optname: i32| -> i32 {
        let mut val: i32 = 0;
        let mut len: libc::socklen_t = std::mem::size_of::<i32>() as libc::socklen_t;
        unsafe {
            libc::getsockopt(
                fd,
                level,
                optname,
                &mut val as *mut i32 as *mut libc::c_void,
                &mut len,
            );
        }
        val
    };

    assert!(read_opt(libc::SOL_SOCKET, libc::SO_SNDBUF) >= 2097152);
    assert!(read_opt(libc::SOL_SOCKET, libc::SO_RCVBUF) >= 1048576);
    assert_eq!(read_opt(libc::IPPROTO_TCP, libc::TCP_NODELAY), 1);
    assert_eq!(read_opt(libc::SOL_SOCKET, libc::SO_KEEPALIVE), 1);
    unsafe { libc::close(fd) };
}
