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

mod backend;
mod bridge;
mod fileops;
mod net;
mod submit;
