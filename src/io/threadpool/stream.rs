//! The byte-stream operations a worker runs: the four reads, the write, and
//! the flush. Each owns its `OpBound` for the operation's lifetime, so its
//! syscalls are non-blocking and every wait is answerable.

use super::*;

/// Read up to `size` bytes once.
pub(super) fn read(bound: OpBound, fd: RawFd, size: usize) -> (i32, Vec<u8>) {
    let mut buf = vec![0u8; size];
    loop {
        let ret = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, size) };
        if ret >= 0 {
            buf.truncate(ret as usize);
            return (ret as i32, buf);
        }
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        if errno == libc::EINTR {
            continue;
        }
        if is_would_block(errno) {
            match bound.wait(libc::POLLIN) {
                Wake::Ready => continue,
                Wake::Stopped => return (-libc::ECANCELED, Vec::new()),
                Wake::TimedOut => return (-libc::ETIMEDOUT, Vec::new()),
            }
        }
        return (-errno, Vec::new());
    }
}

/// Read exactly `size` units, looping until full or EOF/error. Units are bytes
/// unless `graphemes`, in which case they are grapheme clusters counted under
/// `gen`.
pub(super) fn read_exact(
    bound: OpBound,
    fd: RawFd,
    size: usize,
    graphemes: bool,
    gen: crate::segment::Generation,
) -> (i32, Vec<u8>) {
    // Buffer grows as we go — graphemes mode can't preallocate because we
    // don't know the byte count in advance.  In bytes mode we still grow into
    // a `size`-capacity Vec so the loop's tail-read writes into one buffer.
    let mut buf: Vec<u8> = if graphemes {
        Vec::with_capacity(size)
    } else {
        vec![0u8; size]
    };
    let mut total = 0usize;
    // `want` is how many bytes we ask the kernel for on each iteration.  Bytes
    // mode knows exactly (size - total); graphemes mode estimates one byte per
    // missing grapheme (ASCII best case) and loops on undershoot.
    loop {
        let want = if graphemes {
            // Re-evaluate progress every iteration.
            let g = grapheme_count_in_valid_prefix(&buf[..total], gen);
            if g >= size {
                return (total as i32, buf[..total].to_vec());
            }
            (size - g).max(1)
        } else {
            if total >= size {
                return (total as i32, buf);
            }
            size - total
        };
        // Make room for the next read if we're in graphemes mode (bytes mode
        // preallocated).
        if graphemes && buf.len() < total + want {
            buf.resize(total + want, 0);
        }
        let ret = unsafe { libc::read(fd, buf[total..].as_mut_ptr() as *mut libc::c_void, want) };
        if ret < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            if errno == libc::EINTR {
                continue;
            }
            if is_would_block(errno) {
                match bound.wait(libc::POLLIN) {
                    Wake::Ready => continue,
                    Wake::Stopped => return (-libc::ECANCELED, Vec::new()),
                    // The deadline passed with nothing more arriving. That is
                    // the caller's timeout, not the end of the stream —
                    // surfacing the partial here would read as EOF and the
                    // completion would map it to nil.
                    Wake::TimedOut => return (-libc::ETIMEDOUT, Vec::new()),
                }
            }
            if total == 0 {
                return (-errno, Vec::new());
            }
            // Partial read then error: surface what we got so the completion
            // path treats it as short-then-EOF.
            return (total as i32, buf[..total].to_vec());
        }
        if ret == 0 {
            // EOF before full count.  Return short; the completion handler
            // maps short-on-ReadExact to nil.
            return (total as i32, buf[..total].to_vec());
        }
        total += ret as usize;
    }
}

/// Read until a newline arrives (`until_newline`) or until EOF.
pub(super) fn read_until(bound: OpBound, fd: RawFd, until_newline: bool) -> (i32, Vec<u8>) {
    let mut accumulated = Vec::new();
    let mut chunk = vec![0u8; 4096];
    loop {
        let ret = unsafe { libc::read(fd, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len()) };
        if ret < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            if errno == libc::EINTR {
                continue;
            }
            if is_would_block(errno) {
                match bound.wait(libc::POLLIN) {
                    Wake::Ready => continue,
                    Wake::Stopped => return (-libc::ECANCELED, Vec::new()),
                    // The deadline passed with nothing more arriving. Report
                    // the timeout rather than the partial, which the completion
                    // would treat as a line or a stream that ended.
                    Wake::TimedOut => return (-libc::ETIMEDOUT, Vec::new()),
                }
            }
            if accumulated.is_empty() {
                return (-errno, Vec::new());
            }
            // Return whatever we accumulated before the error.
            return (accumulated.len() as i32, accumulated);
        }
        if ret == 0 {
            // EOF — return whatever we have.
            return (accumulated.len() as i32, accumulated);
        }
        accumulated.extend_from_slice(&chunk[..ret as usize]);
        if until_newline && accumulated.contains(&b'\n') {
            return (accumulated.len() as i32, accumulated);
        }
    }
}

/// Write every byte of `data`.
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
pub(super) fn write(bound: OpBound, fd: RawFd, data: Vec<u8>) -> (i32, Vec<u8>) {
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
