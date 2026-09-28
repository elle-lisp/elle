// audited: 2026-09-28
//! `FiberStatus`: the fiber lifecycle enum and its display name.
//!
//! docs/signals/fibers.md

/// Fiber lifecycle status. Diverges from Janet: a caught SIG_ERROR leaves the
/// fiber Paused (resumable), not Error. See vm/fiber.rs for details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberStatus {
    /// Not yet started (has closure but hasn't been resumed)
    New,
    /// Currently executing (on the VM's run stack)
    Alive,
    /// Paused by a signal (waiting for resume)
    Paused,
    /// Completed, halted, or cancelled — it never runs again
    Dead,
    /// Stopped by an error its mask did not catch, or by `fiber/abort`; a
    /// resume restarts it at the raising call
    Error,
}

impl FiberStatus {
    /// Human-readable name for display formatting.
    pub fn as_str(self) -> &'static str {
        match self {
            FiberStatus::New => "new",
            FiberStatus::Alive => "alive",
            FiberStatus::Paused => "paused",
            FiberStatus::Dead => "dead",
            FiberStatus::Error => "error",
        }
    }
}
