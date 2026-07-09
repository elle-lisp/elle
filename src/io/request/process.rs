//! Subprocess handle stored behind an `IoOp::Spawn`/`ProcessWait` request.

use std::cell::RefCell;
use std::process::Child;

/// Handle to a running subprocess. Stored as ExternalObject with type_name "process".
#[derive(Debug)]
pub(crate) struct ProcessHandle {
    pid: u32,
    pub(crate) inner: RefCell<ProcessState>,
}

/// Lifecycle state of a subprocess.
#[derive(Debug)]
pub(crate) enum ProcessState {
    Running(Child),
    Exited(i32), // cached exit code
}

impl ProcessHandle {
    pub fn new(pid: u32, child: Child) -> Self {
        ProcessHandle {
            pid,
            inner: RefCell::new(ProcessState::Running(child)),
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }
}

/// Reap the subprocess on drop to prevent zombie accumulation.
/// `try_wait` is non-blocking; if the process hasn't exited yet,
/// it stays in the OS process table until it does.
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if let ProcessState::Running(ref mut child) = *self.inner.borrow_mut() {
            let _ = child.try_wait();
        }
    }
}
