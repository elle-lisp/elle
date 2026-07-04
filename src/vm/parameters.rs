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
        for (idx, frame) in self.fiber.param_frames.iter().enumerate().rev() {
            for &(param_id, value) in frame {
                if param_id == id {
                    // A binding resolved from the inherited baseline frame
                    // (index 0) is a seeded cross-fiber borrow; confirm its
                    // region is still live (debug builds). Bindings from inner
                    // `parameterize` frames are this fiber's own and carry no
                    // recorded borrow.
                    #[cfg(debug_assertions)]
                    if idx == 0 {
                        self.check_param_borrow_fresh(id);
                    }
                    #[cfg(not(debug_assertions))]
                    let _ = idx;
                    return value;
                }
            }
        }
        default
    }

    /// Panic (debug builds) if the seeded borrow for parameter `id` points into
    /// a region freed since this fiber inherited it — the deref-site companion
    /// of the resume checkpoint. Reads only the generation counter, never the
    /// resolved value's page, so it cannot itself fault on a stale value
    /// (docs/impl/region-generations.md § "Uncounted-borrow check").
    #[cfg(debug_assertions)]
    fn check_param_borrow_fresh(&self, id: u32) {
        // SAFETY: `heap_ptr` is the VM's own region heap (a leaked Box, shared
        // per-thread, never moved); a shared read is the sanctioned split-borrow
        // access here, since `resolve_parameter` is `&self`. This is the same
        // heap that seeded the borrow, so the generations are comparable.
        let heap = unsafe { &*self.heap_ptr };
        for &(pid, r, gen) in &self.fiber.param_borrows {
            if pid == id {
                assert!(
                    heap.generation_raw(r.get()) == gen,
                    "stale param-snapshot borrow at deref: parameter {pid} resolved \
                     to a value in region {r}, which was freed since this fiber \
                     inherited it (docs/impl/region-generations.md \
                     § 'Uncounted-borrow check')"
                );
            }
        }
    }
}
