//! `FiberStatus`: the fiber lifecycle enum and its display name.

/// Fiber lifecycle status. Diverges from Janet: caught SIG_ERROR leaves
/// fiber Suspended (resumable), not Error. See vm/fiber.rs for details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberStatus {
    /// Not yet started (has closure but hasn't been resumed)
    New,
    /// Currently executing (on the VM's run stack)
    Alive,
    /// Paused by a signal (waiting for resume)
    Paused,
    /// Completed normally (returned a value)
    Dead,
    /// Terminated by an unhandled error signal
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
