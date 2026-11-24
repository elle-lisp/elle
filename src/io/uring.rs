//! audited: 2026-11-23
//! io_uring SQE submission and CQE processing for async I/O (Linux only).
//!
//! docs/impl/io/platforms.md
//! docs/impl/io/bytes.md
//! docs/impl/io/write.md

use crate::io::aio::{EVENTFD_USER_DATA, TIMEOUT_USER_DATA_TAG};
use crate::io::completion::process_raw_completion;
use crate::io::pending::{PendingOp, PendingTable, TakenOp};
use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::{apply_socket_options, ConnectAddr, PortOp};
use crate::io::types::{FdState, PortKey};
use crate::io::{Completion, SubmissionId};
use crate::port::{Port, PortKind};
use std::collections::{HashMap, VecDeque};
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::time::Duration;

mod drain;
pub(crate) use drain::*;

mod file;
pub(crate) use file::submit_uring_file;

mod memcheck;

mod ops;
pub(crate) use ops::*;

mod open;
pub(crate) use open::submit_uring_open;

mod resubmit;

mod ring;
pub(crate) use ring::Ring;

mod stream;
pub(crate) use stream::*;

mod write;
pub(crate) use write::{keep_copy, write_needs_buffer};

mod submit;
pub(crate) use submit::{submit_linked, LinkTimer};

mod wait;
pub(crate) use wait::*;

/// Upper bound on a single kernel read into a caller's buffer, in bytes. A
/// text read of many clusters has a buffer four bytes per cluster wide. A
/// `read-exact` whose buffer is larger is filled by several reads through the
/// resubmit loop rather than one oversized syscall. 64 KiB matches
/// the default Linux loopback recv buffer, so a single read rarely returns less
/// anyway.
const MAX_READ_CHUNK: usize = 64 * 1024;

/// Upper bound on the bytes one SQE moves, in bytes. io_uring carries an SQE's
/// length as a `u32`, so a transfer past that boundary must be split. A
/// write's resubmit loop already walks a payload the fd accepts piecewise, and
/// a `read-all` reads into whatever room its accumulation has. 1 GiB keeps
/// every realistic transfer to one syscall while staying far below the `u32`
/// limit.
const MAX_SQE_BYTES: usize = 1024 * 1024 * 1024;

#[cfg(test)]
mod tests;
