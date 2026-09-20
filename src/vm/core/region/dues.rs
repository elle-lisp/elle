// audited: 2026-09-19
//! The per-activation region-remap frame and the dues slot beside it: what
//! an activation owes the region system, and who discharges it.
//!
//! docs/impl/region/owner.md

use super::*;
use crate::value::fiber::ActivationDues;

impl VM {
    /// Push a fresh region-remap frame on closure entry, with its (empty)
    /// parallel dues slot (docs/impl/region/owner.md § "Owner nodes").
    #[inline]
    pub(crate) fn push_activation_region_map(&mut self) {
        self.fiber
            .activation_region_maps
            .push(rustc_hash::FxHashMap::default());
        self.fiber.activation_dues.push(ActivationDues::default());
        debug_assert_eq!(
            self.fiber.activation_region_maps.len(),
            self.fiber.activation_dues.len(),
            "the dues stack must parallel the region-remap stack (one slot \
             per activation frame)",
        );
    }
    /// Push a previously-captured region-remap frame, restoring an activation's
    /// static→physical mapping on resume (`resume_suspended`). The map is
    /// re-owned by the live stack so the resumed body's allocs/decrefs mutate
    /// it in place; the matching `pop_activation_region_map` discards it afterward.
    /// The parallel dues slot receives the record the suspend MOVED into the
    /// parked frame (`BytecodeFrame::activation_dues`) — the owner node and the
    /// releases the activation took over from frame-replacing tail calls — so
    /// the resumed body's normal completion discharges both through the
    /// trampoline's clean break (docs/impl/region/owner.md § "Owner nodes",
    /// § "A deferred tail-call release has the node's life").
    #[inline]
    pub(crate) fn restore_activation_region_map(
        &mut self,
        frame: rustc_hash::FxHashMap<u32, MappedRegion>,
        dues: ActivationDues,
    ) {
        self.fiber.activation_region_maps.push(frame);
        self.fiber.activation_dues.push(dues);
    }
    /// The current activation's owner node — the pages-less forest root
    /// `AdoptIntoActivation` adopts members into (docs/impl/region/owner.md
    /// § "Owner nodes — an activation as a forest root") — minted lazily on
    /// first use so an activation that adopts nothing pays nothing. The slot
    /// parallels the activation's region-remap frame (`Fiber::activation_dues`);
    /// the node is freed at the activation's normal completion via
    /// [`Self::release_activation_dues`].
    #[inline]
    pub(crate) fn activation_owner_node(&mut self) -> RuntimeRegion {
        if let Some(node) = self.activation_dues().owner_node {
            return node;
        }
        let node = self.heap().new_runtime_region();
        self.activation_dues().owner_node = Some(node);
        node
    }
    /// The current activation's dues slot. Always present — a base slot covers
    /// the top level and every push/pop pair keeps the stack parallel to
    /// `activation_region_maps`.
    #[inline]
    pub(crate) fn activation_dues(&mut self) -> &mut ActivationDues {
        self.fiber
            .activation_dues
            .last_mut()
            .expect("dues stack must be non-empty (a base slot covers the top level)")
    }
    /// Take over one release a frame-replacing tail call stranded, recorded on
    /// the activation that owes it (docs/impl/region/owner.md § "A deferred
    /// tail-call release has the node's life"). Called from `tail_call_inner`,
    /// where the current activation is still the caller's — which is the
    /// activation the frame replacement hands the obligation to.
    #[inline]
    pub(crate) fn defer_activation_release(&mut self, region: RuntimeRegion) {
        self.activation_dues().defer(region);
    }
    /// Take everything the current activation owes, leaving the slot empty: the
    /// owner node (`None` if it never adopted) and the deferred set (empty if it
    /// made no frame-replacing tail call). The MOVE that keeps the record in
    /// exactly one place — the live slot, or one parked frame.
    #[inline]
    pub(crate) fn take_activation_dues(&mut self) -> ActivationDues {
        std::mem::take(self.activation_dues())
    }
    /// Discharge everything the current activation owes. The owner node gets one
    /// tolerant decref — rc 1→0, subtree drop over the node plus every member the
    /// activation adopted (interior cycles reclaim with the set; the Shared
    /// frontier cascades once). Each deferred region gets the decref its
    /// emitting instruction would have run, had the tail call not replaced the
    /// frame holding it. Runs at the activation's NORMAL completion on every
    /// tier — the interpreter trampoline's clean break and the compiled `Return`
    /// path (`elle_jit_release_activation_dues`) — never from an emitted
    /// drop instruction (docs/impl/region/owner.md § "Owner nodes").
    pub(crate) fn release_activation_dues(&mut self) {
        let dues = self.take_activation_dues();
        for region in dues.deferred {
            self.heap().decref_region_if_present(region);
        }
        if let Some(node) = dues.owner_node {
            self.heap().decref_region_if_present(node);
        }
    }
    /// The callee closure's runtime region, whose release the new activation
    /// takes over — the CALLEE channel of the deferred set
    /// ([`Self::defer_activation_release`]).
    ///
    /// Called ONLY when the compiler flagged this tail call's callee as a
    /// per-call local closure whose release is dead past the `TailCall`
    /// (`lower_call`'s `defer_callee_release`). That covers one region per call
    /// through every higher-order stdlib fn — `fold`/`map`/`filter`/…, whose
    /// recursion tail-calls a `letrec`-bound `go`. It is NOT flagged for a
    /// callee that is not a per-call local closure: a top-level `defn` is a
    /// program-root closure whose region is held for the program's life and has
    /// no per-call decref, and a native or parameter callee replaces no frame.
    /// Releasing a program-root region would be a use-after-free, so the
    /// discrimination is made at compile time (the solver knows whether the
    /// closure's region dies at the call) and this is just the runtime region
    /// lookup. `None` for an immediate (no region). See
    /// `tests/elle/region-tailcall-closure-callee-leak.lisp`.
    #[inline]
    pub(crate) fn tail_callee_release_region(&self, func: Value) -> Option<RuntimeRegion> {
        crate::value::arena::region_of(unsafe { &mut *self.heap_ptr }, func)
    }
    /// Pop the current region-remap frame on normal closure return, with its
    /// parallel dues slot. The base frame (top level) is never popped.
    #[inline]
    pub(crate) fn pop_activation_region_map(&mut self) {
        if self.fiber.activation_region_maps.len() > 1 {
            self.fiber.activation_region_maps.pop();
            self.fiber.activation_dues.pop();
        }
        debug_assert_eq!(
            self.fiber.activation_region_maps.len(),
            self.fiber.activation_dues.len(),
            "the dues stack must parallel the region-remap stack (one slot \
             per activation frame)",
        );
    }
}
