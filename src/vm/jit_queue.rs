// audited: 2026-11-24
//! The settle `--dump=stats` runs before it reports: the JIT's finished compiles taken, and the worker dropped with the rest.
//!
//! docs/config.md
//! docs/impl/jit.md

use super::core::VM;

impl VM {
    /// Take every result the worker has finished, then drop the worker, and
    /// answer how many compiles were still pending. The drop discards the
    /// queue and stops the compile in progress, as a VM's drop does, so
    /// `--dump=stats` waits for no compile (docs/config.md). A dropped compile
    /// releases its pin and counts nowhere in `jit_stats`, as one the queue
    /// abandons counts nowhere (docs/impl/jit.md).
    pub fn settle_jit_queue(&mut self) -> usize {
        self.poll_jit_completions();
        self.jit_worker = None;
        let dropped = self.jit_pending.len();
        self.jit_pending.clear();
        dropped
    }
}
