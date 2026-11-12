//! audited: 2026-11-11
//! Where a read's bytes land: the remainder it borrows, and the buffer a pool
//! worker reads into.
//!
//! docs/impl/io-bytes.md

use crate::io::pending::PendingOp;
use crate::io::request::writeable_buffer_ptr;
use crate::io::types::{fd_state_mut, FdState, PortKey};
use crate::port::Port;
use crate::value::Value;
use std::collections::HashMap;
use std::os::unix::io::RawFd;

/// How much room a `read-all` keeps in its accumulation for the next read,
/// and how much it grows by when the room runs out.
pub(crate) const READ_ALL_CHUNK: usize = 64 * 1024;

/// Hand the port's remainder to a read it cannot answer alone: copy it into the
/// front of the caller's buffer, and move it out of the port into the
/// operation. Answers the bytes moved, which the operation gives back if it
/// ends without answering ([`give_back`]).
///
/// A remainder the buffer cannot hold with room to spare stays with the port,
/// and the completion joins it outside the buffer. So does an empty one.
///
/// # Safety
///
/// `buffer` is the read's own pre-allocated `LBytes`, and the fiber that owns
/// it is parked.
pub(crate) unsafe fn lend_remainder(state: &mut FdState, buffer: &Value) -> Vec<u8> {
    let (dst, cap) = writeable_buffer_ptr(buffer);
    let held = state.buffer.len();
    if held == 0 || held >= cap {
        return Vec::new();
    }
    std::ptr::copy_nonoverlapping(state.buffer.as_ptr(), dst, held);
    std::mem::take(&mut state.buffer)
}

/// Give a read's borrowed remainder back to the front of its port's remainder,
/// for a read that ends without answering. The next read on the port then
/// finds those bytes first, which is where they are in the stream.
///
/// A closed port's remainder went with its close, and these bytes go the same
/// way: a new port may already hold the descriptor number.
pub(crate) fn give_back(
    fd_states: &mut HashMap<PortKey, FdState>,
    key: &PortKey,
    port: &Value,
    lent: Vec<u8>,
) {
    if lent.is_empty() || port.as_external::<Port>().is_none_or(|p| p.is_closed()) {
        return;
    }
    let state = fd_state_mut(fd_states, key);
    if state.buffer.is_empty() {
        state.buffer = lent;
    } else {
        state.buffer.splice(0..0, lent);
    }
}

/// [`give_back`] whatever remainder `op` borrowed. Reads the port the entry
/// names, so only an entry still holding its operands may call this.
pub(crate) fn give_back_lent(op: &mut PendingOp, fd_states: &mut HashMap<PortKey, FdState>) {
    if let PendingOp::Port {
        port_key,
        port,
        lent,
        ..
    } = op
    {
        give_back(fd_states, port_key, port, std::mem::take(lent));
    }
}

/// How many bytes lie between a regular file's position and its end, or
/// `None` for a descriptor with no end to measure: a pipe, a socket, a tty.
pub(crate) fn rest_of_file(fd: RawFd) -> Option<usize> {
    // SAFETY: `fstat` fills the struct it is handed and reads nothing else.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut st) } != 0 || (st.st_mode & libc::S_IFMT) != libc::S_IFREG {
        return None;
    }
    let at = unsafe { libc::lseek(fd, 0, libc::SEEK_CUR) };
    if at < 0 {
        return None;
    }
    Some((st.st_size as i64).saturating_sub(at as i64).max(0) as usize)
}

/// Where a pool worker puts what it reads: the caller's buffer by address, or
/// a buffer of the worker's own the same size.
pub(crate) struct Landing {
    base: *mut u8,
    cap: usize,
    /// Bytes already in the buffer when the worker started: a borrowed
    /// remainder. They count toward the read, and the worker does not report
    /// them back.
    start: usize,
    filled: usize,
    /// The worker's own buffer, when the read cannot address the caller's.
    own: Option<Vec<u8>>,
}

// SAFETY: a landing in the caller's buffer is written by one worker while the
// entry that filed it holds the buffer's region, and a backend's teardown waits
// for the worker; see `Landing::in_buffer`.
unsafe impl Send for Landing {}

impl Landing {
    /// The caller's buffer, behind the `start` bytes already in it.
    ///
    /// # Safety
    ///
    /// `buffer` is the read's pre-allocated `LBytes`, its fiber is parked, and
    /// the pending entry holds its region until the completion is reaped.
    pub(crate) unsafe fn in_buffer(buffer: &Value, start: usize) -> Landing {
        let (base, cap) = writeable_buffer_ptr(buffer);
        Landing {
            base,
            cap,
            start,
            filled: start,
            own: None,
        }
    }

    /// A buffer of the worker's own, `cap` bytes long.
    pub(crate) fn owned(cap: usize) -> Landing {
        let mut own = vec![0u8; cap];
        Landing {
            base: own.as_mut_ptr(),
            cap,
            start: 0,
            filled: 0,
            own: Some(own),
        }
    }

    /// The bytes in the buffer so far, a borrowed remainder included.
    pub(crate) fn filled(&self) -> &[u8] {
        // SAFETY: `base` holds `cap` bytes and the first `filled` are written.
        unsafe { std::slice::from_raw_parts(self.base, self.filled) }
    }

    /// The address and length of the room left in the buffer.
    pub(crate) fn spare(&mut self) -> (*mut u8, usize) {
        // SAFETY: `filled <= cap`, so the address stays inside the buffer.
        (
            unsafe { self.base.add(self.filled) },
            self.cap - self.filled,
        )
    }

    /// Count `n` more bytes as written into the room [`spare`](Self::spare)
    /// named.
    pub(crate) fn advance(&mut self, n: usize) {
        debug_assert!(self.filled + n <= self.cap, "a read ran past its buffer");
        self.filled += n;
    }

    pub(crate) fn is_full(&self) -> bool {
        self.filled == self.cap
    }

    /// The completion this read reports. Its result code is the bytes it added
    /// to the caller's buffer. Its data is the bytes that went anywhere else:
    /// `extra`, past a full buffer, or everything, when the worker read into a
    /// buffer of its own.
    pub(crate) fn finish(self, extra: Vec<u8>) -> (i32, Vec<u8>) {
        match self.own {
            None => ((self.filled - self.start) as i32, extra),
            Some(mut own) => {
                own.truncate(self.filled);
                own.extend_from_slice(&extra);
                (0, own)
            }
        }
    }
}
