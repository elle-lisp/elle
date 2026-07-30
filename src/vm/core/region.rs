use super::*;

impl VM {
    /// Resolve a static region slot to a **fresh** physical region for an
    /// allocation that has a matching compiler-emitted `DecrefRegion` at its
    /// decref_point (Pair, arrays, structs, closures, capture cells).
    ///
    /// Tofte-Talpin (docs/impl/region/merging.md): **every allocation execution gets
    /// its own physical region, period** — merging is the only thing that may
    /// collapse regions onto a shared slot, and a merged slot routes through
    /// `runtime_region_for_alloc_slot_maybe_merged` / `_for_merged_alloc_slot`
    /// instead of here. So this mints a fresh region on every call and stores
    /// `slot → physical` in the current activation frame *for the matching
    /// `DecrefRegion` to find* (`take_runtime_region_for_drop_slot` reads then clears
    /// it); it does **not** return a cached region.
    ///
    /// Returning the cached entry would let a *re-executed* slot reuse the prior
    /// region — commingling distinct values in one region (Rule 6) — and, when
    /// the slot's `DecrefRegion` is dead (a tail-moved alloc whose decref lands
    /// past the `TailCall`, so `take_runtime_region_for_drop_slot` never clears
    /// the slot), every iteration of a tail-recursive body would pile into one
    /// never-cleared region. A `while` loop is unaffected: its reachable
    /// `DecrefRegion` clears the slot each iteration, so the map was already
    /// empty at the next alloc. Overwriting a still-mapped entry is sound — that
    /// entry can only survive a previous alloc when its `DecrefRegion` was dead,
    /// so its region was already going to leak; orphaning the stale mapping
    /// changes nothing.
    #[inline]
    pub(crate) fn runtime_region_for_alloc_slot(
        &mut self,
        static_id: StaticRegion,
    ) -> RuntimeRegion {
        // Each alloc slot mints a fresh region per execution. (There is no
        // slot-0 case: the operand is a `StaticRegion`, always ≥ 1.)
        let phys = self.heap().new_runtime_region();
        let gen = self.heap().generation_raw(phys.get());
        self.fiber
            .activation_region_maps
            .last_mut()
            .expect("region frame stack must be non-empty")
            .insert(static_id.get(), MappedRegion::new(phys, gen));
        phys
    }
    /// Resolve a static region slot for an allocation, honoring builder-idiom
    /// **merging** (docs/impl/region/merging.md § Merging).
    ///
    /// For a slot NOT in `merged_slots` this is exactly
    /// [`Self::runtime_region_for_alloc_slot`] — mint a fresh physical region
    /// every execution and overwrite the activation mapping (the unmerged
    /// one-region-per-value baseline). For a MERGED slot — one shared by ≥2 alloc
    /// instructions after a merge — the FIRST member to execute (the child) finds
    /// the slot unmapped and mints `R`; a LATER member (the parent) finds the slot
    /// already mapped and **reuses** `R`, so every member lands in one region freed
    /// by the single `DecrefRegion`. Per-iteration uniqueness in loops is
    /// preserved because that `DecrefRegion` clears the slot
    /// (`take_runtime_region_for_drop_slot`) each iteration, so the next
    /// iteration's first member mints fresh.
    ///
    /// `merged_slots` is the current function's set (from its `Code`). It is empty
    /// unless a builder-idiom merge fired (a nested `%pair` literal seeding the
    /// merge), so this is byte-identical to the plain mint when no merge exists.
    #[inline]
    pub(crate) fn runtime_region_for_alloc_slot_maybe_merged(
        &mut self,
        static_id: StaticRegion,
        merged_slots: &rustc_hash::FxHashSet<u32>,
    ) -> RuntimeRegion {
        if merged_slots.contains(&static_id.get()) {
            return self.runtime_region_for_merged_alloc_slot(static_id);
        }
        self.runtime_region_for_alloc_slot(static_id)
    }
    /// Resolve a **merged** static slot for an allocation: reuse the physical region
    /// a prior member of the merge tree already minted for this activation (the
    /// parent reusing the child's region), else mint fresh (the first/child member).
    /// The JIT calls this directly (`elle_jit_resolve_alloc_region_merged`) for a
    /// slot it determined at compile time to be in `LirFunction.merged_slots`; the
    /// interpreter reaches it through the merged branch of
    /// `runtime_region_for_alloc_slot_maybe_merged`. The single `DecrefRegion` at the
    /// merged root's `decref_point` clears the slot each loop iteration
    /// (`take_runtime_region_for_drop_slot`), preserving per-iteration uniqueness.
    /// (docs/impl/region/merging.md § Merging, mint-or-reuse.)
    #[inline]
    pub(crate) fn runtime_region_for_merged_alloc_slot(
        &mut self,
        static_id: StaticRegion,
    ) -> RuntimeRegion {
        if let Some(m) = self
            .fiber
            .activation_region_maps
            .last()
            .and_then(|frame| frame.get(&static_id.get()))
        {
            return m.region;
        }
        self.runtime_region_for_alloc_slot(static_id)
    }
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
    #[inline]
    pub(crate) fn new_runtime_region_for_call_slot(
        &mut self,
        _static_id: StaticRegion,
    ) -> RuntimeRegion {
        self.heap().new_runtime_region()
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
        let alloc_region = self.new_runtime_region_for_call_slot(region_id);
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
                | RegionEffect::Opaque => {}
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
        // AND EXCEPT a fiber-carrier signal (`fiber/resume`/`fiber/abort`/
        // `fiber/propagate` returning its fiber ARGUMENT as the payload): the
        // signal handler replaces the carrier with the child's actual outcome
        // before any caller release runs, so a retain here would have no
        // consumer — one dangling retain per suspending resume, pinning every
        // parked-then-discarded fiber's region forever (docs/impl/region/owner.md
        // § "Park/unpark symmetry"; the `multi-resume`/`yield-discard` oracle
        // probes). A parked fiber's liveness holds are its holders' ordinary
        // counted references, never this retain.
        let is_fiber_carrier = matches!(
            crate::signals::dispatch::classify(bits, &value),
            crate::signals::dispatch::SignalAction::Resume
                | crate::signals::dispatch::SignalAction::Abort
                | crate::signals::dispatch::SignalAction::Propagate
        ) && value.as_fiber().is_some();
        if !def.moves_out && !is_fiber_carrier {
            crate::value::arena::pass_through_retain(
                unsafe { &mut *self.heap_ptr },
                value,
                alloc_region,
            );
        }
        (bits, value)
    }
    /// Resolve a static slot to the physical region it currently maps to in this
    /// activation, WITHOUT minting or clearing — the read a closure-cycle
    /// merged-arena tail-call deferred release needs (`TailCall::deferred_release_slot`,
    /// docs/impl/region/letrec.md § The letrec closure-cycle merge).
    ///
    /// Unlike [`Self::take_runtime_region_for_drop_slot`] this leaves the mapping
    /// in place: the arena is handed to the completing activation's
    /// `deferred_releases`, and its own scope-exit `DecrefRegion` is dead code past
    /// the frame-replacing tail call, so the mapping is never consumed by a drop.
    /// `None` when the slot is unmapped — the merged alloc did not execute in this
    /// activation (nothing to release).
    #[inline]
    pub(crate) fn runtime_region_for_release_slot(
        &self,
        static_id: StaticRegion,
    ) -> Option<RuntimeRegion> {
        self.fiber
            .activation_region_maps
            .last()
            .and_then(|frame| frame.get(&static_id.get()).map(|m| m.region))
    }
    /// Resolve a static region id for a `DecrefRegion` (the compiler's
    /// initial-reference drop at a value's decref_point). Returns the physical
    /// region and clears the slot; `None` if the allocation never executed
    /// in this activation (conditional alloc — a benign no-op).
    #[inline]
    pub(crate) fn take_runtime_region_for_drop_slot(
        &mut self,
        static_id: StaticRegion,
    ) -> Option<RuntimeRegion> {
        self.fiber
            .activation_region_maps
            .last_mut()
            .and_then(|frame| frame.remove(&static_id.get()))
            .map(|m| m.region)
    }
    /// Push a fresh region-remap frame on closure entry, with its (empty)
    /// parallel owner-node slot (docs/impl/region/owner.md § "Owner nodes").
    #[inline]
    pub(crate) fn push_activation_region_map(&mut self) {
        self.fiber
            .activation_region_maps
            .push(rustc_hash::FxHashMap::default());
        self.fiber.activation_owner_nodes.push(None);
        debug_assert_eq!(
            self.fiber.activation_region_maps.len(),
            self.fiber.activation_owner_nodes.len(),
            "the owner-node stack must parallel the region-remap stack (one slot \
             per activation frame)",
        );
    }
    /// Push a previously-captured region-remap frame, restoring an activation's
    /// static→physical mapping on resume (`resume_suspended`). The map is
    /// re-owned by the live stack so the resumed body's allocs/decrefs mutate
    /// it in place; the matching `pop_activation_region_map` discards it afterward.
    /// The parallel owner-node slot receives the node the suspend MOVED into the
    /// parked frame (`BytecodeFrame::activation_owner_node`) — `None` when the
    /// activation had not adopted — so the resumed body's normal completion
    /// frees it through the trampoline's clean break
    /// (docs/impl/region/owner.md § "Owner nodes").
    #[inline]
    pub(crate) fn restore_activation_region_map(
        &mut self,
        frame: rustc_hash::FxHashMap<u32, MappedRegion>,
        owner_node: Option<RuntimeRegion>,
    ) {
        self.fiber.activation_region_maps.push(frame);
        self.fiber.activation_owner_nodes.push(owner_node);
    }
    /// The current activation's owner node — the pages-less forest root
    /// `AdoptIntoActivation` adopts members into (docs/impl/region/owner.md
    /// § "Owner nodes — an activation as a forest root") — minted lazily on
    /// first use so an activation that adopts nothing pays nothing. The slot
    /// parallels the activation's region-remap frame
    /// (`Fiber::activation_owner_nodes`); the node is freed at the activation's
    /// normal completion via [`Self::release_activation_owner_node`].
    #[inline]
    pub(crate) fn activation_owner_node(&mut self) -> RuntimeRegion {
        if let Some(node) = self.fiber.activation_owner_nodes.last().copied().flatten() {
            return node;
        }
        let node = self.heap().new_runtime_region();
        *self
            .fiber
            .activation_owner_nodes
            .last_mut()
            .expect("owner-node stack must be non-empty (a base slot covers the top level)") =
            Some(node);
        node
    }
    /// Take the current activation's owner node, leaving the slot empty; `None`
    /// if this activation never adopted (the node is minted lazily).
    #[inline]
    pub(crate) fn take_activation_owner_node(&mut self) -> Option<RuntimeRegion> {
        self.fiber
            .activation_owner_nodes
            .last_mut()
            .and_then(|slot| slot.take())
    }
    /// Free the current activation's owner node, if one was minted: one
    /// tolerant decref takes the node's rc 1→0 and subtree-drops the node plus
    /// every member the activation adopted (interior cycles reclaim with the
    /// set; the Shared frontier cascades once). Runs at the activation's NORMAL
    /// completion on every tier — the interpreter trampoline's clean break and
    /// the compiled `Return` path (`elle_jit_release_activation_owner_node`) —
    /// never from an emitted drop instruction
    /// (docs/impl/region/owner.md § "Owner nodes").
    pub(crate) fn release_activation_owner_node(&mut self) {
        if let Some(node) = self.take_activation_owner_node() {
            self.heap().decref_region_if_present(node);
        }
    }
    /// The callee closure's runtime region, whose release the new activation
    /// takes over (`DeferredReleases::callee`).
    /// Called ONLY when the compiler flagged this tail call's callee as a
    /// per-call local closure whose release is dead past the `TailCall`
    /// (`lower_call`'s `defer_callee_release`, plumbed through `TailCallInfo`). The
    /// program-root-vs-local discrimination is made at compile time (the solver
    /// knows whether the closure's region dies at the call), so this is just the
    /// runtime region lookup. `None` for an immediate (no region). See
    /// `TailCallInfo` and `tests/elle/region-tailcall-closure-callee-leak.lisp`.
    #[inline]
    pub(crate) fn tail_callee_release_region(&self, func: Value) -> Option<RuntimeRegion> {
        crate::value::arena::region_of(unsafe { &mut *self.heap_ptr }, func)
    }
    /// Pop the current region-remap frame on normal closure return, with its
    /// parallel owner-node slot. The base frame (top level) is never popped.
    #[inline]
    pub(crate) fn pop_activation_region_map(&mut self) {
        if self.fiber.activation_region_maps.len() > 1 {
            self.fiber.activation_region_maps.pop();
            self.fiber.activation_owner_nodes.pop();
        }
        debug_assert_eq!(
            self.fiber.activation_region_maps.len(),
            self.fiber.activation_owner_nodes.len(),
            "the owner-node stack must parallel the region-remap stack (one slot \
             per activation frame)",
        );
    }
    pub(crate) fn ffi(&self) -> &FFISubsystem {
        &self.ffi
    }
    pub(crate) fn ffi_mut(&mut self) -> &mut FFISubsystem {
        &mut self.ffi
    }
}

// ── The declaration oracle (docs/impl/region/effects.md "Native region effects") ──
//
// A `RegionEffect` declaration is a soundness claim; these tests pin that
// a primitive whose result contradicts its declaration panics
// deterministically in debug builds — a mis-declared primitive cannot
// survive the suite — and that truthful declarations pass.

#[cfg(test)]
mod tests;
