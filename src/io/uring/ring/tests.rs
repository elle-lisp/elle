// audited: 2026-11-11
//! The ring's wait: the flags it enters the kernel with, and that a backend's
//! wait parks outside iowait wherever the kernel allows.
//!
//! docs/impl/io/platforms.md

use super::*;
use crate::io::aio::AsyncBackend;
use crate::io::request::{IoOp, IoRequest};
use crate::io::Completion;
use crate::value::Value;
use std::sync::mpsc;
use std::time::Instant;

/// `IORING_FEAT_NO_IOWAIT`, which the crate's `Parameters` does not read out.
const FEAT_NO_IOWAIT: u32 = 1 << 17;

/// Whether this kernel reports `IORING_FEAT_NO_IOWAIT` from `io_uring_setup(2)`,
/// or `None` when it has no io_uring at all.
///
/// Asked of the kernel directly, so the answer is independent of whatever
/// `Ring::open` concluded: `io_uring_params` is 120 bytes and `features` is
/// its sixth `u32`.
fn kernel_reports_no_iowait() -> Option<bool> {
    let mut params = [0u32; 30];
    let fd = unsafe { libc::syscall(libc::SYS_io_uring_setup, 1u32, params.as_mut_ptr()) };
    if fd < 0 {
        return None;
    }
    unsafe { libc::close(fd as libc::c_int) };
    Some(params[5] & FEAT_NO_IOWAIT != 0)
}

/// The flags a wait enters with, with and without the kernel's leave to stay
/// out of iowait. The counter-factual is a wait that enters with the crate's
/// own flags, which carry no `NO_IOWAIT` on any kernel.
#[test]
fn wait_flags_carry_no_iowait_only_where_the_kernel_takes_it() {
    assert_eq!(
        wait_flags(true, false),
        EnterFlags::GETEVENTS | EnterFlags::NO_IOWAIT
    );
    assert_eq!(
        wait_flags(true, true),
        EnterFlags::GETEVENTS | EnterFlags::EXT_ARG | EnterFlags::NO_IOWAIT
    );
    assert_eq!(wait_flags(false, false), EnterFlags::GETEVENTS);
    assert_eq!(
        wait_flags(false, true),
        EnterFlags::GETEVENTS | EnterFlags::EXT_ARG
    );
}

/// `Ring::open` asks the kernel, and its answer matches the feature bit the
/// kernel reports. A probe that answered false everywhere would leave every
/// wait in iowait with nothing else here to say so: the wait tests below
/// return early on a kernel that reports no such feature.
#[test]
fn open_learns_what_the_kernels_feature_bit_says() {
    let Some(reported) = kernel_reports_no_iowait() else {
        return;
    };
    let ring = Ring::open(8).expect("a ring");
    assert_eq!(
        ring.no_iowait(),
        reported,
        "the ring must learn whether the kernel takes IORING_ENTER_NO_IOWAIT"
    );
}

