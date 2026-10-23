// audited: 2026-10-22
//! Dispatching a native call: the result region it mints, the pass-through
//! retain it hands the caller, and the declaration oracle over both.
//!
//! docs/impl/region/effects.md
//! docs/impl/region/ctx.md

use super::*;
use crate::value::fiberheap::regionstore::RegionMint;

impl VM {
    /// Resolve a static region slot for a *call result* (a native's fresh
    /// result region, or a closure-call setup region). These are freed
    /// value-based by `DecrefValueRegion` (no `DecrefRegion` to clear a
    /// cache), so each execution mints its own fresh physical region —
    /// never cached.
    ///
    /// `static_id` picks no region, because every call result gets its own fresh
    /// one; only the arena trace names it. The slot is still the solver's
    /// per-call result-region *assignment*, carried end to end (emitter →
    /// bytecode → both dispatch tiers). The `StaticRegion` newtype keeps it from
    /// being compared with a runtime id (`dispatch_native_call`'s doc).
    ///
    /// The mint is **tracked**. The region is minted before the callee runs,
    /// because the callee may allocate its result into it. A callee that
    /// returns an immediate or a value borrowed from an argument allocates
    /// nothing, and the receipt lets the dispatcher return that id to the free
    /// list (docs/impl/region/model.md). A call that does allocate leaves the id
    /// live, and the recycle does nothing.
    #[inline]
    pub(crate) fn new_runtime_region_for_call_slot(
        &mut self,
        static_id: StaticRegion,
    ) -> RegionMint {
        let mint = self.heap().new_runtime_region_tracked();
        self.note_region_mint(mint.region(), "call result", Some(static_id));
        mint
    }
    /// Close out a call-result mint: return its id to the free list unless the call
    /// materialized the region (docs/impl/region/model.md). The shared tail of both
    /// call dispatchers.
    #[inline]
    pub(crate) fn release_unused_call_region(&mut self, mint: RegionMint) {
        self.heap().recycle_unmaterialized_region(mint);
    }
    /// Dispatch a native primitive call with per-execution region routing and
    /// the "pass-through retain", shared verbatim by the interpreter
    /// (`call_inner` / `tail_call_inner`) and the JIT (`elle_jit_call` /
    /// `elle_jit_tail_call`).
    ///
    /// Mints this call's fresh result region, and runs the primitive with that
    /// region as its `NativeCtx` alloc target, so fresh allocations land in it.
    /// When the result lives in a *different* region than this call allocated (a
    /// pass-through native such as `first`/`rest`/`get`), the dispatch hands the
    /// caller exactly one owning reference to the result's runtime region. That
    /// retain balances the caller's `DecrefValueRegion` at the result
    /// binding's decref_point. A fresh result lives in `alloc_region` and is
    /// skipped — its alloc incref (rc=1) already is the handoff.
    ///
    /// Both engines MUST go through this, so the retain/release accounting is
    /// identical whichever tier runs the caller. A JIT caller that skipped the
    /// pass-through retain would under-count the result region, and free it
    /// while a freshly built cons still references it (UAF).
    ///
    /// The skip compares two *runtime* regions — `result_region` against the
    /// `alloc_region` this call just minted. Both sides are `RuntimeRegion`. The
    /// `StaticRegion` newtype on `region_id` keeps a static slot from being
    /// compared here, where it would never match a freshly minted runtime id
    /// and would leak one region per native call
    /// (`tests/impl/region-native-result-leak.lisp`).
    pub(crate) fn dispatch_native_call(
        &mut self,
        def: &'static crate::primitives::def::PrimitiveDef,
        args: &[Value],
        region_id: StaticRegion,
    ) -> (SignalBits, Value) {
        let mint = self.new_runtime_region_for_call_slot(region_id);
        let alloc_region = mint.region();
        // The question a `SIG_QUERY` asked, once the dispatch has answered it.
        let mut question = None;
        let (bits, value) = {
            // The native-call capability: this call's fresh result region, the
            // VM's heap, and the driving VM itself, so the primitive can reach
            // VM state / re-enter through `ctx.vm()` (docs/impl/region/ctx.md).
            let vm_ptr: *mut VM = self as *mut VM;
            let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
                alloc_region,
                unsafe { &mut *self.heap_ptr },
                vm_ptr,
            );
            let (bits, value) =
                if std::ptr::fn_addr_eq(def.func, crate::plugin_api::PLUGIN_SENTINEL) {
                    crate::plugin_api::call_plugin(def, &mut ctx, args, alloc_region)
                } else {
                    (def.func)(&mut ctx, args)
                };
            // A `SIG_QUERY` answer (`vm/config`, `arena/stats`, `doc`, …) is the
            // call's *result*, and the VM builds it here (`Value::set`,
            // `Value::struct_from`, …). Build it through THIS call's `ctx` so the
            // answer is born in `alloc_region`, the call's own region, like any
            // native result (Rule 3: values are born in their solver-assigned
            // region; docs/impl/region/rules.md). The escape/skip accounting below
            // then treats it exactly as a native result: a fresh answer lives in
            // `alloc_region` (skip), a pass-through answer (`fiber/self`) lives
            // elsewhere and is retained. An answer built in any other region is
            // fatal in a spawned worker. The worker's result region also holds the
            // live reconstructed closure and its captures. This result's
            // `DecrefValueRegion` would drop that region to 0, and free them out
            // from under execution (tests/impl/spawn-config-region.lisp).
            if crate::signals::dispatch::classify(bits, &value)
                == crate::signals::dispatch::SignalAction::Query
            {
                question = Some(value);
                self.dispatch_query(&mut ctx, value)
            } else {
                (bits, value)
            }
        };
        // The declaration oracle (docs/impl/region/effects.md):
        // in debug builds, check the declared RegionEffect's result-side
        // claim against where the result actually lives, on every normally-
        // completing native call. A mis-declared primitive panics
        // deterministically, naming itself — it cannot survive the suite.
        // Signal-carrying returns (error/yield payloads) are exempt; their
        // payloads ride the signal machinery's own accounting. The store
        // side of `Stores`/`Mixed` is unobservable here (that is the
        // mutable-store funnel's and guardfree's territory).
        #[cfg(debug_assertions)]
        if crate::signals::dispatch::classify(bits, &value)
            == crate::signals::dispatch::SignalAction::Ok
        {
            let result_region =
                crate::value::arena::region_of(unsafe { &mut *self.heap_ptr }, value);
            // `fresh`: the result lives in the region this call just minted.
            let fresh = result_region == Some(alloc_region);
            use crate::primitives::def::RegionEffect;
            match def.effect {
                RegionEffect::Immediate => assert!(
                    result_region.is_none(),
                    "primitive `{}` declares RegionEffect::Immediate but returned \
                     a heap value in {:?} (declaration oracle; docs/impl/region/effects.md)",
                    def.name,
                    result_region,
                ),
                RegionEffect::Fresh | RegionEffect::Stores { .. } | RegionEffect::Sends { .. } => {
                    assert!(
                        result_region.is_none() || fresh,
                        "primitive `{}` declares RegionEffect::{:?} but returned a \
                     non-fresh heap value in {:?}, not this call's own region \
                     {:?} (declaration oracle; docs/impl/region/effects.md)",
                        def.name,
                        def.effect,
                        result_region,
                        alloc_region,
                    )
                }
                RegionEffect::PassThrough => assert!(
                    !fresh,
                    "primitive `{}` declares RegionEffect::PassThrough but \
                     returned a value freshly allocated in this call's own \
                     region {:?} (declaration oracle; docs/impl/region/effects.md)",
                    def.name, alloc_region,
                ),
                RegionEffect::Funnel
                | RegionEffect::Mixed
                | RegionEffect::Unknown
                | RegionEffect::Opaque
                | RegionEffect::Delivers { .. } => {}
            }
        }
        // Skip the escape incref when the result is fresh in this call's own
        // region: the caller's `DecrefValueRegion` already balances that lone
        // owning ref. Incref only a genuine pass-through (the result lives in a
        // region this call did not allocate — `first`/`rest`/`get`, or an
        // immediate), so the caller's decref balances the incref instead of
        // freeing a region owned elsewhere. Shared with the intrinsic opcode
        // handlers (`%put`/`%del`/`%string-push`) via `pass_through_retain`.
        //
        // EXCEPT a `moves_out` native (`%pop`/`pop`): its result is an element
        // REMOVED from a container arg, and the body already took the pass-through
        // retain in place. It must take the retain BEFORE it releases the
        // container's reference, or a sole-owned element would be freed under the
        // returned Value (`arena::pop_with_decref`). Retaining again here would
        // double-count, one leaked region per op (the `raw-pop` probe in
        // tests/impl/probe/store.lisp).
        //
        // AND EXCEPT a `result_minted` native (the module loaders, the
        // `compile/*-module` test loaders). Its result was produced by compiled
        // code run on this VM. So it left that code through the return convention,
        // already carrying the one owed reference the caller's release consumes.
        // On any path that ran no thunk, the declarant supplies that reference
        // itself (`import/load-plugin`'s retain). Retaining again here is the same
        // double-count, one stranded region graph per call (the `import-result`
        // probe in tests/impl/probe/native.lisp).
        //
        // AND ONLY for a value the native returns as a RESULT. The retain funds
        // the caller's `DecrefValueRegion` on the call result, and that release
        // targets the call result. So a value the native returns as a SIGNAL
        // PAYLOAD has no consumer for the retain. The signal machinery accounts
        // for a payload itself, on the path each payload takes:
        //
        // - A fiber carrier (`fiber/resume`/`fiber/abort`/`fiber/propagate`
        //   returning its fiber ARGUMENT) is replaced by the child's outcome
        //   before any caller release runs.
        // - A suspending payload rides `fiber.signal` under the
        //   `SuspendEscape`/`EmitEscape` retain, and is released on the resume
        //   path.
        // - An error or halt payload is read through the signal, never through
        //   the caller's result slot, which the handler stamps `nil`.
        //
        // Retaining any of them here would strand one region per call. That is
        // a parked-then-discarded fiber's whole region graph
        // (docs/impl/region/park.md; the `multi-resume`/`yield-discard` probes
        // in tests/impl/probe/concurrent.lisp), or the emitted value of every
        // `fiber/emit` (tests/impl/region-fiber-install-clique-leak.lisp). The
        // declaration oracle above makes the same exemption for a
        // signal-carrying return; this is that exemption on the accounting side.
        let is_result = crate::signals::dispatch::classify(bits, &value)
            == crate::signals::dispatch::SignalAction::Ok;
        if !def.moves_out && !def.result_minted && is_result {
            crate::value::arena::pass_through_retain(
                unsafe { &mut *self.heap_ptr },
                value,
                alloc_region,
            );
        }
        // A query's question is born in this call's region and read by the
        // dispatch alone. A fresh answer shares the region, and the caller's
        // release of the answer takes the question with it. An answer that
        // lives elsewhere leaves the region's birth reference with no consumer,
        // so it is released here (docs/impl/region/ctx.md).
        if let Some(question) = question {
            let heap = unsafe { &mut *self.heap_ptr };
            let asked_here = crate::value::arena::region_of(heap, question) == Some(alloc_region);
            let answered_here = crate::value::arena::region_of(heap, value) == Some(alloc_region);
            if asked_here && !answered_here {
                heap.decref_region(alloc_region);
            }
        }
        // A primitive that returned an immediate or a borrowed value never
        // allocated into this call's region, so its id names nothing and goes
        // back to the free list. Runs after the pass-through retain, which reads
        // `alloc_region` to decide whether the result is fresh.
        self.release_unused_call_region(mint);
        (bits, value)
    }
}

#[cfg(test)]
mod tests;
