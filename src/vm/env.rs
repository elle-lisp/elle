//! Closure environment building.
//!
//! Handles constructing the `Vec<Value>` environment that a closure receives
//! at call time: captured variables, positional arguments (with optional lbox
//! wrapping), rest-parameter collection (list, struct, or strict struct), and
//! local variable slots.
//!
//! Entry points:
//! - `build_closure_env`: reuses `env_cache` to avoid a fresh allocation per call
//! - `populate_env`: fills a caller-supplied buffer; shared by `build_closure_env`
//!   and `tail_call_inner` (which uses `tail_call_env_cache`)

use crate::hir::region::RuntimeRegion;
use crate::value::Value;
use std::rc::Rc;

use super::core::VM;

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
        if !Self::populate_env(
            &mut self.env_cache,
            unsafe { &mut *self.heap_ptr },
            &mut self.fiber,
            closure,
            args,
            true,
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
        if !Self::populate_env(
            &mut self.tail_call_env_cache,
            unsafe { &mut *self.heap_ptr },
            &mut self.fiber,
            closure,
            args,
            own_params,
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
        let ok = Self::populate_env(
            &mut buf,
            unsafe { &mut *self.heap_ptr },
            &mut self.fiber,
            closure,
            args,
            false,
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
    pub(super) fn populate_env(
        buf: &mut Vec<Value>,
        heap: &mut crate::value::fiberheap::FiberHeap,
        fiber: &mut crate::value::Fiber,
        closure: &crate::value::Closure,
        args: &[Value],
        own_params: bool,
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
                        match Self::collect_struct_in_own_region(fiber, heap, rest_args, None) {
                            Some(v) => v,
                            None => return false,
                        }
                    }
                    crate::value::VarargTag::StrictStruct => {
                        let keys = closure.template.strict_keys();
                        match Self::collect_struct_in_own_region(fiber, heap, rest_args, Some(keys))
                        {
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
        // LocalCell(NIL). Non-cell locals get bare NIL — they use stack slots via
        // StoreLocal/LoadLocal and the env slot is never accessed. The
        // `capture_locals_mask` names every local precisely at any index, so an
        // uncaptured local — even one beyond slot 63 — gets a bare NIL and never
        // a dead, leaked cell.
        let num_locally_defined = closure
            .template
            .num_locals()
            .saturating_sub(closure.template.num_params());
        for i in 0..num_locally_defined {
            if closure.template.capture_locals_mask().is_set(i) {
                use crate::value::heap::HeapObject;
                use std::cell::RefCell;
                use std::rc::Rc;
                let obj = HeapObject::CaptureCell {
                    cell: Rc::new(RefCell::new(Value::NIL)),
                    traits: Value::NIL,
                };
                // Each captured-local cell gets its own region (see `env_value_region`).
                let cell_region = env_value_region(heap);
                buf.push(heap.alloc_in_region(obj, cell_region));
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
            use crate::value::heap::HeapObject;
            use std::cell::RefCell;
            use std::rc::Rc;
            // alloc_in_region → alloc_obj → incref_cross_region_refs handles
            // the cross-region incref for the wrapped value automatically. An
            // LBox/captured param is owned by its cell (not an owned local), so
            // it takes NO `CallArgument` incref — `own_params` does not apply.
            let obj = HeapObject::CaptureCell {
                cell: Rc::new(RefCell::new(val)),
                traits: Value::NIL,
            };
            // Each capture cell gets its own region (see `env_value_region`).
            let cell_region = env_value_region(heap);
            buf.push(heap.alloc_in_region(obj, cell_region));
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

    /// Collect values into an Elle list (pair chain terminated by EMPTY_LIST).
    ///
    /// One region per cons, with ownership transfer down the chain so a single
    /// release of the HEAD cascade-frees the whole list (every value its own
    /// region — see `env_value_region`). Built tail→head: each new cons points
    /// at the prior head via its `rest`, so `alloc_in_region`'s cross-region
    /// scan increfs the prior head's region (rc 1→2). We then drop our minting
    /// reference on that prior head (rc 2→1), leaving it owned solely by the new
    /// cons's edge. Only the final head keeps its minting rc=1 — the one owning
    /// reference the owned-params move carries into the callee (or releases).
    /// Freeing the head then cascades head→cons₂→…→tail, each rc 1→0.
    fn args_to_list(args: &[Value], heap: &mut crate::value::fiberheap::FiberHeap) -> Value {
        use crate::value::heap::{HeapObject, HeapTag, Pair};
        let mut list = Value::EMPTY_LIST;
        for arg in args.iter().rev() {
            let cons_region = env_value_region(heap);
            let traits = crate::primitives::traitregistry::default_traits_for(heap, HeapTag::Pair);
            let obj = HeapObject::Pair(Pair {
                first: *arg,
                rest: list,
                traits,
            });
            // `alloc_in_region` → `alloc_obj` increfs every cross-region ref in
            // the object: the prior head (this cons's `rest`) and any heap
            // `first`. Both are balanced by the free-time cascade.
            let new_cons = heap.alloc_in_region(obj, cons_region);
            // Drop the minting ref on the prior head now that `new_cons` pins it
            // via `rest`. Guarded on a genuine cross-region edge (the first
            // cons's `rest` is EMPTY_LIST — no region).
            if let Some(prior) = crate::value::arena::region_of(heap, list) {
                if prior != cons_region {
                    heap.decref_region(prior);
                }
            }
            list = new_cons;
        }
        list
    }

    /// Release the moved-in reference of each collected arg on a MOVE call
    /// (`own_params = false`), for every collector kind — `&`, `&keys`, `&named`
    /// alike (docs/impl/region/mechanism.md § "A collector parameter takes the
    /// moved reference over itself"; rate pinned by
    /// `tests/elle/region-collector-arg-move.lisp`). Released ONLY for a value that
    /// appears exactly once across ALL arg positions (`all_args`) — an aliased value
    /// shares one transferred reference a fixed slot / earlier member already
    /// consumes, so a second release would over-free (leak-safe: never mis-free).
    ///
    /// A keyword key in `rest_args` is an immediate, so `region_of` reports no
    /// region for it and the `&keys`/`&named` keys are skipped without a special
    /// case — only the values they name are released.
    ///
    /// The occurrence counts come from ONE pass over `all_args`, so the whole
    /// step is linear in the argument count. Counting per rest arg instead —
    /// rescanning `all_args` for each — is quadratic, and every comparison is a
    /// `region_of` page-header walk, so a large `(apply f xs)` in tail position
    /// pays it in full (`tests/elle/apply-tail-linear.lisp`,
    /// docs/regions/performance.md § "Passing arguments costs one pass over
    /// them").
    ///
    /// Counting first and releasing second gives the same answers as
    /// interleaving them. A release here can only FREE regions (its own and
    /// whatever its cascade reaches), and a free leaves the page's stamped
    /// region id alone — only recycling a page into a fresh region changes it,
    /// and nothing in this function allocates a region. So every value's
    /// `region_of` reads the same id throughout, and a count taken up front
    /// equals one taken part-way through.
    fn release_moved_rest_args(
        rest_args: &[Value],
        all_args: &[Value],
        heap: &mut crate::value::fiberheap::FiberHeap,
    ) {
        let mut occurrences: rustc_hash::FxHashMap<RuntimeRegion, usize> =
            rustc_hash::FxHashMap::default();
        for arg in all_args {
            if let Some(r) = crate::value::arena::region_of(heap, *arg) {
                *occurrences.entry(r).or_insert(0) += 1;
            }
        }
        for arg in rest_args {
            let Some(arg_region) = crate::value::arena::region_of(heap, *arg) else {
                continue; // an immediate carries no region
            };
            if occurrences.get(&arg_region) == Some(&1) {
                heap.decref_region(arg_region);
            }
        }
    }

    /// Collect alternating keyword args into a struct in its OWN fresh region.
    ///
    /// This mints a per-value region (see `env_value_region`) and routes
    /// `args_to_struct_static`'s construction into it, so the collected
    /// `&keys`/`&named` struct is releasable on its own (region-env-leak.lisp
    /// witness (e) pins this). Returns `None` (with the error already set on the
    /// fiber) on bad keyword args, releasing the now-unused region.
    fn collect_struct_in_own_region(
        fiber: &mut crate::value::Fiber,
        heap: &mut crate::value::fiberheap::FiberHeap,
        args: &[Value],
        valid_keys: Option<crate::value::closure::StrKeys<'_>>,
    ) -> Option<Value> {
        let sr = env_value_region(heap);
        // Build the struct INSIDE `sr` (the per-value region the result lives
        // in on success). On failure, `args_to_struct_static` returns the error
        // *description* WITHOUT allocating — the error struct must NOT be born
        // in `sr`, because we free `sr` below and the error escapes into
        // `fiber.signal`, read later via `fiber/value`/propagation. An error
        // born in `sr` would point into a freed (and recycled) region — a stale
        // deref the region-generation guard catches under `protect`/`fiber`
        // (docs/impl/region/generations.md). The error is instead set AFTER the
        // alloc-region bracket closes, so it is born in its own durable region
        // (`heap.new_runtime_region()`) — like every other param-binding error
        // (e.g. `check_arity`) — which survives until the fiber dies.
        let built = Self::args_to_struct_static(heap, args, valid_keys, sr);
        match built {
            Ok(v) => Some(v),
            Err((kind, msg)) => {
                // `sr` may hold partial allocations from a struct that got far
                // enough to insert before failing; free them. `decref_region_if_present`
                // is a tolerant no-op when the error fired before any alloc.
                heap.decref_region_if_present(sr);
                // Born in a fresh region of its own minted from `heap` (Rule 3),
                // durable and not the just-freed `sr`; freed value-based once the
                // fiber's terminal signal is consumed.
                let err_region = heap.new_runtime_region();
                fiber.set_error_in(heap, kind, msg, err_region);
                None
            }
        }
    }

    /// Collect alternating key-value args into an immutable struct.
    ///
    /// On success the struct is allocated into the explicit `region` (the
    /// caller's per-value `sr`). On failure returns `Err((kind, message))`
    /// WITHOUT allocating an error value — the caller sets the error outside that
    /// region so the error struct is not stranded in a region about to be freed
    /// (see `collect_struct_in_own_region`). If `valid_keys` is `Some`, fails on
    /// unknown keys (strict `&named` mode).
    fn args_to_struct_static(
        heap: &mut crate::value::fiberheap::FiberHeap,
        args: &[Value],
        valid_keys: Option<crate::value::closure::StrKeys<'_>>,
        region: crate::hir::region::RuntimeRegion,
    ) -> Result<Value, (&'static str, String)> {
        use crate::value::types::TableKey;
        use std::collections::BTreeMap;

        if args.is_empty() {
            return Ok(crate::value::build::struct_from(
                heap,
                BTreeMap::new(),
                region,
            ));
        }

        if !args.len().is_multiple_of(2) {
            return Err((
                "argument-error",
                format!("odd number of keyword arguments ({} args)", args.len()),
            ));
        }

        let mut map = BTreeMap::new();
        for i in (0..args.len()).step_by(2) {
            let key = match TableKey::from_value(&args[i]) {
                Some(TableKey::Keyword(hash)) => hash,
                _ => {
                    return Err((
                        "argument-error",
                        format!(
                            "keyword argument key must be a keyword, got {}",
                            args[i].type_name()
                        ),
                    ));
                }
            };
            // Error-message spelling: this static path has no memo, so an
            // off-vocabulary key falls back to the unreadable form.
            let spell = || {
                crate::value::keyword::resolve_keyword_name(None, key)
                    .map(|n| format!(":{}", n))
                    .unwrap_or_else(|| format!("#<keyword:{:#x}>", key))
            };

            // Strict validation for &named — parameter spellings are known, so
            // matching is by hash and the error lists the valid spellings.
            if let Some(valid) = valid_keys {
                if !valid
                    .iter()
                    .any(|v| crate::value::keyword::keyword_hash(v) == key)
                {
                    return Err((
                        "argument-error",
                        format!(
                            "unknown named parameter {}, valid parameters are: {}",
                            spell(),
                            valid
                                .iter()
                                .map(|v| format!(":{}", v))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    ));
                }
            }

            let table_key = TableKey::Keyword(key);
            if map.contains_key(&table_key) {
                return Err((
                    "argument-error",
                    format!("duplicate keyword argument {}", spell()),
                ));
            }
            map.insert(table_key, args[i + 1]);
        }
        Ok(crate::value::build::struct_from(heap, map, region))
    }
}

#[cfg(test)]
mod callback_env_tests;