/// The flags of the `io_uring_enter` a backend's `wait(timeout)` parks in,
/// or `None` on the pool platform, which has no ring to park on.
///
/// The waiter runs on a thread of its own, parked on a 600 ms timer. This
/// thread reads the parked call's arguments from `/proc/self/task/<tid>/syscall`,
/// which a sibling thread may always read; the fourth argument is the flags.
/// Nothing system-wide is read, because a box with other processes in iowait
/// would make that reading say nothing about this one.
fn flags_of_a_parked_wait(timeout: Option<Duration>) -> Option<u64> {
    let (tx, rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        crate::value::arena::with_test_region(|| {
            let backend = AsyncBackend::new().expect("a backend");
            if !backend.is_uring() {
                tx.send(None).expect("the test is listening");
                return 0;
            }
            let req = IoRequest::unbounded(
                IoOp::Sleep {
                    duration: Duration::from_millis(600),
                },
                Value::NIL,
            );
            backend
                .submit(&req, crate::io::pending::Submitter::for_test())
                .expect("a sleep");
            tx.send(Some(unsafe { libc::gettid() }))
                .expect("the test is listening");
            let completions = backend.wait(timeout).expect("the wait");
            let answered = completions.len();
            Completion::discard_all(completions);
            answered
        })
    });
    let Some(tid) = rx.recv().expect("the waiter reports its thread") else {
        waiter.join().expect("the waiter");
        return None;
    };

    let path = format!("/proc/self/task/{tid}/syscall");
    let enter = libc::SYS_io_uring_enter.to_string();
    let deadline = Instant::now() + Duration::from_secs(5);
    let flags = loop {
        assert!(
            Instant::now() < deadline,
            "the waiter never parked in io_uring_enter"
        );
        let line = std::fs::read_to_string(&path).expect("a sibling thread's syscall");
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.first() == Some(&enter.as_str()) {
            let hex = fields[4].trim_start_matches("0x");
            break u64::from_str_radix(hex, 16).expect("the flags argument in hex");
        }
        std::thread::sleep(Duration::from_millis(2));
    };

    assert_eq!(
        waiter.join().expect("the waiter"),
        1,
        "the wait must still answer the timer's completion"
    );
    Some(flags)
}

/// A wait with no bound parks with `IORING_ENTER_NO_IOWAIT` exactly when the
/// kernel reports the feature. The counter-factual is the crate's
/// `submit_and_wait`, which parks with `GETEVENTS` alone, in iowait.
#[test]
fn an_unbounded_wait_parks_outside_iowait_where_the_kernel_allows() {
    let Some(reported) = kernel_reports_no_iowait() else {
        return;
    };
    let Some(flags) = flags_of_a_parked_wait(None) else {
        return;
    };
    assert_ne!(flags & EnterFlags::GETEVENTS.bits() as u64, 0);
    assert_eq!(
        flags & EnterFlags::NO_IOWAIT.bits() as u64 != 0,
        reported,
        "an unbounded wait parked with flags {flags:#x}"
    );
}

/// A bounded wait carries its timespec through `EXT_ARG`, and parks with
/// `IORING_ENTER_NO_IOWAIT` exactly when the kernel reports the feature. The
/// counter-factual is the crate's `submit_with_args`, which parks with
/// `GETEVENTS | EXT_ARG`, in iowait.
#[test]
fn a_bounded_wait_parks_outside_iowait_where_the_kernel_allows() {
    let Some(reported) = kernel_reports_no_iowait() else {
        return;
    };
    let Some(flags) = flags_of_a_parked_wait(Some(Duration::from_secs(5))) else {
        return;
    };
    assert_ne!(flags & EnterFlags::GETEVENTS.bits() as u64, 0);
    assert_ne!(flags & EnterFlags::EXT_ARG.bits() as u64, 0);
    assert_eq!(
        flags & EnterFlags::NO_IOWAIT.bits() as u64 != 0,
        reported,
        "a bounded wait parked with flags {flags:#x}"
    );
}

/// A bounded wait with nothing to report ends at its bound and answers
/// nothing. The bridge poll alone is in flight, so only the timespec can end
/// the wait: a wait that passed it wrongly would run to the 5 s patience and
/// fail here rather than hang the run.
#[test]
fn a_bounded_wait_with_nothing_to_report_ends_at_its_bound() {
    let backend = AsyncBackend::new().expect("a backend");
    if !backend.is_uring() {
        return;
    }
    let started = Instant::now();
    let completions = backend
        .wait(Some(Duration::from_millis(100)))
        .expect("the wait");
    let elapsed = started.elapsed();
    assert!(completions.is_empty(), "nothing was in flight to answer");
    assert!(
        elapsed >= Duration::from_millis(100),
        "the wait returned after {elapsed:?}, before its bound"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "the wait returned after {elapsed:?}, long past its bound"
    );
}
