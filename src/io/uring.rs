//! audited: 2026-09-23
//! io_uring SQE submission and CQE processing for async I/O (Linux only).
//!
//! src/io/AGENTS.md
//! docs/impl/io-bytes.md

use crate::io::aio::{EVENTFD_USER_DATA, TIMEOUT_USER_DATA_TAG};
use crate::io::completion::process_raw_completion;
use crate::io::pending::{PendingOp, PendingTable, Taken, TakenOp};
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

mod ops;
pub(crate) use ops::*;

mod resubmit;

mod stream;
pub(crate) use stream::*;

mod submit;
pub(crate) use submit::submit_linked;

mod wait;
pub(crate) use wait::*;

/// Upper bound on a single kernel read into a caller's buffer, in bytes. A
/// `read-exact` whose buffer is larger (a text read of many clusters, whose
/// buffer is a chunk four bytes per cluster wide) is filled by several reads
/// through the resubmit loop rather than one oversized syscall. 64 KiB matches
/// the default Linux loopback recv buffer, so a single read rarely returns less
/// anyway.
const MAX_READ_CHUNK: usize = 64 * 1024;

/// Upper bound on the bytes one SQE moves, in bytes. io_uring carries an SQE's
/// length as a `u32`, so a transfer past that boundary must be split: a
/// write's resubmit loop already walks a payload the fd accepts piecewise, and
/// a `read-all` reads into whatever room its accumulation has. 1 GiB keeps
/// every realistic transfer to one syscall while staying far below the `u32`
/// limit.
const MAX_SQE_BYTES: usize = 1024 * 1024 * 1024;

#[cfg(test)]
mod tests;
