//! io_uring submission and wait methods for async I/O.

use crate::io::aio::{EVENTFD_USER_DATA, TIMEOUT_USER_DATA_TAG};
use crate::io::completion::process_raw_completion;
use crate::io::pending::{PendingOp, PendingTable, Taken};
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

mod stream;
pub(crate) use stream::*;

mod submit;
pub(crate) use submit::submit_linked;

/// Upper bound on a single kernel read, in bytes. A `read-exact` whose
/// buffer is larger (e.g. a text read of many graphemes, sized at
/// 4 bytes/grapheme) is filled by several page-sized reads plus the
/// resubmit loop rather than one oversized syscall. 64 KiB matches the
/// default Linux loopback recv buffer, so a single read rarely returns
/// less anyway.
const MAX_READ_CHUNK: usize = 64 * 1024;

/// Upper bound on a single kernel write, in bytes. io_uring carries an SQE's
/// length as a `u32`, so a payload past that boundary must be split; the
/// short-write resubmit loop in `drain_cqes` already walks a payload the fd
/// accepts piecewise, and a payload too large for one SQE is the same walk
/// with a bigger first step. 1 GiB keeps every realistic write to one syscall
/// while staying far below the `u32` limit.
const MAX_WRITE_CHUNK: usize = 1024 * 1024 * 1024;

#[cfg(test)]
mod tests;
