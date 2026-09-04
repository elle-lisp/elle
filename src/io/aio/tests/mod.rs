use super::*;
use crate::io::request::{IoOp, IoRequest};
use crate::port::{Direction, Encoding, Port, PortKind};
use crate::value::error_val_in;
use crate::value::heap::TableKey;
use crate::value::sorted_struct_get;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A TCP listener socket for the accept tests.
///
/// `SOCK_NONBLOCK` as a `socket(2)` flag is a Linux extension, and `libc` does
/// not define it anywhere else, so naming it unconditionally stopped the whole
/// test target from compiling off Linux.
///
/// The flag is kept on Linux, where these tests already run, so their behaviour
/// there is unchanged. Elsewhere the listener is left blocking, because the
/// thread-pool backend calls `accept(2)` directly: on a non-blocking listener
/// that returns `EAGAIN` before any peer connects, and the backend reports the
/// error rather than waiting for a connection.
///
/// # Safety
/// The caller owns the returned descriptor and must close it.
pub(super) unsafe fn tcp_listener_socket() -> libc::c_int {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let ty = libc::SOCK_STREAM | libc::SOCK_NONBLOCK;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let ty = libc::SOCK_STREAM;
    libc::socket(libc::AF_INET, ty, 0)
}

/// Unique scratch path under the platform temp root (honors TMPDIR — never
/// hardcoded /tmp). Callers create the file and remove it before returning.
fn temp_path(tag: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join(format!("elle-test-{}-{}-{}", tag, std::process::id(), n))
        .to_str()
        .unwrap()
        .to_string()
}

fn write_temp_file(content: &str) -> String {
    let path = temp_path("async");
    std::fs::write(&path, content).unwrap();
    path
}

fn open_read_port(path: &str) -> Value {
    let h = crate::primitives::ctx::TestHeap::new();
    let file = std::fs::File::open(path).unwrap();
    let fd: std::os::unix::io::OwnedFd = file.into();
    h.ctx().external(
        "port",
        Port::new_file(fd, Direction::Read, Encoding::Text, path.to_string()),
    )
}

fn open_write_port(path: &str) -> Value {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .unwrap();
    let h = crate::primitives::ctx::TestHeap::new();
    let fd: std::os::unix::io::OwnedFd = file.into();
    h.ctx().external(
        "port",
        Port::new_file(fd, Direction::Write, Encoding::Text, path.to_string()),
    )
}

fn open_rw_port(path: &str) -> Value {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .unwrap();
    let h = crate::primitives::ctx::TestHeap::new();
    let fd: std::os::unix::io::OwnedFd = file.into();
    h.ctx().external(
        "port",
        Port::new_file(fd, Direction::ReadWrite, Encoding::Text, path.to_string()),
    )
}

/// Give a submission time to reach a worker, and then a moment more, so a test
/// that wants the operation already parked in its syscall gets that order.
///
/// Reports whether a worker came out. `workers()` counts a submission from the
/// moment its thread spawns, so a true here says the operation is out rather
/// than that the worker has reached its syscall — the pause is what makes the
/// interesting order the likely one. Always false on the ring, which runs its
/// operations in the kernel and has no worker to wait for.
fn wait_for_worker(backend: &AsyncBackend) -> bool {
    for _ in 0..200 {
        if backend.workers() > 0 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

/// Cancel a pool operation and assert it RETIRES: its worker comes back and
/// its `pending` entry goes, within a bounded number of waits.
///
/// The pool's `wait` blocks on the hub channel only `if hub.in_flight() > 0`.
/// The io_uring arm has no such guard — it waits on the ring unconditionally,
/// and says so: "a genuinely lost wakeup hangs rather than being downgraded to
/// a bounded stall". So the pool has a state the ring does not: an operation
/// still in `pending` while no worker is out for it. `wait` then returns
/// nothing without blocking, and whoever is parked on that operation is never
/// woken again.
///
/// No completion is delivered for a cancelled operation, and that is the design
/// rather than an omission — `cook_raw` discards a cancelled op before cooking
/// it, because the fiber that requested it is already gone and cooking a read
/// would write the worker's bytes into a freed heap. What must not happen is
/// the entry outliving the worker.
fn assert_cancel_retires(backend: &AsyncBackend, id: SubmissionId, what: &str) {
    assert!(
        wait_for_worker(backend),
        "the pool never took the {} out to a worker",
        what
    );

    backend.cancel(id).unwrap();

    // A bounded number of waits, because the property under test is exactly
    // that this terminates: an operation left in `pending` with no worker out
    // would leave `wait` returning nothing for as long as it is asked.
    for _ in 0..40 {
        let _ = backend.wait(50).unwrap();
        if !backend.has_pending() && backend.workers() == 0 {
            break;
        }
    }

    assert!(
        !backend.has_pending(),
        "the cancelled {} is still pending with {} worker(s) out — an \
         operation that keeps its `pending` entry after its worker is gone \
         can never be reaped, because the pool's `wait` blocks only while \
         `in_flight() > 0`",
        what,
        backend.workers(),
    );
    assert_eq!(
        backend.workers(),
        0,
        "the cancelled {} never gave its worker back",
        what,
    );
}

mod backend;
mod bridge;
mod fileops;
mod net;
mod park;
mod process;
mod submit;
