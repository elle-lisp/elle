//! audited: 2026-11-09
//! `Ring`: the ring, and the one `io_uring_enter` that sleeps on it for CQEs.
//!
//! src/io/AGENTS.md
//! docs/io.md

use io_uring::types::{SubmitArgs, Timespec};
use io_uring::{EnterFlags, IoUring};
use std::io;
use std::ops::{Deref, DerefMut};
use std::time::Duration;

/// The ring every submission reaches through `Deref`, and whose wait is
/// [`Ring::wait`]: the one `io_uring_enter` that sleeps.
pub(crate) struct Ring {
    ring: IoUring,
    /// Whether this kernel's wait takes `IORING_ENTER_NO_IOWAIT`.
    no_iowait: bool,
}

impl Ring {
    /// Open a ring of `entries` submission and completion slots.
    pub(crate) fn open(entries: u32) -> io::Result<Ring> {
        let ring = IoUring::new(entries)?;
        Ok(Ring {
            ring,
            no_iowait: false,
        })
    }

    /// Whether this kernel's wait takes `IORING_ENTER_NO_IOWAIT`.
    #[cfg(test)]
    pub(crate) fn no_iowait(&self) -> bool {
        self.no_iowait
    }

    /// Submit what the queue holds and wait for one CQE, or until `timeout`
    /// passes; `None` waits as long as it takes.
    ///
    /// A signal that interrupts the wait restarts it with the same bound. A
    /// bound that passes with nothing to report answers `Ok`, like a CQE: the
    /// caller drains the queue either way and reads the difference there.
    pub(crate) fn wait(&mut self, timeout: Option<Duration>) -> Result<(), String> {
        let flags = wait_flags(self.no_iowait, timeout.is_some()).bits();
        loop {
            let to_submit = self.ring.submission().len() as u32;
            let entered = match timeout {
                Some(t) => {
                    let ts = Timespec::new().sec(t.as_secs()).nsec(t.subsec_nanos());
                    let args = SubmitArgs::new().timespec(&ts);
                    // SAFETY: `args` points at `ts`, and both outlive the call
                    // that reads them.
                    unsafe {
                        self.ring
                            .submitter()
                            .enter(to_submit, 1, flags, Some(&args))
                    }
                }
                // SAFETY: no argument, so the kernel reads nothing of ours.
                None => unsafe {
                    self.ring
                        .submitter()
                        .enter::<libc::sigset_t>(to_submit, 1, flags, None)
                },
            };
            match entered {
                Ok(_) => return Ok(()),
                Err(e) if e.raw_os_error() == Some(libc::EINTR) => continue,
                Err(e) if e.raw_os_error() == Some(libc::ETIME) => return Ok(()),
                Err(e) => return Err(format!("io/wait: io_uring wait failed: {e}")),
            }
        }
    }
}

impl Deref for Ring {
    type Target = IoUring;

    fn deref(&self) -> &IoUring {
        &self.ring
    }
}

impl DerefMut for Ring {
    fn deref_mut(&mut self) -> &mut IoUring {
        &mut self.ring
    }
}

/// The flags one wait enters the kernel with: `GETEVENTS` always, `EXT_ARG`
/// when the wait is `timed`, which is how the timespec travels, and
/// `NO_IOWAIT` when the kernel takes it, which keeps the wait out of iowait.
pub(crate) fn wait_flags(_no_iowait: bool, timed: bool) -> EnterFlags {
    let mut flags = EnterFlags::GETEVENTS;
    if timed {
        flags |= EnterFlags::EXT_ARG;
    }
    flags
}

#[cfg(test)]
mod tests;
