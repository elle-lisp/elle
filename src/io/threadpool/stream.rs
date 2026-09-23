//! audited: 2026-09-23
//! The byte-stream operations a worker runs: the four reads, the write, and
//! the flush.
//!
//! docs/impl/io-bytes.md
//!
//! Each owns its `OpBound` for the operation's lifetime, so its syscalls are
//! non-blocking and every wait is answerable. The reads land in the room their
//! `Landing` names, which is the caller's own buffer whenever the operation can
//! be stopped, and the write reads its `Payload` where it lies.

use super::*;
use crate::io::landing::{rest_of_file, READ_ALL_CHUNK};

/// What one attempt to read into `dst` came to.
enum Took {
    /// This many bytes arrived.
    Bytes(usize),
    /// The stream ended.
    End,
    /// The operation ends with this completion instead: a stop, a timeout, or
    /// an error.
    Ends(i32),
}

/// Read once into the `len` bytes at `dst`, waiting under `bound` while the
/// descriptor has nothing.
fn take_into(bound: &OpBound, fd: RawFd, dst: *mut u8, len: usize) -> Took {
    loop {
        let ret = unsafe { libc::read(fd, dst as *mut libc::c_void, len) };
        if ret > 0 {
            return Took::Bytes(ret as usize);
        }
        if ret == 0 {
            return Took::End;
        }
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        if errno == libc::EINTR {
            continue;
        }
        if is_would_block(errno) {
            match bound.wait(libc::POLLIN) {
                Wake::Ready => continue,
                Wake::Stopped => return Took::Ends(-libc::ECANCELED),
                // The deadline passed with nothing more arriving. That is the
                // caller's timeout, not the end of the stream: reporting a
                // partial here would read as EOF, which a `read-exact` maps to
                // nil and a `read-line` to a line.
                Wake::TimedOut => return Took::Ends(-libc::ETIMEDOUT),
            }
        }
        return Took::Ends(-errno);
    }
}

/// Read once, into the room left in `landing`.
pub(super) fn read(bound: OpBound, fd: RawFd, mut landing: Landing) -> (i32, Vec<u8>) {
    let (dst, room) = landing.spare();
    match take_into(&bound, fd, dst, room) {
        Took::Bytes(n) => landing.advance(n),
        Took::End => {}
        Took::Ends(code) => return (code, Vec::new()),
    }
    landing.finish(Vec::new())
}

/// Read until the bytes hold `count` units, the stream ends, or an error
/// fires. Units are bytes unless `graphemes`, in which case they are grapheme
/// clusters counted under `gen`.
///
/// `held` is a remainder the port kept, and it counts toward `count` — see
/// `PoolOp::ReadExact`. The landing may start with a lent remainder too, which
/// counts the same way. Only the shortfall is asked of the kernel, so a peer
/// that has sent everything is never waited on for bytes the port already has.
///
/// A byte count fits the buffer by construction. A cluster count may not, and
/// the bytes past a full buffer go to a `Vec` of the worker's own, which the
/// completion joins (docs/impl/io-bytes.md § "When the answer outgrows the
/// buffer").
pub(super) fn read_exact(
    bound: OpBound,
    fd: RawFd,
    mut landing: Landing,
    count: usize,
    graphemes: bool,
    gen: crate::segment::Generation,
    held: &[u8],
) -> (i32, Vec<u8>) {
    let lent = landing.filled().len();
    let mut extra: Vec<u8> = Vec::new();
    loop {
        let have = held.len() + landing.filled().len() + extra.len();
        // What this operation itself took from the descriptor.
        let taken = landing.filled().len() - lent + extra.len();
        let shortfall = if graphemes {
            // Counted over everything this read owns at once: a cluster can
            // straddle any two of the parts, so none can be counted alone.
            let clusters = if held.is_empty() && extra.is_empty() {
                grapheme_count_in_valid_prefix(landing.filled(), gen)
            } else {
                let mut all = held.to_vec();
                all.extend_from_slice(landing.filled());
                all.extend_from_slice(&extra);
                grapheme_count_in_valid_prefix(&all, gen)
            };
            count.saturating_sub(clusters)
        } else {
            count.saturating_sub(have)
        };
        if shortfall == 0 {
            break;
        }
        let took = if landing.is_full() {
            // Past the buffer: one byte per missing cluster is the ASCII best
            // case, and an undershoot loops.
            let at = extra.len();
            extra.resize(at + shortfall, 0);
            let took = take_into(&bound, fd, extra[at..].as_mut_ptr(), shortfall);
            extra.truncate(at + if let Took::Bytes(n) = took { n } else { 0 });
            took
        } else {
            let (dst, room) = landing.spare();
            // A byte count asks for exactly what is missing, so the port holds
            // no remainder it would have to account for afterwards.
            let want = if graphemes { room } else { room.min(shortfall) };
            let took = take_into(&bound, fd, dst, want);
            if let Took::Bytes(n) = took {
                landing.advance(n);
            }
            took
        };
        match took {
            Took::Bytes(_) => {}
            // Short of the count: the completion answers nil.
            Took::End => break,
            // An error after this read took some bytes surfaces what arrived,
            // which the completion also reads as a stream that ended short. A
            // stop or a timeout discards it.
            Took::Ends(code)
                if code != -libc::ECANCELED && code != -libc::ETIMEDOUT && taken > 0 =>
            {
                break
            }
            Took::Ends(code) => return (code, Vec::new()),
        }
    }
    landing.finish(extra)
}

