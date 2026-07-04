//! io_uring submission and wait methods for async I/O.

use crate::io::aio::{EVENTFD_USER_DATA, TIMEOUT_USER_DATA_TAG};
use crate::io::completion::process_raw_completion;
use crate::io::pending::PendingOp;
use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::{apply_socket_options, ConnectAddr, IoOp};
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

/// Upper bound on a single kernel read, in bytes. A `read-exact` whose
/// buffer is larger (e.g. a text read of many graphemes, sized at
/// 4 bytes/grapheme) is filled by several page-sized reads plus the
/// resubmit loop rather than one oversized syscall. 64 KiB matches the
/// default Linux loopback recv buffer, so a single read rarely returns
/// less anyway.
const MAX_READ_CHUNK: usize = 64 * 1024;

#[cfg(test)]
mod tests;
