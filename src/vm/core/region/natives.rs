// audited: 2026-09-19
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
    /// `_static_id` is intentionally unused under unoptimized Tofte-Talpin
    /// (every call result gets its own fresh region, period). It is NOT dead
    /// code: the slot is the solver's per-call result-region *assignment*,
    /// carried end-to-end (emitter → bytecode → both dispatch tiers). Region
    /// **merging** is exactly the feature that makes this
    /// function resolve `_static_id` to a possibly-*shared* physical region
    /// instead of always minting fresh. Keep it wired; the `StaticRegion`
    /// newtype already guards the static-vs-runtime confusion bug
    /// (`dispatch_native_call`'s doc). Do not "scrub" it as vestigial.
    ///
    /// The mint is **tracked**: the region is minted before the callee runs,
    /// because the callee may allocate its result into it, and a callee that
    /// returns an immediate or a value borrowed from an argument allocates
    /// nothing. The receipt lets the dispatcher return that id to the free list
    /// (docs/impl/region/model.md § "Physical id recycling"); a call that does
    /// allocate leaves the id live and the recycle no-ops.
    #[inline]
    pub(crate) fn new_runtime_region_for_call_slot(
        &mut self,
        _static_id: StaticRegion,
    ) -> RegionMint {
        let mint = self.heap().new_runtime_region_tracked();
        self.note_region_mint(mint.region(), "call result", Some(_static_id));
        mint
    }
    /// Close out a call-result mint: return its id to the free list unless the
    /// call materialized the region (docs/impl/region/model.md § "Physical id
    /// recycling"). The shared tail of both call dispatchers.
    #[inline]
    pub(crate) fn release_unused_call_region(&mut self, mint: RegionMint) {
        self.heap().recycle_unmaterialized_region(mint);
    }
    /// Dispatch a native primitive call with per-execution region routing and
    /// the "pass-through retain", shared verbatim by the interpreter
    /// (`call_inner` / `tail_call_inner`) and the JIT (`elle_jit_call` /
    /// `elle_jit_tail_call`).
    ///
    /// Mints this call's fresh result region, runs the primitive with that
    /// region as its `NativeCtx` alloc target (so fresh allocations land in it), then
    /// hands the caller exactly one owning reference to the result's runtime
    /// region whenever the result lives in a *different* region than this call
    /// allocated (a pass-through native such as `first`/`rest`/`get`). That
    /// retain balances the caller's `DecrefValueRegion` at the result
    /// binding's decref_point. A fresh result lives in `alloc_region` and is
    /// skipped — its alloc incref (rc=1) already is the handoff.
    ///
    /// Both engines MUST go through this so the retain/release accounting is
    /// identical regardless of which tier runs the caller; a JIT caller that
    /// skipped the pass-through retain would under-count the result region and
    /// free it while a freshly built cons still references it (UAF).
    ///
    /// The skip compares two *runtime* regions — `result_region` against the
    /// `alloc_region` this call just minted. Both sides are `RuntimeRegion`; the
    /// `StaticRegion` newtype on `region_id` keeps a static slot from being
    /// compared here, where it would never match a freshly minted runtime id and
    /// would leak one region per native call
    /// (`tests/elle/region-native-result-leak.lisp`).
    pub(crate) fn dispatch_native_call(
        &mut self,
        def: &'static crate::primitives::def::PrimitiveDef,
        args: &[Value],
        region_id: StaticRegion,
    ) -> (SignalBits, Value) {
        let mint = self.new_runtime_region_for_call_slot(region_id);
        let alloc_region = mint.region();
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
            // elsewhere and is retained. Building it in any region but this call's
            // own is fatal in a spawned worker, whose result region also holds the
            // live reconstructed closure + captures: they would be freed out from
            // under execution when this result's `DecrefValueRegion` drops that
            // region to 0 (tests/elle/spawn-config-region.lisp).
            if crate::signals::dispatch::classify(bits, &value)
                == crate::signals::dispatch::SignalAction::Query
            {
                // Build the SIG_QUERY answer through THIS call's ctx, so it is
                // born in `alloc_region` like any native result (the pass-through
                // accounting below then treats it identically).
                self.dispatch_query(&mut ctx, value)
            } else {
                (bits, value)
            }
        };
        // The declaration oracle (docs/impl/region/effects.md "Native region effects"):
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
                     a heap value in {:?} (declaration oracle; docs/impl/region/effects.md \
                     \"Native region effects\")",
                    def.name,
                    result_region,
                ),
                RegionEffect::Fresh | RegionEffect::Stores { .. } | RegionEffect::Sends { .. } => {
                    assert!(
                        result_region.is_none() || fresh,
                        "primitive `{}` declares RegionEffect::{:?} but returned a \
                     non-fresh heap value in {:?}, not this call's own region \
                     {:?} (declaration oracle; docs/impl/region/effects.md \"Native region \
                     effects\")",
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
                     region {:?} (declaration oracle; docs/impl/region/effects.md \"Native \
                     region effects\")",
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
        // retain in-place — necessarily BEFORE releasing the container's reference,
        // or a sole-owned element would be freed under the returned Value
        // (`arena::pop_with_decref`). Retaining again here would double-count (one
        // leaked region per op — the `raw-pop` oracle probe).
        // AND EXCEPT a `result_minted` native (`import`, the `compile/*-module`
        // test loaders): its result was produced by compiled code run on this VM,
        // so it left that code through the return convention already carrying the
        // one owed reference the caller's release consumes — and the declarant
        // supplies that reference itself on any path that did not run a thunk
        // (`import`'s plugin-cache retain). Retaining again here is the same
        // double-count — one stranded region graph per call (the
        // `import-result` oracle probe).
        // AND ONLY for a value the native returns as a RESULT. The retain funds
        // the caller's `DecrefValueRegion` on the call result, and that release
        // targets the call result — so a value the native returns as a SIGNAL
        // PAYLOAD has no consumer for it. The signal machinery accounts for a
        // payload itself, on the path each payload actually takes: a fiber
        // carrier (`fiber/resume`/`fiber/abort`/`fiber/propagate` returning its
        // fiber ARGUMENT) is replaced by the child's outcome before any caller
        // release runs; a suspending payload rides `fiber.signal` under the
        // `SuspendEscape`/`EmitEscape` retain and is released on the resume path;
        // an error or halt payload is read through the signal, never through the
        // caller's result slot, which the handler stamps `nil`. Retaining any of
        // them here strands one region per call — a parked-then-discarded fiber's
        // whole region graph (docs/impl/region/park.md;
        // the `multi-resume`/`yield-discard` oracle probes), or the emitted value
        // of every `fiber/emit` (`region-fiber-install-clique-leak.lisp`). This is
        // the same exemption the declaration oracle above makes for a
        // signal-carrying return, stated on the accounting side.
        let is_result = crate::signals::dispatch::classify(bits, &value)
            == crate::signals::dispatch::SignalAction::Ok;
        if !def.moves_out && !def.result_minted && is_result {
            crate::value::arena::pass_through_retain(
                unsafe { &mut *self.heap_ptr },
                value,
                alloc_region,
            );
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
