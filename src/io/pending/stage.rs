// audited: 2026-11-20
//! FileStage — where a file operation the ring runs stands between two of its CQEs, and what it owns there.
//!
//! docs/impl/io/platforms.md
//! docs/impl/io/cancel.md
//! docs/impl/io/bytes.md

#[cfg(target_os = "linux")]
use std::os::unix::io::OwnedFd;

/// Where a file operation stands between two of its CQEs, and what it owns
/// there. Only the ring runs an operation in steps; a pool worker runs the
/// whole of one in its own call.
pub(crate) enum FileStage {
    /// Nothing to keep: a pool worker runs the operation, or one SQE answers
    /// it with its result code.
    Whole,
    /// A stat, a size or a probe, whose record the kernel writes into the
    /// buffer.
    #[cfg(target_os = "linux")]
    Stat(StatxBuf),
    /// A temporary directory under `root`: the name the kernel is making, and
    /// how many names the operation has tried.
    #[cfg(target_os = "linux")]
    TempDir {
        root: String,
        making: String,
        tried: u64,
    },
    /// A sync. Before its open answers it holds nothing. Then it holds the
    /// descriptor, and the stat that checks the descriptor until that stat
    /// answers.
    #[cfg(target_os = "linux")]
    Sync {
        fd: Option<OwnedFd>,
        stat: Option<StatxBuf>,
    },
}

#[cfg(target_os = "linux")]
impl FileStage {
    /// A stat whose record the kernel has yet to write.
    pub(crate) fn stat() -> FileStage {
        FileStage::Stat(StatxBuf::new())
    }

    /// A temporary directory under `root`, at its first name.
    pub(crate) fn temp_dir(root: &str) -> FileStage {
        FileStage::TempDir {
            root: root.to_string(),
            making: next_temp_name(root),
            tried: 1,
        }
    }

    /// A sync whose open has not answered.
    pub(crate) fn sync() -> FileStage {
        FileStage::Sync {
            fd: None,
            stat: None,
        }
    }

    /// The name a temporary directory is making, or `None` for any other
    /// stage.
    pub(crate) fn making(&self) -> Option<&str> {
        match self {
            FileStage::TempDir { making, .. } => Some(making),
            _ => None,
        }
    }
}

impl FileStage {
    /// Give back what this stage owns, now that the CQE answering `result` has
    /// arrived and nobody will read it. A stat's buffer is freed, and a
    /// temporary directory the kernel made is removed. A sync's descriptor is
    /// closed, the one its open answered with `result` included.
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    pub(crate) fn retire(self, result: i32) {
        match self {
            FileStage::Whole => {}
            // SAFETY: the CQE of the SQE that named the buffer has arrived.
            #[cfg(target_os = "linux")]
            FileStage::Stat(buf) => unsafe { buf.free() },
            // A removal that fails found the directory gone, or filled by a
            // process that guessed the name, and nobody is left to tell.
            #[cfg(target_os = "linux")]
            FileStage::TempDir { making, .. } => {
                if result >= 0 {
                    let _ = std::fs::remove_dir(making);
                }
            }
            #[cfg(target_os = "linux")]
            FileStage::Sync { fd, stat } => {
                if let Some(buf) = stat {
                    // SAFETY: as for a stat's buffer above.
                    unsafe { buf.free() };
                }
                if fd.is_none() && result >= 0 {
                    // SAFETY: the open answered this descriptor, and nothing
                    // else holds it.
                    unsafe { libc::close(result) };
                }
            }
        }
    }
}

/// The next name a temporary directory under `root` tries, numbered from the
/// counter the pool's temporary directories take theirs from.
#[cfg(target_os = "linux")]
pub(crate) fn next_temp_name(root: &str) -> String {
    use crate::io::threadpool::{temp_candidate, TEMP_COUNTER};
    temp_candidate(
        root,
        TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    )
}

/// A `statx` the kernel writes into, owned by its entry until the CQE of the
/// SQE that names it arrives (docs/impl/io/bytes.md).
///
/// It has no `Drop`. A teardown that gives up on an operation drops the entry
/// unretired, and the kernel may still write here, so the allocation leaks
/// rather than return to the allocator under that write.
#[cfg(target_os = "linux")]
pub(crate) struct StatxBuf(*mut libc::statx);

#[cfg(target_os = "linux")]
impl StatxBuf {
    pub(crate) fn new() -> StatxBuf {
        // SAFETY: an all-zero statx is a valid value for the kernel to fill.
        StatxBuf(Box::into_raw(Box::new(unsafe { std::mem::zeroed() })))
    }

    /// Where the kernel writes.
    pub(crate) fn as_mut_ptr(&self) -> *mut libc::statx {
        self.0
    }

    /// What the kernel wrote, and the allocation back.
    ///
    /// # Safety
    ///
    /// The CQE of the SQE that named this buffer has arrived, so the kernel
    /// writes here no more.
    pub(crate) unsafe fn take(self) -> libc::statx {
        *Box::from_raw(self.0)
    }

    /// The allocation back, unread.
    ///
    /// # Safety
    ///
    /// As for [`take`](Self::take).
    pub(crate) unsafe fn free(self) {
        drop(Box::from_raw(self.0));
    }
}
