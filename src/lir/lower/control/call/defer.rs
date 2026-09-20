// audited: 2026-09-19
//! Whether a tail call's callee closure dies at the call, so the new
//! activation must take over the release the frame replacement strands.
//!
//! docs/impl/selfrec.md
//! docs/impl/region/letrec.md

use super::*;

impl<'a> Lowerer<'a> {
    /// Does this tail call's callee CLOSURE die at the call node — i.e. is it a
    /// per-call local closure whose `DecrefRegion` the solver placed here, which
    /// the frame-replacing `TailCall` then strands as dead code? If so, the new
    /// activation must TAKE OVER that release (run it when it completes) to
    /// supply the missing decref — a deferred decref on a still-`Counted` region,
    /// never an ownership-forest adoption.
    ///
    /// Asked in two layers. The **binding-keyed channels** come first: a callee the
    /// enclosing letrec or `def` marked stranded owns a release placed at that
    /// binder's SCOPE END, which this call's frame replacement leaves in dead code,
    /// so nothing about this node's own demises can answer for it (each channel's
    /// arm below carries its own argument).
    ///
    /// Every other callee is decided by two facts, from two sources:
    /// - **region-locality** (a region fact): the callee's region demises at this
    ///   call's `decref_point` (this node) AND its decref is one the frame owns
    ///   (not in `suppressed_decref_regions`). A program-root callee (a top-level
    ///   `defn`) or a primitive has no per-call region here, and a suppressed
    ///   region's decref is owned by the store path (never stamped as an ordinary
    ///   alloc) — deferring either's release decrements an RC the frame never raised (a
    ///   phantom `DecrefRegion` / use-after-free). `EscapeInfo` cannot express
    ///   "has an owned per-call region here", so this stays a region fact.
    /// - **escape** (the authoritative analysis): the release is deferred only when
    ///   it does NOT escape its definition (`EscapeInfo::lambda_escapes_definition`
    ///   / `binding_escapes_activation`). An escaping closure outlives the call and
    ///   must not be freed by the new activation. This reads the one escape
    ///   analysis every consumer reads, in place of the region-level
    ///   `suppressed_decref_regions` proxy.
    ///
    /// Like `tail_arg_is_borrowed`, the deferred release is **transitional value-RC machinery**:
    /// the ownership forest reclaims a non-escaping per-call callee as part
    /// of the activation's Owned subtree (dropped as a unit — no stranded decref to
    /// supply), so this predicate is subsumed there, not preserved. Its lasting
    /// contribution is reading `EscapeInfo`, the analysis that drives the forest's
    /// Owned/Shared classification.
    pub(super) fn tail_callee_defers_release(&self, func: &Hir) -> bool {
        let Some(call_id) = self.current_hir_id else {
            return false;
        };
        // Resolve the callee through the value-transparent wrappers to the
        // Var/Lambda leaf, mirroring the escape walk (`tail_sources`) and the
        // solver's `Return` arm:
        // - a captured callee (a self-recursive `letrec` closure like `fold`'s
        //   `go`, or any closure captured by a sibling) is read through a
        //   `DerefCell` that `functionalize` wraps around a needs-capture
        //   binding;
        // - a literal-lambda callee (`((fn [] …))`) reaches here as the `Let`
        //   the normalizer bound it under, its body the binding `Var`.
        // Without the resolution neither shape ever matches, its per-call
        // closure region never defers, and every such tail call leaks the
        // closure + template (the `protect`-body shape: the fiber wrapper's
        // tail call to its literal body closure).
        let mut func = func;
        loop {
            func = match &func.kind {
                HirKind::DerefCell { cell } => cell,
                HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => body,
                HirKind::Begin(exprs) | HirKind::Block { body: exprs, .. } => match exprs.last() {
                    Some(last) => last,
                    None => break,
                },
                _ => break,
            };
        }
        let func_regions: Vec<crate::hir::region::Region> = match &func.kind {
            HirKind::Var(b) => self
                .region_info
                .binding_source_regions
                .get(b)
                .cloned()
                .unwrap_or_default(),
            _ => self
                .region_info
                .alloc_region
                .get(&func.id)
                .into_iter()
                .copied()
                .collect(),
        };
        // The three binding-keyed channels below name a release the enclosing letrec
        // (or `def`) places at its SCOPE END, not at this call node, so they are asked
        // before the demise reading — whose `decrefs_by_decref_point` lookup answers a
        // different question and would otherwise refuse a stranded callee wherever this
        // node happens to be nobody's `decref_point`.
        //
        // A self-recursive local closure (cell-free — its self-reference resolves to
        // the executing closure) is a per-call allocation whose region lives through
        // the whole recursion. Its scope-end `DecrefRegion` lands at the enclosing
        // letrec/def scope; when the defining body is a tail call, the frame-replacing
        // `TailCall` strands that release as dead code (`stranded_self_bindings`), and
        // — because the binding is referenced across branches and its own body —
        // `dies_here` (the demise landing at THIS call node) does not reliably catch
        // it. So a tail call to a stranded self-recursive binding defers its region's
        // release directly: the runtime frees it once at the recursion's normal completion
        // (deduped), reclaimed per call like a top-level recursive `defn`. Gating on
        // `stranded_self_bindings` (a body that TAIL-CALLS the binding) — not merely
        // self-recursive — is what keeps it sound: a body that never replaces the frame runs
        // its release LIVE, and deferring it too would free the region twice (the
        // executing-closure re-dispatch then reads a recycled page).
        //
        // The channel asks escape NOTHING. The strand loses exactly one reference —
        // the frame's own, taken where the closure was allocated — and this deferral
        // is a decref that supplies exactly that one, at the recursion's normal
        // completion. Every facet a gate could read belongs to some OTHER reference:
        // store/capture are CONTAINMENT (a closure held by a local container dies WITH
        // the activation, so refusing there re-strands the release into a leak); the
        // RETURN facet is funded by the callee's own `Return` mint, which runs before
        // `trampoline_loop` breaks and fires the deferred decref; and a FIBER crossing
        // counts its own reference as it crosses — the emit's park retain into
        // `fiber.signal`, which the resumer's result release consumes, and
        // `chan/send`'s send-site incref, held until a receive builds the result
        // carrying the message. The crossing is a node of the same body, so it runs
        // first; a crossing INSIDE the recursion suspends, and a suspending exit
        // abandons the trampoline's whole deferred set — an over-keep, never a second
        // release (docs/impl/selfrec.md § "The deferral needs no escape gate").
        if let HirKind::Var(b) = &func.kind {
            if self.stranded_self_bindings.contains(b) {
                // Invariant: a stranded self-recursive binding is CELL-FREE
                // (`!needs_capture()`; the strand sites in `binding.rs` both gate on
                // it, docs/impl/selfrec.md § the cell-free gate). A sibling-captured
                // (`needs_capture`) member's closure region is released by its forward
                // cell's cascade; deferring its release here decrefs that region a SECOND time,
                // under the still-live cell — the captured-self-tail double-free
                // (tests/elle/region-selfrec-captured-tail-release.lisp). Asserting at
                // the CONSUMER catches any future strand path that skips the gate,
                // turning that UAF into a loud panic at the seam.
                debug_assert!(
                    !self.arena.get(*b).needs_capture(),
                    "stranded self-recursive binding {b:?} is needs_capture: its forward \
                     cell already releases the closure region, so a tail-call deferred release would \
                     double-free it (see docs/impl/selfrec.md § the cell-free gate)"
                );
                return true;
            }
            // A letrec closure-cycle merge member the enclosing letrec's BODY
            // tail-calls: the merged arena's binding-scope DecrefRegion is dead
            // past this frame-replacing TailCall, so the deferred release supplies
            // it once at the recursion's normal completion — the mutual
            // twin of the stranded-self deferral above. Honoured only through a
            // NON-upvalue reference: in the letrec's own function the binding is
            // a plain stack-slot local, while a nested closure reads it as an
            // upvalue — and a nested closure's activation completes before later
            // uses of the arena, so deferring there would free it early.
            //
            // The escape gate is the FIBER frontier alone, for the same reason and by
            // the same argument as the self path above: this deferral also runs at the
            // recursion's normal completion, after the `Return` mint that funds the
            // caller's reference — and the merge collapses a returned member's region
            // onto the arena, so that mint raises the arena's own count. The merge
            // admits a returned cycle only where every tail exit of the letrec body is
            // a member call, which is what makes this deferral the arena's sole release
            // (docs/impl/region/letrec.md § The frontier gate). A member reaches this
            // marking only through an ADMITTED merge, so the gate never re-argues
            // admission; it is kept whole so both ends of the channel state the same
            // premise.
            if self.stranded_cycle_bindings.contains(b) && !self.upvalue_bindings.contains(b) {
                return !self.escape_info.escapes_fiber(*b);
            }
            // A member of the enclosing letrec whose OWN closure region the solver
            // releases at that letrec's scope end (`stranded_member_bindings`): a
            // sibling captures it, so its uses span the letrec and `dies_here`
            // above — which reads a demise landing at THIS node — never sees it.
            // The relocation must leave that release where it is, the call being
            // about to enter the very closure it would free, so the exemption's
            // premise that the new activation takes it over is only true if this
            // channel runs it (docs/impl/region/mechanism.md § "What the exemption
            // keeps, a channel must still run").
            //
            // The escape gate is the FIBER frontier alone, by the same ordering
            // argument the two channels above make: this deferral is a decref that
            // runs at the callee's normal completion, AFTER the `Return` mint that
            // funds the caller's reference, so the return facet needs no bridge.
            // Only a fiber crossing hands the closure to a holder the compiler did
            // not place. The non-upvalue guard is the cycle channel's: a nested
            // closure that captures the member completes its own activation before
            // the enclosing letrec's later uses, so deferring there frees early.
            if self.stranded_member_bindings.contains(b) && !self.upvalue_bindings.contains(b) {
                return !self.escape_info.escapes_fiber(*b);
            }
        }
        // Any other reference to a closure-cycle merge member never defers: the
        // merged arena is released exactly once by the merge's own channel (the
        // binding-scope DecrefRegion, or the stranded-cycle deferral above), so a
        // second deferred release — an interior sibling rotation (`ev` tail-calling `od`,
        // whose region demises at that in-body call node and would otherwise
        // pass `dies_here` below), or a nested-closure call — would release the
        // still-live arena a second time (a double-free on a non-tail letrec
        // body, where the binding-scope drop fires live).
        if func_regions
            .iter()
            .any(|r| self.region_info.closure_cycle_members.contains(r))
        {
            return false;
        }
        // Region-locality (a region fact, not escape): the callee must have a
        // per-call region that demises at THIS node AND whose decref the frame
        // actually owns. A program-root callee (top-level `defn`) or a primitive
        // has no per-call region here, and a SUPPRESSED region's decref is owned
        // by the store path (the reassign-gate / container model) and was never
        // stamped as an ordinary alloc — deferring either's release decrements an RC the
        // frame never raised (a phantom `DecrefRegion` panic / use-after-free).
        // `EscapeInfo` cannot express "has an owned per-call region here", so this
        // stays a region fact.
        let Some(dying) = self.decrefs_by_decref_point.get(&call_id) else {
            return false;
        };
        let dies_here = func_regions
            .iter()
            .any(|r| dying.contains(r) && !self.region_info.suppressed_decref_regions.contains(r));
        // Escape is read from the authoritative analysis: among those owned
        // per-call callees, defer only one that does NOT escape its definition —
        // an escaping closure outlives the call and must not be freed by the new
        // activation. This escape refinement is a distinct responsibility from
        // the region-level suppression proxy.
        let escapes = match &func.kind {
            HirKind::Var(b) => self.escape_info.binding_escapes_activation(*b),
            _ => self.escape_info.lambda_escapes_definition(func.id),
        };
        dies_here && !escapes
    }
}
