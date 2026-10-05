// audited: 2026-10-04
//! The stdin thread ends on shutdown, idle or blocked inside a read of descriptor 0.
//!
//! src/io/AGENTS.md

use super::super::*;
use crate::io::isolate::{run, Outcome};

/// Shutdown signal must wake the stdin thread when it is sitting in
/// `request_rx.recv()` waiting for the next request (no read in
/// flight). The thread should exit cleanly within a short
/// timeout. Counter-factual: without the self-pipe + shutdown
/// wiring, the thread sits in `recv()` until the channel sender
/// drops, which doesn't happen until process exit — the test
/// helper's `recv_timeout` below would fire.
///
/// This test does NOT need to touch fd 0. The thread is idle
/// (never submits a request) so the read syscall is never reached.
#[test]
fn stdin_thread_shutdown_while_idle_joins() {
    use std::time::{Duration, Instant};
    let (tx, _rx) = crossbeam_channel::unbounded::<RawCompletion>();
    let st = StdinThread::new(tx, None);
    st.shutdown();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !st.is_finished() {
        if Instant::now() >= deadline {
            panic!("stdin thread did not exit within 2s of shutdown");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Shutdown signal must wake the stdin thread when it is parked
/// inside `libc::read(0, …)` waiting for input. The body runs through
/// `run`, in a process of its own, so it can `dup2` a pipe onto fd 0
/// without disturbing the test process (peer tests share fd 0). The
/// body holds the write end open so the read truly blocks (no EOF).
/// After a 100 ms settle, the body calls `shutdown()` and expects an
/// error completion within 2 s.
///
/// Counter-factual: the legacy
/// `std::io::stdin().lock().read_line(…)` auto-retries on EINTR
/// and has no shutdown path; a signal or pipe-write cannot wake
/// it. The body would hang past `run`'s 5 s deadline.
#[test]
fn stdin_thread_shutdown_cancels_inflight_read() {
    let report = run(std::time::Duration::from_secs(5), stdin_close_child_logic);
    assert_eq!(
        report.end,
        Outcome::Exited(0),
        "see codes 51-58 in stdin_close_child_logic"
    );
}

fn stdin_close_child_logic() -> i32 {
    use std::time::Duration;
    // Replace fd 0 with the read end of a pipe and hold the write
    // end so the read never sees EOF. The stdin thread will block
    // inside libc::read(0, …) until our shutdown signal wakes it.
    let mut pipe_fds: [libc::c_int; 2] = [0; 2];
    if unsafe { libc::pipe(pipe_fds.as_mut_ptr()) } != 0 {
        return 51;
    }
    if unsafe { libc::dup2(pipe_fds[0], 0) } < 0 {
        return 52;
    }
    unsafe { libc::close(pipe_fds[0]) };
    let _write_end = pipe_fds[1]; // kept open until process exit

    let (tx, rx) = crossbeam_channel::unbounded::<RawCompletion>();
    let st = StdinThread::new(tx, None);
    if st
        .submit(SubmissionId::from_raw(1), StdinOpKind::ReadLine)
        .is_err()
    {
        return 53;
    }
    // Settle: give the thread time to enter the read.
    std::thread::sleep(Duration::from_millis(100));

    st.shutdown();

    // The worker reports through the shared hub channel as a
    // RawCompletion::Stdin; here that channel is the test-local `rx`.
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(RawCompletion::Stdin(c)) => {
            if c.id != 1 {
                return 54;
            }
            match c.result {
                Ok(_) => 55, // expected an error, got Ok
                Err(msg) => {
                    if msg.contains("stdin closed") {
                        0
                    } else {
                        56
                    }
                }
            }
        }
        Ok(RawCompletion::Pool(_)) => 58, // wrong variant — never happens for stdin
        Err(_) => 57,
    }
}
