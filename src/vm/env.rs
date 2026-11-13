// audited: 2026-11-13
//! Closure environment building: captures, arguments, the rest collection and local cells, each in regions of its own.
//!
//! docs/impl/region/rules.md
//! docs/impl/region/handoff.md
//! docs/impl/region/restlist.md
//!
//! Constructs the `Vec<Value>` environment a closure receives at call time:
//! captured variables, positional arguments (celled when the capture mask
//! names them), rest-parameter collection (`rest.rs`), and local slots.
//!
//! Entry points:
//! - `build_closure_env`: reuses `env_cache` to avoid a fresh allocation per call
//! - `build_call_env`: the same, taking the moves a call offered
//! - `pending_tail_call_to`: the pending call both tail resolvers build, with
//!   an environment in `tail_call_env_cache` or the callee's compiled code
//! - `populate_env`: fills a caller-supplied buffer; shared by every builder here

use crate::hir::region::RuntimeRegion;
use crate::value::Value;
use std::rc::Rc;

use super::call::handoff::{fixed_arg_count, param_is_celled, ArgRefs};
use super::core::VM;

mod rest;

/// Mint the runtime region for one closure-env value — a capture cell, a
/// rest list or one cons of it, the `&keys`/`&named` struct, or a
/// captured-local cell.
///
/// Rule 6 (no commingling) and the principle "every value its own region": each
/// env value gets its OWN fresh runtime region, never one shared per-call "env
/// region". A value-based release of any one (the owned-params calling
/// convention) then frees only that value, never a co-located live neighbour —
/// so an owned-param `DecrefValueRegion` cannot free a shared region out from
/// under a still-live `CaptureCell`. A rest list takes one region per cons, or
/// one for the whole list where no cons can outlive the head
/// (docs/impl/region/restlist.md).
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
        self.build_call_env(closure, args, ArgRefs::Mint)
    }

    /// Build the environment of a NON-tail closure call, as
    /// [`Self::build_closure_env`] does, with `refs` naming the reference each
    /// owned parameter takes. The callee owns each non-captured fixed param and
    /// releases it at its `decref_point`, so each takes one: minted here, or
    /// the frame's own where the callee took the call's offer
    /// (docs/impl/region/handoff.md).
    pub(crate) fn build_call_env(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
        refs: ArgRefs,
    ) -> Option<Rc<Vec<Value>>> {
        // The memo is read before the mutable borrows below, so the message a
        // rejected `&named` key earns can spell it.
        let symbols = unsafe { self.symbols_ptr.as_ref() };
        if !Self::populate_env(
            &mut self.env_cache,
            unsafe { &mut *self.heap_ptr },
            &mut self.fiber,
            closure,
            args,
            refs,
            symbols,
        ) {
            return None;
        }
        Some(Rc::new(self.env_cache.clone()))
    }

    /// The pending tail call to `closure`, the value `func`, on `args`, which
    /// both tail resolvers build: `tail_call_inner` and the JIT's tail-call
    /// helper. Counts the call for the JIT. Returns `None` (error set on
    /// fiber) on bad keyword args.
    ///
    /// A callee with compiled code the trampoline can run gets that code and
    /// `args` (`compiled_tail_call`). Any other gets the environment
    /// `populate_env` builds in `tail_call_env_cache`, which must not alias
    /// `env_cache`. An ordinary tail call MOVES the caller's reference to each
    /// arg into the callee, which releases it at the param's last use. A
    /// SPLICED tail call has no reference of the frame's to move, so the callee
    /// mints one of its own (`ArgRefs::tail`; docs/impl/region/mechanism.md,
    /// the spliced call).
    pub(crate) fn pending_tail_call_to(
        &mut self,
        func: Value,
        closure: &crate::value::Closure,
        args: Vec<Value>,
        spliced_args: bool,
    ) -> Option<crate::vm::core::TailCallInfo> {
        #[cfg(feature = "jit")]
        if let Some(code) = self.tail_callee_code(closure) {
            return Some(self.compiled_tail_call(code, func, closure, args, spliced_args));
        }
        let symbols = unsafe { self.symbols_ptr.as_ref() };
        if !Self::populate_env(
            &mut self.tail_call_env_cache,
            unsafe { &mut *self.heap_ptr },
            &mut self.fiber,
            closure,
            &args,
            ArgRefs::tail(spliced_args),
            symbols,
        ) {
            return None;
        }
        Some(crate::vm::core::TailCallInfo::interpreted(
            closure.template.code(),
            Rc::new(self.tail_call_env_cache.clone()),
            func,
            crate::value::closure::Boundary::of(closure),
        ))
    }

    /// Build a closure environment for a C→Elle FFI callback invocation
    /// (the libffi trampoline in `src/ffi/callback.rs`), unified on
    /// `populate_env` exactly as `build_closure_env`/`pending_tail_call_to` are
    /// — no duplicated env builder.
    ///
    /// **`ArgRefs::Move` (a pure MOVE, like an unspliced tail call).** The
    /// trampoline converts each C argument into a *fresh* Elle value — scalars/pointers are
    /// immediates; `:struct`/array/byte args are newly minted heap values (rc=1,
    /// `read_value_from_buffer` → `Value::array`/`Value::bytes`) — and does NOT
    /// retain them past the call (`Value` is `Copy`; dropping its `elle_args`
    /// Vec releases no region). So the single owning reference to each heap arg
    /// transfers to the callee, which releases it value-based at the param's
    /// last use. Hence NO `CallArgument` incref: a mint would add a
    /// reference nothing balances (the trampoline never decrefs), leaking the
    /// converted arg.
    ///
    /// The env values `populate_env` itself constructs (capture cells, the rest
    /// list, captured-local cells, `&keys`/`&named` structs) each get regions of
    /// their OWN via `env_value_region` (Rule 6, no commingling). `populate_env`
    /// allocates every env value through an explicit region
    /// (`env_value_region`/`alloc_in_region`), so no region is established here.
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
            ArgRefs::Move,
            symbols,
        );
        if !ok {
            return None;
        }
        Some(Rc::new(buf))
    }

    /// Populate an environment buffer with captures, arguments, and local slots.
    ///
    /// Shared by `build_call_env` (which uses `env_cache`),
    /// `pending_tail_call_to` (which uses `tail_call_env_cache`), and
    /// `build_callback_env`. The two caches can't alias — a tail call may occur
    /// inside a closure call that is still using `env_cache`.
    ///
    /// Capture cells and rest-list cons cells are allocated directly via
    /// `heap.alloc_in_region()`, into regions of their own (`env_value_region`):
    /// one per value, and one per cons or one per list for a rest list.
    ///
    /// Returns `false` if keyword argument collection fails (error set on fiber).
    ///
    /// `symbols` is the calling instance's display memo, carried only so that
    /// a rejected `&named` key is named rather than hashed in the error
    /// message (docs/impl/symbol.md).
    pub(super) fn populate_env(
        buf: &mut Vec<Value>,
        heap: &mut crate::value::fiberheap::FiberHeap,
        fiber: &mut crate::value::Fiber,
        closure: &crate::value::Closure,
        args: &[Value],
        refs: ArgRefs,
        symbols: Option<&crate::symbol::SymbolTable>,
    ) -> bool {
        buf.clear();
        let needed = closure.env_capacity();
        if buf.capacity() < needed {
            buf.reserve(needed - buf.len());
        }
        buf.extend(closure.env.iter().copied());

        match closure.template.arity() {
            crate::value::Arity::AtLeast(_) => {
                // Total fixed slots = num_params - 1 (rest slot is last param)
                let fixed_slots = closure.template.num_params() - 1;
                let provided_fixed = fixed_arg_count(closure, args);

                // Push args for fixed slots (required + optional)
                for (i, arg) in args[..provided_fixed].iter().enumerate() {
                    Self::push_param(buf, heap, closure, i, *arg, refs.mints(i));
                }
                // Fill missing optional slots with nil
                for i in provided_fixed..fixed_slots {
                    Self::push_param(buf, heap, closure, i, Value::NIL, false);
                }

                // Collect remaining args into rest slot
                let rest_args = if args.len() > provided_fixed {
                    &args[provided_fixed..]
                } else {
                    &[]
                };
                let collected = match closure.template.vararg_tag() {
                    crate::value::VarargTag::List => {
                        Self::args_to_list(rest_args, closure.template.rest_list_layout(), heap)
                    }
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
                // On a MOVE (`ArgRefs::Move`: an unspliced tail call or an FFI
                // callback) the caller's owning reference to each arg
                // transferred to us, and a
                // collected arg lands in `collected` — which took its own reference
                // when it stored the value — rather than in an env slot. So the
                // moved reference is surplus and is released here, whichever
                // collector took the value over (docs/impl/region/relocate.md).
                // Released after `collected` is built, so the collection's own
                // reference already stands.
                if refs == ArgRefs::Move {
                    Self::release_moved_rest_args(rest_args, args, heap);
                }
                // The rest-param's collected list/struct is built into its own
                // region here, not moved in by the caller — it is a borrow, not
                // an owned param, so no caller incref balances it: `false`.
                Self::push_param(buf, heap, closure, fixed_slots, collected, false);
            }
            crate::value::Arity::Range(_, max) => {
                // All slots are fixed (no rest param)
                // Push provided args
                for (i, arg) in args.iter().enumerate() {
                    Self::push_param(buf, heap, closure, i, *arg, refs.mints(i));
                }
                // Fill missing optional slots with nil
                for i in args.len()..max {
                    Self::push_param(buf, heap, closure, i, Value::NIL, false);
                }
            }
            crate::value::Arity::Exact(_) => {
                for (i, arg) in args.iter().enumerate() {
                    Self::push_param(buf, heap, closure, i, *arg, refs.mints(i));
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
        mint: bool,
    ) {
        if param_is_celled(closure, i) {
            // alloc_in_region → alloc_obj → incref_cross_region_refs handles
            // the cross-region incref for the wrapped value automatically. An
            // LBox/captured param is owned by its cell (not an owned local), so
            // it takes NO `CallArgument` incref — `mint` does not apply.
            // Each capture cell gets its own region (see `env_value_region`).
            let cell_region = env_value_region(heap);
            buf.push(crate::value::build::capture_cell(
                heap,
                val,
                crate::value::heap::CellOrigin::Runtime,
                cell_region,
            ));
        } else {
            // Non-captured fixed param. The callee owns it and releases it
            // value-based at its `decref_point` (`DecrefValueRegion` reading the
            // param slot). Where `mint` holds, hand it one owning reference here
            // so that release balances; use `result_region_of` to match the
            // region the callee's `DecrefValueRegion` will target (it sees
            // through a capture-cell wrapper identically). A moved argument
            // takes none: the caller's reference transfers. Immediates (region
            // `None`) no-op.
            if mint {
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
