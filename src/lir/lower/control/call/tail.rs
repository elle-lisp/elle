// audited: 2026-09-19
//! Lowering a tail call: the frame-replacing `TailCall`, the relocation point
//! it opens, and what its native fall-through still owes.
//!
//! docs/impl/region/relocate.md
//! docs/impl/region/rules.md

use super::*;

impl<'a> Lowerer<'a> {
    /// Emit the frame-replacing call, once `lower_call` has lowered the
    /// arguments and the callee. `borrowed_arg_slots` names the retains the
    /// argument loop minted, which the native fall-through below consumes.
    pub(super) fn lower_tail_call(
        &mut self,
        func: &Hir,
        args: &[CallArg],
        arg_regs: Vec<Reg>,
        func_reg: Reg,
        arity_checked: bool,
        borrowed_arg_slots: Vec<u16>,
    ) -> Result<Reg, String> {
        // Move-on-tail-call, per argument (docs/impl/region/rules.md Rule 5).
        //
        // A tail call replaces the frame, so the caller's value-based
        // release for an arg is emitted dead (after the `TailCall`, by
        // `lower_expr`'s trailing `emit_decrefs_for`) and never runs.
        // For an OWNED arg — a value built in the body, an owned local,
        // an owned param loaded from a local slot — that never-executed
        // release IS the ownership transfer: the caller's owning ref
        // moves to the callee, which releases it at the param's last use.
        // No caller incref; the move balances.
        //
        // A BORROWED arg has no such transfer. A captured upvalue is
        // owned by the closure env's capture-incref (cascade-released
        // when the closure region dies), not by this activation — so
        // pure-moving it hands the callee a reference the caller never
        // owned, and the callee's owned-param release over-frees it,
        // draining the capture RC to a premature free
        // (region-tail-move-borrow-uaf.lisp; the `<stdlib>:1759` async
        // scheduler UAF — its `pending`/`runnable`/`fiber-io` @structs
        // are captured upvalues forwarded into `put`/`del`/... tail
        // calls). Hand the callee one fresh owning reference, the
        // tail-position mirror of the non-tail `CallArgument` incref
        // (`push_param`, `own_params`); the callee's release then
        // balances this incref and leaves the env's capture-ref intact.
        // A NATIVE callee never releases args, so the post-`TailCall`
        // fall-through consumes the retain instead (the
        // `borrowed_arg_slots` releases below) — one consumer on
        // either path.
        // The incref itself was already emitted per-arg in the
        // arg-lowering loop above (it MUST precede the later arg/func
        // pushes so the emitter need not `DupN` the value to the top);
        // see that loop's comment for the stack-layout reasoning.

        // Emit pending RegionExits before TailCall — the scope's
        // allocations must be freed before the frame is replaced.
        //
        self.emit_pending_free_regions();

        // `dst` is the call's result register, also returned as this
        // expression's value so the enclosing tail position's `Return`
        // names it. On the native-completion path the JIT binds it to
        // the native's result and runs the post-`TailCall` releases
        // (see `LirInstr::TailCall`); the interpreter leaves the result
        // on the stack and ignores it.
        let dst = self.fresh_reg();
        let defer_callee_release = self.tail_callee_defers_release(func);
        // A letrec body tail-calling a NON-member out of a closure-cycle
        // merged arena carries the arena's root slot: the binding-scope
        // `DecrefRegion` is dead past this frame-replacing `TailCall`, so a
        // closure callee's new activation takes over the arena's release,
        // freeing it at the
        // recursion's completion (a native callee never consumes it and the
        // live scope-exit drop fires). Keyed by the tail-call HirId in
        // `cycle_tail_release`; canonicalized through `merged_root` by
        // `static_slot` like every merge slot. A MEMBER callee is absent from
        // the map and keeps `defer_callee_release` (the two never both fire).
        let deferred_release_slot = self
            .current_hir_id
            .and_then(|id| self.region_info.cycle_tail_release.get(&id).copied())
            .map(|root| self.static_slot(root));
        // The values the call is about to consume, in the registers that
        // hold them — what the relocation point below must not release.
        let operands: Vec<Reg> = arg_regs.iter().copied().chain([func_reg]).collect();
        self.emit_alloc(|region| LirInstr::TailCall {
            region,
            dst,
            func: func_reg,
            args: arg_regs,
            arity_checked,
            defer_callee_release,
            deferred_release_slot,
            // The borrowed-argument retains this call minted, so a
            // SIGNAL exit — which reaches neither the callee's
            // owned-param release nor the fall-through block below —
            // can consume them itself
            // (docs/impl/region/mechanism.md § "What the fall-through
            // owes, a signal exit owes too").
            borrowed_arg_slots: borrowed_arg_slots.clone(),
        });
        // From here the block runs only on the NATIVE fall-through: a
        // native pushes no bytecode frame and the dispatch loop continues
        // into it, while a closure callee replaces the frame and never
        // arrives. The two instructions this arm emits next belong to that
        // path by design (the ReturnValue retain balances the native's
        // pass-through, and each borrowed-arg release consumes a retain the
        // callee's owned-param release would otherwise have taken). Every
        // release the ENCLOSING scopes emit into this block afterwards does
        // not: it is the frame's own reference, stranded once per call.
        // Open the relocation point that carries those releases back ahead
        // of the frame replacement (docs/impl/region/mechanism.md § "A
        // release past a frame-replacing tail call is not a release").
        if let Some(call_id) = self.current_hir_id {
            self.open_tail_exit_hoist(call_id, func, args, &operands);
        }
        // ReturnValue retain on the native-completion fall-through, the
        // tail-position mirror of `lower_return`'s `IncrefValueRegion`.
        // A native/collection tail call pushes NO bytecode frame: on
        // normal completion the dispatch loop pushes the result and runs
        // this post-`TailCall` block (`tail_call_inner`, src/vm/call.rs)
        // before the enclosing lambda's `Return`. The native already
        // applied ONE pass-through retain (`dispatch_native_call`), which
        // the caller's `DecrefValueRegion` consumes — so WITHOUT a second
        // retain here a heap pass-through result (`first`/`rest`/`get`, a
        // collection call-index `(xs i)`, a tail-returned HOF result) has
        // its single owning reference drained by the caller and is freed
        // under the caller's borrow (region-native-tail-return-uaf.lisp,
        // docs/impl/region/rules.md Rules 4/5/8). This incref must PRECEDE the dead
        // owned-arg `DecrefValueRegion`s (emitted next by the enclosing
        // `lower_expr`), matching `lower_return`'s retain-before-decref
        // ordering. The frame-replacing closure tail call never reaches
        // this instruction (the callee emits its own `Return` retain), and
        // an immediate result no-ops the incref — so it is correct
        // unconditionally. The emitter's `IncrefValueRegion` peeks the
        // operand-stack top, which is exactly the native's pushed result.
        //
        // This is the tail-position twin of `lower_return`'s return mint.
        //
        // EXCEPT a `-mut` PASS-THROUGH store/remove funnel whose wrapper released
        // the CONTAINER owned-param reference at this site
        // (`container_release_sites`, set by the per-arm container compensation
        // only for the `-mut` pass-through subset). There the result IS arg0 —
        // the container the caller passed in and already owns a reference to —
        // and the wrapper no longer holds it after the arm, so a second
        // `ReturnValue` retain would out-count the caller's single result release
        // (the over-keep the compensation closes: `set-add`/`struct-put`/
        // `del-wrapper` probes). Two gates keep it sound: (1) a RAW (non-wrapper)
        // funnel is not compensated (no branch), so it retains its ReturnValue;
        // (2) an IMMUTABLE funnel's FRESH result is excluded from
        // `container_release_sites` (only `-mut` sites qualify) — dropping its
        // ReturnValue would over-free a result stored into a reassigned slot,
        // whose move consumes that retain (`resource.lisp` struct-assoc). A
        // `first`/`rest`/`get` borrow is never a container site.
        let container_released_here = self
            .current_hir_id
            .is_some_and(|id| self.region_info.container_release_sites.contains(&id));
        // AND a moves-out ∩ PassThrough native (`%pop`/`%pop-array*`): the
        // native body already escape-retained the moved-out element in place
        // (`arena::pop_with_decref`), and `dispatch_native_call` skipped its
        // own pass-through retain (`def.moves_out`) — so that in-body retain is
        // the caller's single owning reference. A second ReturnValue retain
        // here double-counts and frees the element under a live reference
        // (`region_pop_tail_moves_out_uaf`). Recorded only for the PassThrough
        // subset (`moves_out_release_sites`), so a FRESH-result moves-out pop
        // (`@string` grapheme / `@bytes` int) is absent and KEEPS its retain.
        let moves_out_here = self
            .current_hir_id
            .is_some_and(|id| self.region_info.moves_out_release_sites.contains(&id));
        // A closure-cycle arena rides this site's fall-through (the letrec body
        // tail-calls a non-member out of a merged arena), and the merge admitted
        // it on the premise that SOME mint stands between the native's result and
        // the binding-scope `DecrefRegion` emitted after this body
        // (docs/impl/region/letrec.md § The frontier gate). The two suppressions
        // below hand that role to a retain the native already took, which the
        // premise does not read — so the merge must never have admitted a site
        // wearing them. It cannot today (a container site needs a `Match`-armed
        // dispatch body, which the admission's exhaustiveness reading refuses,
        // and a moves-out result is an element the store funnel counted
        // separately), and asserting it here turns a future widening of either
        // set into a loud panic at the seam rather than a stale-arena deref.
        debug_assert!(
            deferred_release_slot.is_none() || !(container_released_here || moves_out_here),
            "a closure-cycle tail-release site must keep its ReturnValue mint or \
         a `Return`'s: the arena's binding-scope release runs after this \
         block on the native fall-through"
        );
        if !container_released_here && !moves_out_here && !self.return_mint_covers_here() {
            self.emit(LirInstr::IncrefValueRegion { src: dst });
        }
        // Consume each borrowed-arg retain on the native-completion
        // fall-through: a native callee borrows its args (no owned-param
        // release), so without this the retain pins the arg's region rc
        // once per call. Dead code for a frame-replacing closure callee
        // — there the callee's owned-param release consumes the retain
        // and this block never runs. Each retain is thus consumed
        // exactly once on either path. Ordered AFTER the ReturnValue
        // retain above, which must peek the native's result while it is
        // the stack top; the LoadLocal/DecrefValueRegion pair here is
        // push-pop-neutral around it.
        for &slot in &borrowed_arg_slots {
            let v = self.fresh_reg();
            self.emit(LirInstr::LoadLocal { dst: v, slot });
            self.emit(LirInstr::DecrefValueRegion { src: v });
        }
        Ok(dst)
    }
}
