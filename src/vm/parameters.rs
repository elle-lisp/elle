// audited: 2026-11-12
//! Dynamic parameters: the binding a call to a parameter reads, and the reference that call hands back.
//!
//! docs/parameters.md
//! docs/impl/region/generations.md

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

    /// Call the parameter `(id, default)` with `nargs` arguments, on either tier
    /// and in either position: its value, with one reference for the caller.
    ///
    /// A resolve never allocates into the call's region, so the caller's release
    /// of the call's result consumes this retain rather than the binding frame's
    /// own reference. An immediate has no region, and costs nothing. A call with
    /// arguments raises an `arity-error` and answers `None`.
    pub(crate) fn call_parameter(
        &mut self,
        id: u32,
        default: Value,
        nargs: usize,
    ) -> Option<Value> {
        if nargs != 0 {
            self.set_error(
                "arity-error",
                format!("parameter call: expected 0 arguments, got {}", nargs),
            );
            return None;
        }
        let value = self.resolve_parameter(id, default);
        let heap = unsafe { &mut *self.heap_ptr };
        let region = crate::value::arena::region_of(heap, value);
        crate::value::arena::incref_for_escape(
            heap,
            region,
            crate::value::arena::EscapeSite::ParameterResolve,
        );
        Some(value)
    }

    /// Panic (debug builds) if the seeded borrow for parameter `id` points into
    /// a region freed since this fiber inherited it — the deref-site companion
    /// of the resume checkpoint. Reads only the generation counter, never the
    /// resolved value's page, so it cannot itself fault on a stale value
    /// (docs/impl/region/generations.md).
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
                     inherited it (docs/impl/region/generations.md, the \
                     uncounted-borrow check)"
                );
            }
        }
    }
}
