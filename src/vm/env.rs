// audited: 2026-09-19
//! Closure environment building.
//!
//! Constructs the `Vec<Value>` environment a closure receives at call time:
//! captured variables, positional arguments (celled when the capture mask
//! names them), rest-parameter collection (`rest.rs`), and local slots.
//!
//! Entry points:
//! - `build_closure_env`: reuses `env_cache` to avoid a fresh allocation per call
//! - `populate_env`: fills a caller-supplied buffer; shared by `build_closure_env`
//!   and `tail_call_inner` (which uses `tail_call_env_cache`)

use crate::hir::region::RuntimeRegion;
use crate::value::Value;
use std::rc::Rc;

use super::core::VM;

mod rest;

/// Mint the runtime region for one closure-env value — a capture cell, a
/// rest-arg cons, the `&keys`/`&named` struct, or a captured-local cell.
///
/// docs/regions/semantics.md Rule 6 (no commingling) and the core principle "every value
/// its own region": each env value gets its OWN fresh runtime region instead of
/// sharing one per-call "env region". A value-based release of any one (the
/// owned-params calling convention) then frees only that value, never a
/// co-located live neighbour — so an owned-param `DecrefValueRegion` cannot free
/// a whole env region out from under a still-live `CaptureCell`. Each env value
/// gets its own fresh `RuntimeRegion`.
#[inline]
fn env_value_region(heap: &mut crate::value::fiberheap::FiberHeap) -> RuntimeRegion {
    heap.new_runtime_region()
}

