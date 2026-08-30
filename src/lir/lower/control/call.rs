use super::*;

/// Does this HIR subtree lower a **loop** (a back-edge) in the CURRENT function —
/// a `While`/`Loop` not inside a nested lambda?
///
/// The bytecode VM is stack-based: a call parks each already-lowered argument on
/// the operand stack while it lowers the next. A loop, on re-entering its head,
/// resets the operand stack to its head-block layout — which does not include an
/// earlier argument value parked *below* the loop's working set, so that value is
/// dropped and the call reads whatever sits in its slot instead (a phantom-arity
/// or wrong-value corruption at the call). `if`/`begin`/`match` are forward merges
/// that carry the full stack across, so only a back-edge loop triggers it. When a
/// later argument contains one, `lower_call` spills every argument to a local as
/// it is lowered and reloads them adjacent to the call.
///
/// Nested lambda bodies are separate lowered functions, so their loops never touch
/// this function's operand stack and are not counted (stop at the `Lambda`).
fn hir_contains_loop(h: &Hir) -> bool {
    match &h.kind {
        HirKind::While { .. } | HirKind::Loop { .. } => true,
        HirKind::Lambda { .. } => false,
        _ => {
            let mut found = false;
            h.for_each_child(|c| found |= hir_contains_loop(c));
            found
        }
    }
}

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
    fn tail_callee_defers_release(&self, func: &Hir) -> bool {
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
        // self-recursive — is load-bearing: a body that never replaces the frame runs
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

    /// Does a `Return` mint already cover the tail call being lowered? True for
    /// ANF's canonical wrap `(let [t (f …)] (return t))`, recorded by `lower_let`
    /// — there the frame names the result and `lower_return`'s mint plus the
    /// binding's `decref_point` carry the whole return convention, so the
    /// post-`TailCall` fall-through retain would be a second, unbalanced
    /// reference (docs/impl/region/mechanism.md § "The return mint is emitted
    /// exactly once"; pinned by `region-native-tail-compound-leak.lisp`).
    fn return_mint_covers_here(&self) -> bool {
        self.current_hir_id
            .is_some_and(|id| self.return_minted_calls.contains(&id))
    }

    pub(in crate::lir::lower) fn lower_call(
        &mut self,
        func: &Hir,
        args: &[CallArg],
        is_tail: bool,
        call_signals: SignalBits,
    ) -> Result<Reg, String> {
        let has_splice = args.iter().any(|a| a.spliced);

        if !has_splice {
            // === Common path: no spliced args ===
            // Check for intrinsic specialization
            let plain_args: Vec<&Hir> = args.iter().map(|a| &a.expr).collect();
            if let Some(result) = self.try_lower_intrinsic(func, &plain_args)? {
                return Ok(result);
            }

            let mut arg_regs = Vec::new();
            // Stash slots for the borrowed args' retained values: the retain is
            // consumed by the callee's owned-param release only when the callee
            // is a frame-replacing CLOSURE. A NATIVE tail callee borrows its
            // args and releases nothing, so the call falls through to the
            // post-`TailCall` block — which must consume the retain itself
            // (below) or every borrowed arg pins its region's rc by one per
            // call, an unbounded over-keep
            // (region-const-tail-move-borrow-uaf.lisp, witness (c)). The slot
            // stash keeps the RETAINED value addressable there: re-lowering the
            // arg instead would re-READ a cell a callee-run closure (`apply`)
            // may have reassigned, releasing the wrong value.
            let mut borrowed_arg_slots = Vec::new();
            // If a LATER argument lowers a loop, its back-edge resets the operand
            // stack and drops any earlier argument value parked there (see
            // `hir_contains_loop`). Park every argument in a local as it is lowered
            // and reload them adjacent to the call, so no argument value survives on
            // the operand stack across the loop.
            let spill_across_loop =
                args.len() >= 2 && args.iter().skip(1).any(|a| hir_contains_loop(&a.expr));
            let mut arg_spill_slots: Vec<Option<u16>> = Vec::new();
            // Regions an earlier argument of this tail call already MOVED. The
            // frame holds one reference per region and the callee releases once
            // per owned parameter, so only the first occurrence is funded by the
            // move; a later one must be minted exactly as a borrowed argument is
            // (docs/impl/region/rules.md Rule 5).
            let mut moved_arg_regions: rustc_hash::FxHashSet<crate::hir::region::Region> =
                rustc_hash::FxHashSet::default();
            // An argument past the callee's fixed parameters is collected into the
            // rest parameter's fresh list instead of becoming an owned param, and
            // that list's own allocation scan and surplus release balance it
            // without a mint — so a repeat landing there must NOT take one. Only a
            // callee this compilation resolved says so; anything else keeps the
            // mint, which is the leak-preserving direction.
            let rest_from = self
                .current_hir_id
                .and_then(|id| self.region_info.tail_callee_facts.get(&id))
                .map(|f| f.fixed_params);
            // A dynamic `emit` whose payload this body releases nowhere. The park
            // owes the body one reference of every value it yields, and the shape
            // that carries it depends on position (docs/impl/region/owner.md
            // § "What yields is the emit OPERATION, not the `Emit` node"): a TAIL
            // call already mints one for a borrowed argument, which the suspending
            // exit leaves standing and the replayed fall-through releases, so only
            // a non-tail call owes a mint of its own. Routed through `borrowed`
            // below, whose retain / private stash / post-call release is the exact
            // shape `lower_emit` uses at the terminator.
            let emit_payload_arg = (!is_tail
                && self
                    .current_hir_id
                    .is_some_and(|id| self.region_info.borrowed_emit_payloads.contains(&id)))
            .then_some(crate::hir::region::EMIT_PAYLOAD_ARG);
            for (index, arg) in args.iter().enumerate() {
                // For a tail call, a BORROWED arg must be handed the callee a
                // fresh owning reference (see the move-on-tail-call comment at
                // the `is_tail` block below for the ownership argument). The
                // retain must ORDER BEFORE the arg node's own `emit_decrefs_for`:
                // when the borrowed arg is a `@`-mutable param read, that node's
                // last-use release is a `DecrefCellRegion` of the param's OWN cell,
                // whose cascade frees the cell's contents — the very value being
                // moved. A retain emitted after it reads a freed page (the reassigned
                // mutable-param double-release UAF, region-mutable-reassign-param.lisp).
                // So defer this node's decrefs, emit the retain, then emit the
                // deferred decrefs — the retain now precedes the cell's cascade-free.
                //
                // A REPEATED owned argument takes the same treatment for a
                // different reason: the move handed over one reference and this
                // occurrence is a second owned parameter to release. Only an owned
                // occupant of a fixed parameter both consumes the move and can be
                // starved by an earlier one — a borrowed argument mints for itself,
                // and a rest position is balanced by the collected list.
                let mut leaf_regions = rustc_hash::FxHashSet::default();
                if is_tail {
                    self.arg_leaf_regions(&arg.expr, &mut leaf_regions);
                }
                let borrowed_leaf = is_tail && self.tail_arg_is_borrowed(&arg.expr);
                let takes_the_move =
                    is_tail && !borrowed_leaf && rest_from.is_none_or(|n| index < n);
                let repeated =
                    takes_the_move && leaf_regions.iter().any(|r| moved_arg_regions.contains(r));
                if takes_the_move && !repeated {
                    moved_arg_regions.extend(leaf_regions);
                }
                let borrowed = borrowed_leaf || repeated || emit_payload_arg == Some(index);
                if borrowed {
                    self.deferred_decref_points.insert(arg.expr.id);
                }
                let mut reg = self.lower_expr(&arg.expr)?;
                // Emit the retain HERE, while this arg's value is on top of the
                // operand stack — the emitter's `IncrefValueRegion` peeks the
                // stack top, so deferring it until after the remaining args and
                // the func are pushed would force `ensure_on_top` to `DupN` the
                // value up, orphaning it and corrupting the tail call's argument
                // layout. The single-arg case tolerated the late incref; a
                // multi-arg tail call (e.g. a `(struct :k v …)` whose values are
                // cell-backed upvalues, as stdlib's export struct is) does not.
                // `IncrefValueRegion` does not pop, so the arg stays in place for
                // the rest of the arg/func pushes and the `TailCall`.
                if borrowed {
                    self.emit(LirInstr::IncrefValueRegion { src: reg });
                    // Stash-and-reload: `StoreLocal` consumes the value (the
                    // emitter auto-pops), so reload it as the arg actually
                    // handed to the call — same value, layout intact.
                    let slot = self.current_func.num_locals;
                    self.current_func.num_locals += 1;
                    self.emit(LirInstr::StoreLocal { slot, src: reg });
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot,
                    });
                    reg = reloaded;
                    borrowed_arg_slots.push(slot);
                    self.deferred_decref_points.remove(&arg.expr.id);
                    self.emit_decrefs_for(arg.expr.id, Some(reg));
                }
                // Park this argument off the operand stack (a `StoreLocal` auto-pops
                // it) so a later argument's loop cannot clobber it; the value flows
                // through the local unchanged, immediates included.
                if spill_across_loop {
                    let slot = self.current_func.num_locals;
                    self.current_func.num_locals += 1;
                    self.emit(LirInstr::StoreLocal { slot, src: reg });
                    arg_spill_slots.push(Some(slot));
                } else {
                    arg_spill_slots.push(None);
                }
                arg_regs.push(reg);
            }
            // Reload the parked arguments in order, so they sit on the stack below
            // the callee — the `[arg0..argN, func]` layout `Call` expects — freshly
            // loaded past the loop that would otherwise have clobbered them.
            for (i, slot) in arg_spill_slots.iter().enumerate() {
                if let Some(slot) = *slot {
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot,
                    });
                    arg_regs[i] = reloaded;
                }
            }
            // Lower the callee. A self-reference here lowers to `LoadSelf` (the
            // executing closure) exactly as in value position, so the call re-enters
            // the current code+env — the self-call re-dispatch (`lower_var`).
            let func_reg = self.lower_expr(func)?;

            // Determine if the compiler verified arity for this call.
            // True when the callee is a primitive binding that hasn't been
            // shadowed or mutated (the analyzer would have errored on arity
            // mismatch, so if compilation succeeded the arity is correct).
            let arity_checked = if let HirKind::Var(binding) = &func.kind {
                let bi = self.arena.get(*binding);
                bi.is_primitive
                    && bi.is_immutable
                    && !bi.is_mutated
                    && self
                        .immutable_values
                        .get(binding)
                        .is_some_and(|v| v.is_native_fn())
            } else {
                false
            };

            if is_tail {
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
            } else {
                let dst = self.fresh_reg();
                // A call needs the CPS suspending convention (a resumable
                // continuation) if the callee can SUSPEND and later RESUME —
                // every signal the fiber scheduler parks on and wakes: a plain
                // yield, an io request, and a structured-concurrency wait. SIG_IO
                // and SIG_WAIT matter on their own because signal narrowing can
                // resolve an `(emit :io …)` / `(emit :wait …)` to just that bit,
                // dropping the SIG_YIELD `emit`'s static signal carries — so a
                // wrapper like `emit-wait` / `ev/join`, whose narrowed signal is
                // SIG_WAIT alone, would otherwise compile to a plain Call with no
                // continuation frame, and the code after the wait would be lost on
                // resume (the whole async scheduler's `handle-wait` path). SIG_ERROR
                // / SIG_HALT are excluded: they unwind or terminate, never resume.
                // Pinned by tests/elle/wasm-wait-call-resumes.lisp.
                if call_signals.intersects(
                    crate::signals::SIG_YIELD
                        .union(crate::signals::SIG_DEBUG)
                        .union(crate::signals::SIG_IO)
                        .union(crate::signals::SIG_WAIT),
                ) {
                    self.emit_alloc(|region| LirInstr::SuspendingCall {
                        region,
                        dst,
                        func: func_reg,
                        args: arg_regs,
                        arity_checked,
                    });
                } else {
                    self.emit_alloc(|region| LirInstr::Call {
                        region,
                        dst,
                        func: func_reg,
                        args: arg_regs,
                        arity_checked,
                    });
                }
                // The body reference a dynamic `emit` park owes, released here:
                // the primitive parks at the instruction after this call, so this
                // is the continuation past the suspend — the release a fiber
                // abandoned while suspended never reaches and the discard discharge
                // stands in for (docs/impl/region/owner.md § "Park/unpark
                // symmetry"). The stash is private to this site, and the
                // `LoadLocal`/`DecrefValueRegion` pair is push-pop-neutral around
                // the result the call left on top. Empty for every other non-tail
                // call: only the emit payload sets `borrowed` off tail position.
                //
                // The nil stamp and the table entry are what make this release run
                // ONCE when the signal turns out to be terminal. There the catcher
                // consumes the delivery the raise minted and this retain answers to
                // the continuation alone — reached by a restart's replay, or, for a
                // fiber nobody restarts, by the abandoned-frame walk off exactly
                // this table (docs/impl/region/mechanism.md § "An abandoned frame
                // runs the releases it still owes"). A frame that takes both routes
                // reloads the stamp on the second and no-ops. A SUSPENDING raise
                // records the slot too and is unaffected: its parked payload is what
                // the walk protects, so the walk passes the slot over.
                for &slot in &borrowed_arg_slots {
                    let v = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal { dst: v, slot });
                    self.emit(LirInstr::DecrefValueRegion { src: v });
                    if let Ok(nil_reg) = self.emit_const(crate::lir::LirConst::Nil) {
                        self.emit(LirInstr::StoreLocal { slot, src: nil_reg });
                    }
                    if !self.current_func.frame_release_slots.contains(&slot) {
                        self.current_func.frame_release_slots.push(slot);
                    }
                }
                // After ANF (`src/hir/anf.rs`), every consumer position
                // for a Call has a synthetic `Let` binding owning the
                // result. The enclosing `lower_let` / `lower_letrec` /
                // `lower_define` records `region_to_slot[r]` so
                // `emit_decrefs_for` at the Call's `decref_point` can emit
                // `LoadLocal slot + DecrefValueRegion` — no shadow
                // stash slot needed at the Call site.
                Ok(dst)
            }
        } else {
            // === Splice path: build args array, then CallArrayMut ===
            // Lower all args first
            let mut lowered: Vec<(Reg, bool)> = Vec::new();
            for arg in args {
                let reg = self.lower_expr(&arg.expr)?;
                lowered.push((reg, arg.spliced));
            }
            // Callee: a self-reference lowers to `LoadSelf` (self-call re-dispatch),
            // as in the non-splice path.
            let func_reg = self.lower_expr(func)?;

            // Build the args array incrementally
            // Start with MakeArrayMut of the first run of non-spliced args
            let mut args_reg: Option<Reg> = None;

            for (reg, spliced) in &lowered {
                match (args_reg, spliced) {
                    (None, false) => {
                        // First arg, not spliced: create array with one element
                        let dst = self.fresh_reg();
                        self.emit_alloc(|region| LirInstr::MakeArrayMut {
                            region,
                            dst,
                            elements: vec![*reg],
                        });
                        args_reg = Some(dst);
                    }
                    (None, true) => {
                        // First arg, spliced: create empty array, then extend
                        let empty = self.fresh_reg();
                        self.emit_alloc(|region| LirInstr::MakeArrayMut {
                            region,
                            dst: empty,
                            elements: vec![],
                        });
                        let dst = self.fresh_reg();
                        self.emit(LirInstr::ArrayMutExtend {
                            dst,
                            array: empty,
                            source: *reg,
                        });
                        args_reg = Some(dst);
                    }
                    (Some(arr), false) => {
                        let dst = self.fresh_reg();
                        self.emit(LirInstr::ArrayMutPush {
                            dst,
                            array: arr,
                            value: *reg,
                        });
                        args_reg = Some(dst);
                    }
                    (Some(arr), true) => {
                        let dst = self.fresh_reg();
                        self.emit(LirInstr::ArrayMutExtend {
                            dst,
                            array: arr,
                            source: *reg,
                        });
                        args_reg = Some(dst);
                    }
                }
            }

            let final_args = args_reg.unwrap_or_else(|| {
                let dst = self.fresh_reg();
                self.emit_alloc(|region| LirInstr::MakeArrayMut {
                    region,
                    dst,
                    elements: vec![],
                });
                dst
            });

            if is_tail {
                self.emit_pending_free_regions();
                self.emit_alloc(|region| LirInstr::TailCallArrayMut {
                    region,
                    func: func_reg,
                    args: final_args,
                });
                // ReturnValue retain on the native-completion fall-through —
                // the splice/`apply` mirror of the non-splice `TailCall` arm
                // above. A splice tail call to a heap pass-through native
                // (`(first ;argv)`, `(get ;argv)`, an `apply`'d accessor) needs
                // the same retain or its result is freed under the caller's
                // borrow once the args-array leak that currently masks it is
                // fixed (region-splice-tail-return.lisp; docs/impl/region/rules.md
                // Rules 4/5). Dead for a frame-replacing closure tail call;
                // no-op for an immediate result. The emitter peeks the operand
                // stack top — the native's pushed result. Stands down under the
                // same one-mint rule as the non-splice arm when ANF named this
                // call's result and a `Return` mints for it.
                let dst = self.fresh_reg();
                if !self.return_mint_covers_here() {
                    self.emit(LirInstr::IncrefValueRegion { src: dst });
                }
                Ok(dst)
            } else {
                let dst = self.fresh_reg();
                self.emit_alloc(|region| LirInstr::CallArrayMut {
                    region,
                    dst,
                    func: func_reg,
                    args: final_args,
                });
                // ANF binds this Call's result; the binding's slot is
                // recorded in `region_to_slot` by the enclosing
                // `lower_let` / `lower_letrec` / `lower_define`.
                Ok(dst)
            }
        }
    }
}
