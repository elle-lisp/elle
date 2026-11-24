// audited: 2026-11-24
//! The settle `--dump=stats` runs before it reports: the JIT's queued compiles dropped, and the one in progress waited for.
//!
//! docs/config.md
//! docs/impl/jit.md

use super::core::VM;

impl VM {
    /// Take back every compile still queued, release the pin each one held,
    /// then wait for the compile in progress, and answer how many it took
    /// back. `--dump=stats` settles rather than drains, so an exit does not
    /// wait for the queue to empty (docs/config.md). A compile taken back
    /// counts nowhere in `jit_stats`, as one the queue abandons counts
    /// nowhere (docs/impl/jit.md).
    pub fn settle_jit_queue(&mut self) -> usize {
        let queued = self
            .jit_worker
            .as_ref()
            .map_or_else(Vec::new, |worker| worker.take_queued());
        for key in &queued {
            self.jit_pending.remove(key);
        }
        self.drain_jit_pending();
        queued.len()
    }
}