impl VM {
    /// Build a closure environment from captured variables and arguments.
    ///
    /// Reuses `self.env_cache` to avoid a fresh Vec allocation per call.
    /// Returns `None` if `populate_env` fails (e.g., bad keyword args for `&keys`/`&named`).
    pub fn build_closure_env(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> Option<Rc<Vec<Value>>> {
        // A regular (NON-tail) closure call: each non-captured fixed param is an
        // OWNED binding the callee releases at its `decref_point` (see the
        // Lambda arm of `src/hir/regions.rs` + `lower_lambda_body`). Hand the
        // callee one owning reference per such arg here (`own_params = true`),
        // balanced by that release. A tail call (`tail_call_inner`) is a pure
        // move and passes `false`.
        //
        // The memo is read before the mutable borrows below, so the message a
        // rejected `&named` key earns can spell it.
        let symbols = unsafe { self.symbols_ptr.as_ref() };
        if !Self::populate_env(
            &mut self.env_cache,
            unsafe { &mut *self.heap_ptr },
            &mut self.fiber,
            closure,
            args,
            true,
            symbols,
        ) {
            return None;
        }
        Some(Rc::new(self.env_cache.clone()))
    }

    /// Build a closure environment for a JIT TAIL call, mirroring
    /// `tail_call_inner`'s closure path (`src/vm/call.rs`). Uses
    /// `tail_call_env_cache` (it must not alias `env_cache`). Returns `None`
    /// (error set on fiber) on bad keyword args.
    ///
    /// `own_params` is what `tail_call_inner` decides from the same fact: an
    /// ordinary tail call is a pure MOVE (`false` — the caller's reference to
    /// each arg transfers to the callee, which releases it at the param's last
    /// use, so no `CallArgument` incref here), while a SPLICED tail call has no
    /// reference of the frame's to move and the callee mints one of its own
    /// (`true`; docs/impl/region/mechanism.md § "A spliced call's arguments come
    /// out of an array the convention owns").
    #[cfg(feature = "jit")]
    pub(crate) fn build_tail_call_env(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
        own_params: bool,
    ) -> Option<Rc<Vec<Value>>> {
        let symbols = unsafe { self.symbols_ptr.as_ref() };
        if !Self::populate_env(
            &mut self.tail_call_env_cache,
            unsafe { &mut *self.heap_ptr },
            &mut self.fiber,
            closure,
            args,
            own_params,
            symbols,
        ) {
            return None;
        }
        Some(Rc::new(self.tail_call_env_cache.clone()))
    }

    /// Build a closure environment for a C→Elle FFI callback invocation
    /// (the libffi trampoline in `src/ffi/callback.rs`), unified on
    /// `populate_env` exactly as `build_closure_env`/`build_tail_call_env` are
    /// — no duplicated env builder.
    ///
    /// **`own_params = false` (a pure MOVE, like a tail call).** The trampoline
    /// converts each C argument into a *fresh* Elle value — scalars/pointers are
    /// immediates; `:struct`/array/byte args are newly minted heap values (rc=1,
    /// `read_value_from_buffer` → `Value::array`/`Value::bytes`) — and does NOT
    /// retain them past the call (`Value` is `Copy`; dropping its `elle_args`
    /// Vec releases no region). So the single owning reference to each heap arg
    /// transfers to the callee, which releases it value-based at the param's
    /// last use. Hence NO `CallArgument` incref: `own_params = true` would add a
    /// reference nothing balances (the trampoline never decrefs), leaking the
    /// converted arg.
    ///
    /// The env values `populate_env` itself constructs (capture cells, rest-list
    /// conses, captured-local cells, `&keys`/`&named` structs) each get their
    /// OWN fresh per-execution region via `env_value_region` (docs/impl/region/rules.md
    /// Rule 6, no commingling). `populate_env` allocates every env value through
    /// an explicit region (`env_value_region`/`alloc_in_region`), so no region is
    /// established here.
    ///
    /// Returns `None` (error set on the fiber) on bad `&keys`/`&named` args.
    pub fn build_callback_env(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> Option<Rc<Vec<Value>>> {
        let mut buf = Vec::new();
        let symbols = unsafe { self.symbols_ptr.as_ref() };
        let ok = Self::populate_env(
            &mut buf,
            unsafe { &mut *self.heap_ptr },
            &mut self.fiber,
            closure,
            args,
            false,
            symbols,
        );
        if !ok {
            return None;
        }
        Some(Rc::new(buf))
    }

    /// Populate an environment buffer with captures, arguments, and local slots.
    ///
    /// Shared by `build_closure_env` (which uses `env_cache`) and
    /// `tail_call_inner` (which uses `tail_call_env_cache`). The two caches
    /// can't alias — a tail call may occur inside a closure call that is
    /// still using `env_cache`.
    ///
    /// Capture cells and rest-arg cons cells are allocated directly via
    /// `heap.alloc_in_region()`, each into its own region (`env_value_region`).
    ///
    /// Returns `false` if keyword argument collection fails (error set on fiber).
    ///
    /// `symbols` is the calling instance's display memo, carried only so that
    /// a rejected `&named` key is named rather than hashed in the error
    /// message (docs/impl/symbol.md § "The display memo").
    pub(super) fn populate_env(
        buf: &mut Vec<Value>,
        heap: &mut crate::value::fiberheap::FiberHeap,
        fiber: &mut crate::value::Fiber,
        closure: &crate::value::Closure,
        args: &[Value],
        own_params: bool,
        symbols: Option<&crate::symbol::SymbolTable>,
    ) -> bool {
        buf.clear();
        let needed = closure.env_capacity();
        if buf.capacity() < needed {
            buf.reserve(needed - buf.len());
        }
        buf.extend(closure.env.iter().copied());

        match closure.template.arity() {
            crate::value::Arity::AtLeast(min) => {
                // Total fixed slots = num_params - 1 (rest slot is last param)
                let fixed_slots = closure.template.num_params() - 1;

                // Determine how many positional args to consume for fixed slots.
                // For &keys/&named, keyword args should not fill optional slots —
                // once we see a keyword past the required params, the rest are
                // keyword arguments for the collector.
                let collects_keywords = matches!(
                    closure.template.vararg_tag(),
                    crate::value::VarargTag::Struct | crate::value::VarargTag::StrictStruct
                );
                let provided_fixed = if collects_keywords {
                    // Always fill required slots, then fill optional slots
                    // only with non-keyword args
                    let mut count = args.len().min(min);
                    while count < fixed_slots && count < args.len() {
                        if args[count].is_keyword() {
                            break;
                        }
                        count += 1;
                    }
                    count
                } else {
                    args.len().min(fixed_slots)
                };

                // Push args for fixed slots (required + optional)
                for (i, arg) in args[..provided_fixed].iter().enumerate() {
                    Self::push_param(buf, heap, closure, i, *arg, own_params);
                }
                // Fill missing optional slots with nil
                for i in provided_fixed..fixed_slots {
                    Self::push_param(buf, heap, closure, i, Value::NIL, own_params);
                }

                // Collect remaining args into rest slot
                let rest_args = if args.len() > provided_fixed {
                    &args[provided_fixed..]
                } else {
                    &[]
                };
                let collected = match closure.template.vararg_tag() {
                    crate::value::VarargTag::List => Self::args_to_list(rest_args, heap),
                    crate::value::VarargTag::Struct => {
                        match Self::collect_struct_in_own_region(
                            fiber, heap, rest_args, None, symbols,
                        ) {
                            Some(v) => v,
                            None => return false,
                        }
                    }
                    crate::value::VarargTag::StrictStruct => {
                        let keys = closure.template.strict_keys();
                        match Self::collect_struct_in_own_region(
                            fiber,
                            heap,
                            rest_args,
                            Some(keys),
                            symbols,
                        ) {
                            Some(v) => v,
                            None => return false,
                        }
                    }
                };
                // On a MOVE (`own_params = false`: a tail call / FFI callback) the
                // caller's owning reference to each arg transferred to us, and a
                // collected arg lands in `collected` — which took its own reference
                // when it stored the value — rather than in an env slot. So the
                // moved reference is surplus and is released here, whichever
                // collector took the value over (docs/impl/region/mechanism.md
                // § "A collector parameter takes the moved reference over itself").
                // Released after `collected` is built, so the collection's own
                // reference already stands.
                if !own_params {
                    Self::release_moved_rest_args(rest_args, args, heap);
                }
                // The rest-param's collected list/struct is built into the env
                // region here, not moved in by the caller — it is a borrow, not
                // an owned param, so no caller incref balances it: `false`.
                Self::push_param(buf, heap, closure, fixed_slots, collected, false);
            }
            crate::value::Arity::Range(_, max) => {
                // All slots are fixed (no rest param)
                // Push provided args
                for (i, arg) in args.iter().enumerate() {
                    Self::push_param(buf, heap, closure, i, *arg, own_params);
                }
                // Fill missing optional slots with nil
                for i in args.len()..max {
                    Self::push_param(buf, heap, closure, i, Value::NIL, own_params);
                }
            }
            crate::value::Arity::Exact(_) => {
                for (i, arg) in args.iter().enumerate() {
                    Self::push_param(buf, heap, closure, i, *arg, own_params);
                }
            }
        }

        // Add slots for locally-defined variables.
        // Cell-wrapped locals (captured by nested closures, or mutated) get
        // a CaptureCell holding NIL. Non-cell locals get bare NIL — they use
        // stack slots via StoreLocal/LoadLocal, and the env slot is never
        // accessed. The `capture_locals_mask` names every local precisely at
        // any index, so an uncaptured local — even one beyond slot 63 — gets a
        // bare NIL and never a dead, leaked cell.
        let num_locally_defined = closure
            .template
            .num_locals()
            .saturating_sub(closure.template.num_params());
        for i in 0..num_locally_defined {
            if closure.template.capture_locals_mask().is_set(i) {
                // Each captured-local cell gets its own region (see `env_value_region`).
                let cell_region = env_value_region(heap);
                buf.push(crate::value::build::capture_cell(
                    heap,
                    Value::NIL,
                    crate::value::heap::CellOrigin::Runtime,
                    cell_region,
                ));
            } else {
                buf.push(Value::NIL);
            }
        }

        true
    }

    /// Push a parameter value into the environment buffer, wrapping in a
    /// CaptureCell if the capture_params_mask indicates it's needed.
    #[inline]
    fn push_param(
        buf: &mut Vec<Value>,
        heap: &mut crate::value::fiberheap::FiberHeap,
        closure: &crate::value::Closure,
        i: usize,
        val: Value,
        own_params: bool,
    ) {
        if i < 64 && (closure.template.capture_params_mask() & (1 << i)) != 0 {
            // alloc_in_region → alloc_obj → incref_cross_region_refs handles
            // the cross-region incref for the wrapped value automatically. An
            // LBox/captured param is owned by its cell (not an owned local), so
            // it takes NO `CallArgument` incref — `own_params` does not apply.
            // Each capture cell gets its own region (see `env_value_region`).
            let cell_region = env_value_region(heap);
            buf.push(crate::value::build::capture_cell(
                heap,
                val,
                crate::value::heap::CellOrigin::Runtime,
                cell_region,
            ));
        } else {
            // Non-captured fixed param. On the NON-tail closure path
            // (`own_params`), the callee owns this param and releases it
            // value-based at its `decref_point` (`DecrefValueRegion` reading the
            // param slot). Hand it one owning reference here so that release
            // balances; use `result_region_of` to match the region the callee's
            // `DecrefValueRegion` will target (it sees through a capture-cell
            // wrapper identically). A tail call passes `own_params = false` (the
            // arg is a pure move — the caller's reference transfers). Immediates
            // (region `None`) no-op.
            if own_params {
                let r = crate::value::arena::result_region_of(heap, val);
                crate::value::arena::incref_for_escape(
                    heap,
                    r,
                    crate::value::arena::EscapeSite::CallArgument,
                );
            }
            buf.push(val);
        }
    }
}

#[cfg(test)]
mod callback_env_tests;
