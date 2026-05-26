//! Parameter resolution for dynamic parameters.
//!
//! Walks the fiber's `param_frames` stack from top to bottom,
//! returning the first binding for the given parameter id.
//! Falls back to the parameter's default value.

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
}
