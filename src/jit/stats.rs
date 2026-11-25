// audited: 2026-11-23
//! What a VM's JIT compiles cost: one compile's wall and CPU time, and the sums and the rejected count over every compile the VM took.
//!
//! docs/impl/jit.md

use std::time::{Duration, Instant};

/// The time one compile took, on both clocks, read on the thread that ran it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CompileCost {
    wall: Duration,
    cpu: Duration,
}

impl CompileCost {
    /// Run `compile` on this thread, and answer what it returned and what it
    /// cost.
    pub(crate) fn measure<T>(compile: impl FnOnce() -> T) -> (T, CompileCost) {
        let (wall, cpu) = (Instant::now(), thread_cpu());
        let out = compile();
        let cost = CompileCost {
            wall: wall.elapsed(),
            cpu: thread_cpu().saturating_sub(cpu),
        };
        (out, cost)
    }
}

/// The CPU time of the calling thread.
fn thread_cpu() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid out-buffer, live for the call.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    assert_eq!(rc, 0, "the thread CPU clock is unreadable");
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// Every compile a VM took, how many produced no code, and what they cost
/// together. The rig's `jit/stats` reads it (rig/overview.md).
#[derive(Clone, Copy, Debug, Default)]
pub struct JitStats {
    compiles: u64,
    rejected: u64,
    wall: Duration,
    cpu: Duration,
}

impl JitStats {
    /// Count one compile the VM took, and count it rejected when `outcome`
    /// holds no code.
    pub(crate) fn take<T, E>(&mut self, cost: CompileCost, outcome: &Result<T, E>) {
        self.compiles += 1;
        self.rejected += u64::from(outcome.is_err());
        self.wall += cost.wall;
        self.cpu += cost.cpu;
    }

    /// How many compiles the VM took.
    pub fn compiles(&self) -> u64 {
        self.compiles
    }

    /// How many of them produced no code.
    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// Their wall time, summed.
    pub fn wall(&self) -> Duration {
        self.wall
    }

    /// Their CPU time, summed over the threads that ran them.
    pub fn cpu(&self) -> Duration {
        self.cpu
    }
}
