// audited: 2026-09-19
//! Rest-parameter collection: the `&` list, the `&keys`/`&named` structs,
//! and the release a collector takes over from a moved argument.

use crate::value::Value;

use super::super::core::VM;
use super::env_value_region;

impl VM {
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
    pub(super) fn args_to_list(
        args: &[Value],
        heap: &mut crate::value::fiberheap::FiberHeap,
    ) -> Value {
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
    pub(super) fn release_moved_rest_args(
        rest_args: &[Value],
        all_args: &[Value],
        heap: &mut crate::value::fiberheap::FiberHeap,
    ) {
        let mut occurrences: rustc_hash::FxHashMap<crate::hir::region::RuntimeRegion, usize> =
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
    pub(super) fn collect_struct_in_own_region(
        fiber: &mut crate::value::Fiber,
        heap: &mut crate::value::fiberheap::FiberHeap,
        args: &[Value],
        valid_keys: Option<crate::value::closure::StrKeys<'_>>,
        symbols: Option<&crate::symbol::SymbolTable>,
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
        let built = Self::args_to_struct_static(heap, args, valid_keys, symbols, sr);
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
        symbols: Option<&crate::symbol::SymbolTable>,
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
            // Error-message spelling. The rejected key is one the caller
            // wrote, so it is the instance memo that holds its name — a
            // message built without one names a hash the author has to
            // decode (docs/impl/symbol.md § "Reading a name, and not reading
            // one").
            let spell = || {
                crate::value::keyword::resolve_keyword_name(symbols, key)
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
