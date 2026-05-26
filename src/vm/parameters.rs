//! Parameter resolution for dynamic parameters.
//!
//! Walks the fiber's `param_frames` stack from top to bottom,
//! returning the first binding for the given parameter id.
//! Falls back to the parameter's default value.

use crate::value::fiber::FiberHandle;
use crate::value::Value;

use super::core::VM;

impl VM {
    /// Resolve a parameter's current value.
    ///
    /// Searches `param_frames` from top (most recent `parameterize`)
    /// to bottom. Returns the default if no binding is found.
    pub(crate) fn resolve_parameter(&self, id: u32, default: Value) -> Value {
        for frame in self.fiber.param_frames.iter().rev() {
            for &(param_id, value) in frame {
                if param_id == id {
                    return value;
                }
            }
        }
        default
    }

    /// Snapshot the current fiber's `param_frames` into a freshly-created
    /// child fiber.
    ///
    /// Called by the dispatcher right after `fiber/new` returns, so the
    /// child inherits the dynamic parameter bindings active at *creation*
    /// time — independent of which fiber later resumes it.  This matters
    /// for `ev/spawn`, where the spawning fiber may finish before the
    /// scheduler ever gets around to resuming the child: at resume time
    /// the spawner's `parameterize` frames have long since unwound.
    ///
    /// All frames are flattened into one to keep lookup cost constant
    /// and to mirror the user-visible semantics — the child observes the
    /// resolved value of each parameter at the moment of creation, not
    /// the parent's frame layout.
    pub(crate) fn snapshot_param_frames_into(&self, child: &FiberHandle) {
        if self.fiber.param_frames.is_empty() {
            return;
        }
        let mut flat: Vec<(u32, Value)> = Vec::new();
        for frame in &self.fiber.param_frames {
            for &(id, val) in frame {
                if let Some(pos) = flat.iter().position(|&(k, _)| k == id) {
                    flat[pos].1 = val;
                } else {
                    flat.push((id, val));
                }
            }
        }
        child.with_mut(|c| c.param_frames = vec![flat]);
    }
}
