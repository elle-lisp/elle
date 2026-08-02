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
    let r = match SignalReceiver::new(
        vec![libc::SIGUSR1],
        std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
    ) {
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

/// The full-write invariant at the drain loop (src/io/AGENTS.md § Full-Write
/// Invariant). One write(2) transfers only what fits in the fd's send buffer,
/// so `drain_cqes` must resubmit the unwritten tail — from the pooled buffer
/// the submission copied the payload into — until nothing is left, and report
/// the total across every resubmission rather than the last CQE's count.
///
/// A 4 KiB send buffer cannot take a 512 KiB payload in one syscall, so a
/// backend that completes on the first CQE fails both assertions: the reported
/// count is short, and the peer's tally is short. Driving `drain_cqes` directly
/// keeps the coverage at the resubmission mechanism; the end-to-end contract is
/// `tests/elle/port-shortwrite.lisp`.
#[test]
fn short_write_resubmits_until_the_payload_is_gone() {
    use crate::io::pending::PendingOp;
    use crate::io::types::{FdState, PortKey};
    use crate::io::{Completion, SubmissionId};
    use crate::value::Value;
    use std::collections::{HashMap, VecDeque};
    use std::time::{Duration, Instant};

    const PAYLOAD: usize = 512 * 1024;
    const SNDBUF: libc::c_int = 4096;

    let mut ring = match io_uring::IoUring::new(8) {
        Ok(ring) => ring,
        // No io_uring on this host kernel — nothing to cover here. The
        // thread-pool half of the invariant is pinned by the `--no-uring`
        // run of port-shortwrite.lisp.
        Err(_) => return,
    };

    let mut fds: [libc::c_int; 2] = [0; 2];
    assert_eq!(
        unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) },
        0,
        "socketpair failed: {}",
        std::io::Error::last_os_error()
    );
    let (write_fd, read_fd) = (fds[0], fds[1]);
    unsafe {
        libc::setsockopt(
            write_fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &SNDBUF as *const libc::c_int as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
        // Non-blocking, so the kernel returns a short count instead of
        // sleeping until the whole payload fits — the case under test.
        libc::fcntl(write_fd, libc::F_SETFL, libc::O_NONBLOCK);
    }

    // The peer drains continuously, so a write that loops can always finish.
    let reader = std::thread::spawn(move || {
        let mut received = 0usize;
        let mut buf = vec![0u8; 64 * 1024];
        while received < PAYLOAD {
            let n =
                unsafe { libc::read(read_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n <= 0 {
                break;
            }
            received += n as usize;
        }
        unsafe { libc::close(read_fd) };
        received
    });

    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let data = crate::primitives::ctx::Alloc::new(&mut heap).bytes(vec![b'x'; PAYLOAD]);

    let mut pool = BufferPool::new();
    let buf_handle = pool.alloc(4096);
    let id = SubmissionId::from_raw(1);
    super::submit_uring_stream(
        &mut ring,
        id,
        write_fd,
        &PortOp::Write { data },
        None,
        &mut pool,
        Some(buf_handle),
        0,
    )
    .expect("submit_uring_stream");

    let mut pending: HashMap<SubmissionId, PendingOp> = HashMap::new();
    pending.insert(
        id,
        PendingOp::Port {
            op: PortOp::Write { data },
            port_key: PortKey::Fd(write_fd),
            port: Value::NIL,
            buffer_handle: Some(buf_handle),
            listener_kind: None,
            filled: 0,
            timeout: None,
        },
    );
    let mut fd_states: HashMap<PortKey, FdState> = HashMap::new();
    let mut completions: VecDeque<Completion> = VecDeque::new();
    let mut eventfd_fired = false;

    // Bounded so a regression that stops resubmitting fails here instead of
    // wedging the test run.
    let deadline = Instant::now() + Duration::from_secs(20);
    while completions.is_empty() {
        assert!(
            Instant::now() < deadline,
            "write never completed: {} of {} bytes reported after 20s",
            pending.get(&id).map(|p| p.filled()).unwrap_or(0),
            PAYLOAD
        );
        let ts = io_uring::types::Timespec::new().sec(1).nsec(0);
        let args = io_uring::types::SubmitArgs::new().timespec(&ts);
        match ring.submitter().submit_with_args(1, &args) {
            Ok(_) => {}
            Err(e) if e.raw_os_error() == Some(libc::ETIME) => {}
            Err(e) if e.raw_os_error() == Some(libc::EINTR) => {}
            Err(e) => panic!("submit_with_args: {}", e),
        }
        super::drain_cqes(
            &mut ring,
            &mut pending,
            &mut pool,
            &mut fd_states,
            &mut completions,
            &mut heap as *mut crate::value::fiberheap::FiberHeap,
            &mut eventfd_fired,
        );
    }

    let completion = completions.pop_front().expect("one completion");
    let value = completion.result.expect("write succeeded");
    assert_eq!(
        value.as_int(),
        Some(PAYLOAD as i64),
        "port/write must report every byte it wrote, not the last chunk"
    );

    unsafe { libc::close(write_fd) };
    let received = reader.join().expect("reader thread");
    assert_eq!(
        received, PAYLOAD,
        "the peer must receive every byte, not just what the first syscall took"
    );
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

/// A submission carrying a deadline must come back as `-ECANCELED` once the
/// deadline passes, and must still yield exactly one completion for the
/// operation.
///
/// Both halves are the linked-timeout protocol `submit_linked` implements: the
/// `IO_LINK` flag is what lets the timer cancel the operation, and the tag on
/// the timer's `user_data` is what keeps its own completion from being
/// mistaken for the operation's. Drop the flag and the poll waits forever;
/// drop the tag and the caller sees two completions for one request.
#[test]
fn a_linked_timeout_cancels_its_operation_and_reports_once() {
    use crate::io::uring::submit_linked;
    use crate::io::SubmissionId;
    use std::time::Duration;

    let mut ring = match io_uring::IoUring::new(8) {
        Ok(ring) => ring,
        // No io_uring on this host kernel — the thread-pool backend covers
        // the same timeout behavior through port-read-timeout.lisp.
        Err(_) => return,
    };

    // A pipe nobody ever writes to: the poll can only end by timing out.
    let mut fds: [libc::c_int; 2] = [0; 2];
    assert_eq!(
        unsafe { libc::pipe(fds.as_mut_ptr()) },
        0,
        "pipe failed: {}",
        std::io::Error::last_os_error()
    );
    let (read_fd, write_fd) = (fds[0], fds[1]);

    let id = SubmissionId::from_raw(0x5eed);
    let poll = io_uring::opcode::PollAdd::new(io_uring::types::Fd(read_fd), libc::POLLIN as u32)
        .build()
        .user_data(id.as_u64());

    // SAFETY: the poll SQE points at no caller-owned memory beyond the fd,
    // which outlives the submission.
    unsafe { submit_linked(&mut ring, id, poll, Some(Duration::from_millis(50))) }
        .expect("submission failed");

    ring.submit_and_wait(1).expect("wait failed");

    let mut for_the_operation = 0;
    let mut result = None;
    for cqe in ring.completion() {
        if cqe.user_data() == id.as_u64() {
            for_the_operation += 1;
            result = Some(cqe.result());
        }
    }

    unsafe {
        libc::close(read_fd);
        libc::close(write_fd);
    }

    assert_eq!(
        for_the_operation, 1,
        "the operation must produce exactly one completion; the timer's own \
         CQE carries the tag and is not it"
    );
    assert_eq!(
        result,
        Some(-libc::ECANCELED),
        "an expired deadline must cancel the linked operation"
    );
}
