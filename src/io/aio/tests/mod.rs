use super::*;
use crate::io::request::{IoOp, IoRequest};
use crate::port::{Direction, Encoding, Port, PortKind};
use crate::value::error_val_in;
use crate::value::heap::TableKey;
use crate::value::sorted_struct_get;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn write_temp_file(content: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = format!("/tmp/elle-test-async-{}-{}", std::process::id(), n);
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