/// Read until a newline arrives, the buffer is full, or the stream ends.
///
/// Each read asks for a page at most, so the bytes past the newline that go
/// back to the port stay few. A full buffer with no newline answers as it is:
/// a piece of a longer line, which the next read goes on from.
pub(super) fn read_line(bound: OpBound, fd: RawFd, mut landing: Landing) -> (i32, Vec<u8>) {
    let lent = landing.filled().len();
    while !landing.is_full() {
        let before = landing.filled().len();
        let (dst, room) = landing.spare();
        match take_into(&bound, fd, dst, room.min(4096)) {
            Took::Bytes(n) => landing.advance(n),
            Took::End => break,
            Took::Ends(code)
                if code != -libc::ECANCELED && code != -libc::ETIMEDOUT && before > lent =>
            {
                break
            }
            Took::Ends(code) => return (code, Vec::new()),
        }
        if landing.filled()[before..].contains(&b'\n') {
            break;
        }
    }
    landing.finish(Vec::new())
}

/// Read until EOF, into an accumulation this worker owns.
///
/// The kernel reads straight into the accumulation's spare room. A regular
/// file's accumulation is sized to the rest of the file before the first read,
/// so it does not grow; anything else grows a chunk at a time.
pub(super) fn read_all(bound: OpBound, fd: RawFd) -> (i32, Vec<u8>) {
    let mut all: Vec<u8> = Vec::with_capacity(rest_of_file(fd).unwrap_or(0) + READ_ALL_CHUNK);
    loop {
        if all.capacity() - all.len() < READ_ALL_CHUNK {
            all.reserve(READ_ALL_CHUNK);
        }
        let room = all.capacity() - all.len();
        // SAFETY: the room past `len` is allocated and this worker's alone.
        let dst = unsafe { all.as_mut_ptr().add(all.len()) };
        match take_into(&bound, fd, dst, room) {
            // SAFETY: the kernel wrote `n` bytes into that room.
            Took::Bytes(n) => unsafe { all.set_len(all.len() + n) },
            Took::End => return (all.len() as i32, all),
            Took::Ends(code)
                if code != -libc::ECANCELED && code != -libc::ETIMEDOUT && !all.is_empty() =>
            {
                return (all.len() as i32, all)
            }
            Took::Ends(code) => return (code, Vec::new()),
        }
    }
}

/// Write every byte of `payload`.
///
/// `port/write` writes every byte before it returns (docs/io.md), so this loops
/// until the payload is gone. One `write(2)` transfers only what fits in the
/// fd's send buffer, which on a socket is routinely a fraction of a large
/// payload.
///
/// The caller's timeout bounds every pass of this loop, not the call: a peer
/// that has stopped reading trips one wait, while one that merely reads slowly
/// keeps making progress and the transfer finishes however long it takes. That
/// mirrors the io_uring path, which re-arms its LinkTimeout on each
/// resubmission.
pub(super) fn write(bound: OpBound, fd: RawFd, payload: Payload) -> (i32, Vec<u8>) {
    let data = payload.bytes();
    let mut total = 0usize;
    loop {
        let ret = unsafe {
            libc::write(
                fd,
                data[total..].as_ptr() as *const libc::c_void,
                data.len() - total,
            )
        };
        if ret > 0 {
            total += ret as usize;
            if total >= data.len() {
                return (total as i32, Vec::new());
            }
            continue;
        }
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        if ret < 0 && errno == libc::EINTR {
            // A signal interrupted the syscall before any byte moved; the
            // payload is unchanged, so retry.
            continue;
        }
        if ret < 0 && is_would_block(errno) {
            match bound.wait(libc::POLLOUT) {
                Wake::Ready => continue,
                Wake::Stopped => return (-libc::ECANCELED, Vec::new()),
                // The wait for room expired. Report it as the caller's
                // timeout, which `complete_port_op` maps to a `:timeout` error
                // rather than a generic I/O one.
                Wake::TimedOut => return (-libc::ETIMEDOUT, Vec::new()),
            }
        }
        // Surface the failure rather than the bytes that did get through: a
        // count smaller than the payload reads as a completed write to a caller
        // that trusts the full-write contract. A zero return on a non-empty
        // tail cannot make progress either, so it fails too.
        return (-(if ret == 0 { libc::EIO } else { errno }), Vec::new());
    }
}

/// Flush the descriptor's kernel buffers. `fsync(2)` transfers what the
/// process already handed over, so there is no peer to wait on.
pub(super) fn flush(fd: RawFd) -> (i32, Vec<u8>) {
    if unsafe { libc::fsync(fd) } < 0 {
        return (
            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
            Vec::new(),
        );
    }
    (0, Vec::new())
}
