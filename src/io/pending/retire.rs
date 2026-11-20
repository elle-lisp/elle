// audited: 2026-11-20
//! What an entry gives back when nobody will read its result: buffers, descriptors, a directory, a kernel's record.
//!
//! docs/impl/io/cancel.md
//! docs/impl/io/inflight.md

use super::{FileStage, PendingOp};
use crate::io::pool::BufferPool;
use crate::io::request::FileKind;

impl PendingOp {
    /// Give back everything this operation owns, without building a value for
    /// it. That is the pooled buffer, a descriptor the completion would have
    /// wrapped in a port, the directory a temporary-directory operation made,
    /// and the `siginfo_t` a process wait allocated. A file operation on the
    /// ring gives back what its stage holds.
    ///
    /// `result_fd` is the raw completion's result code, which for a connect, an
    /// open or an accept is the descriptor the operation obtained. Nobody will
    /// take it now, so it is closed here rather than leaked. `data` is the raw
    /// completion's bytes, which for a pool worker's temporary directory are
    /// its path.
    ///
    /// A process wait on the ring reaped nothing, since it asks with `WNOWAIT`,
    /// so its `siginfo_t` is all there is to give back. The pool's wait reaps
    /// in the worker, through the exit record (docs/impl/io/process.md).
    pub(crate) fn retire(self, result_fd: i32, data: &[u8], buffer_pool: &mut BufferPool) {
        if let Some(bh) = self.buffer_handle() {
            buffer_pool.release(bh);
        }
        match self {
            PendingOp::Connect { connect_fd, .. } => {
                // io_uring pre-creates the socket; the pool reports it here.
                if let Some(fd) = connect_fd.or(if result_fd > 0 { Some(result_fd) } else { None })
                {
                    // SAFETY: nothing else holds this descriptor — the port that
                    // would have owned it is never built.
                    unsafe { libc::close(fd) };
                }
            }
            PendingOp::Open { .. } if result_fd > 0 => {
                // SAFETY: as above — the port for this descriptor is never built.
                unsafe { libc::close(result_fd) };
            }
            // An accept that succeeded owns a descriptor too: the connection the
            // kernel handed back. `listener_kind` is what says this entry is an
            // accept, and a negative `result_fd` is a failure or a cancellation,
            // which produced none. A server whose accept loop is aborted retires
            // an accept on every round, so leaving this one open leaks a socket
            // per round.
            PendingOp::Port {
                listener_kind: Some(_),
                ..
            } if result_fd > 0 => {
                // SAFETY: as above — the port for this descriptor is never built.
                unsafe { libc::close(result_fd) };
            }
            PendingOp::ProcessWait { siginfo, .. } if !siginfo.is_null() => {
                // SAFETY: allocated by `Box::into_raw` at submit; reclaimed once.
                drop(unsafe { Box::from_raw(siginfo) });
            }
            // A temporary directory a pool worker made, whose name only `data`
            // holds. A removal that fails found it gone, or filled by a process
            // that guessed the name, and there is nobody left to tell.
            PendingOp::File {
                kind: FileKind::MakeTempDir,
                stage: FileStage::Whole,
                ..
            } if result_fd >= 0 => {
                use std::os::unix::ffi::OsStrExt;
                let _ = std::fs::remove_dir(std::ffi::OsStr::from_bytes(data));
            }
            PendingOp::File { stage, .. } => stage.retire(result_fd),
            _ => {}
        }
    }
}
