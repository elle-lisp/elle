use super::*;

impl<'a> Lowerer<'a> {
    /// Does this tail call's callee CLOSURE die at the call node — i.e. is it a
    /// per-call local closure whose `DecrefRegion` the solver placed here, which
    /// the frame-replacing `TailCall` then strands as dead code? If so, the
    /// runtime must ADOPT the closure (release its region when the new activation
    /// completes) to supply that missing decref.
    ///
    /// Two facts decide it, from two sources:
    /// - **region-locality** (a region fact): the callee's region demises at this
    ///   call's `decref_point` (this node) AND its decref is one the frame owns
    ///   (not in `suppressed_decref_regions`). A program-root callee (a top-level
    ///   `defn`) or a primitive has no per-call region here, and a suppressed
    ///   region's decref is owned by the store path (never stamped as an ordinary
    ///   alloc) — adopting either decrements an RC the frame never raised (a
    ///   phantom `DecrefRegion` / use-after-free). `EscapeInfo` cannot express
    ///   "has an owned per-call region here", so this stays a region fact.
    /// - **escape** (the authoritative analysis): the callee is adopted only when
    ///   it does NOT escape its definition (`EscapeInfo::lambda_escapes_definition`
    ///   / `binding_escapes_activation`). An escaping closure outlives the call and
    ///   must not be freed by the new activation. This reads the one escape
    ///   analysis every consumer reads, in place of the region-level
    ///   `suppressed_decref_regions` proxy.
    ///
    /// Like `tail_arg_is_borrowed`, adopt is **transitional value-RC machinery**:
    /// the ownership forest reclaims a non-escaping per-call callee as part
    /// of the activation's Owned subtree (dropped as a unit — no stranded decref to
    /// supply), so this predicate is subsumed there, not preserved. Its lasting
    /// contribution is reading `EscapeInfo`, the analysis that drives the forest's
    /// Owned/Shared classification.
    fn tail_callee_adopts(&self, func: &Hir) -> bool {
        let Some(call_id) = self.current_hir_id else {
            return false;
        };
        let Some(dying) = self.decrefs_by_decref_point.get(&call_id) else {
            return false;
        };
        // A captured callee (a self-recursive `letrec` closure like `fold`'s
        // `go`, or any closure captured by a sibling) is read through a
        // `DerefCell` that `functionalize` wraps around a needs-capture binding —
        // look through it to the `Var`, exactly as the solver's `Return` arm
        // does. Without this, captured closures (the whole HOF layer) never match
        // and never adopt.
        let func = match &func.kind {
            HirKind::DerefCell { cell } => &**cell,
            _ => func,
        };
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
        // Region-locality (a region fact, not escape): the callee must have a
        // per-call region that demises at THIS node AND whose decref the frame
        // actually owns. A program-root callee (top-level `defn`) or a primitive
        // has no per-call region here, and a SUPPRESSED region's decref is owned
        // by the store path (the reassign-gate / container model) and was never
        // stamped as an ordinary alloc — adopting either decrements an RC the
        // frame never raised (a phantom `DecrefRegion` panic / use-after-free).
        // `EscapeInfo` cannot express "has an owned per-call region here", so this
        // stays a region fact.
        let dies_here = func_regions
            .iter()
            .any(|r| dying.contains(r) && !self.region_info.suppressed_decref_regions.contains(r));
        // A self-recursive local closure (cell-free — its self-reference resolves to
        // the executing closure) is a per-call allocation whose region lives through
        // the whole recursion. Its scope-end `DecrefRegion` lands at the enclosing
        // letrec/def scope; when the defining body is a tail call, the frame-replacing
        // `TailCall` strands that release as dead code (`stranded_self_bindings`), and
        // — because the binding is referenced across branches and its own body —
        // `dies_here` (the demise landing at THIS call node) does not reliably catch
        // it. So a tail call to a stranded self-recursive binding adopts its region
        // directly: the runtime frees it once at the recursion's normal completion
        // (deduped), reclaimed per call like a top-level recursive `defn`. Gating on
        // `stranded_self_bindings` (a tail-call body) — not merely self-recursive — is
        // load-bearing: a non-tail body's release fires LIVE, and adopting it too would
        // free the region twice (the executing-closure re-dispatch then reads a
        // recycled page). Gated additionally on the GENUINE frontier escape — return ∪
        // fiber, not the full `binding_escapes_activation`: the latter folds in
        // store/capture CONTAINMENT (a self-recursive closure held by a local container
        // dies WITH the activation, so it must still be adopted), which would
        // over-conservatively re-strand the release into a leak. Only a closure
        // actually returned or sent to a fiber outlives the activation.
        if let HirKind::Var(b) = &func.kind {
            if self.stranded_self_bindings.contains(b) {
                // Invariant: a stranded self-recursive binding is CELL-FREE
                // (`!needs_capture()`; the strand sites in `binding.rs` both gate on
                // it, docs/impl/selfrec.md § the cell-free gate). A sibling-captured
                // (`needs_capture`) member's closure region is released by its forward
                // cell's cascade; adopting it here decrefs that region a SECOND time,
                // under the still-live cell — the captured-self-tail double-free
                // (tests/elle/region-selfrec-captured-tail-adopt.lisp). Asserting at
                // the CONSUMER catches any future strand path that skips the gate,
                // turning that UAF into a loud panic at the seam.
                debug_assert!(
                    !self.arena.get(*b).needs_capture(),
                    "stranded self-recursive binding {b:?} is needs_capture: its forward \
                     cell already releases the closure region, so a tail-call adopt would \
                     double-free it (see docs/impl/selfrec.md § the cell-free gate)"
                );
                let frontier_escapes = self.escape_info.binding_escapes_via_return(*b)
                    || self.escape_info.escapes_fiber(*b);
                return !frontier_escapes;
            }
            // A letrec closure-cycle merge member the enclosing letrec's BODY
            // tail-calls: the merged arena's binding-scope DecrefRegion is dead
            // past this frame-replacing TailCall, so the adopt supplies its
            // release once at the recursion's normal completion — the mutual
            // twin of the stranded-self adopt above. Honoured only through a
            // NON-upvalue reference: in the letrec's own function the binding is
            // a plain stack-slot local, while a nested closure reads it as an
            // upvalue — and a nested closure's activation completes before later
            // uses of the arena, so adopting there would free it early. Gated on
            // the genuine frontier escape exactly as the self path is.
            if self.stranded_cycle_bindings.contains(b) && !self.upvalue_bindings.contains(b) {
                let frontier_escapes = self.escape_info.binding_escapes_via_return(*b)
                    || self.escape_info.escapes_fiber(*b);
                return !frontier_escapes;
            }
        }
        // Any other reference to a closure-cycle merge member never adopts: the
        // merged arena is released exactly once by the merge's own channel (the
        // binding-scope DecrefRegion, or the stranded-cycle adopt above), so a
        // second adopt — an interior sibling rotation (`ev` tail-calling `od`,
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
        // Escape is read from the authoritative analysis: among those owned
        // per-call callees, adopt only one that does NOT escape its definition —
        // an escaping closure outlives the call and must not be freed by the new
        // activation. This escape refinement is a distinct responsibility from
        // the region-level suppression proxy.
        let escapes = match &func.kind {
            HirKind::Var(b) => self.escape_info.binding_escapes_activation(*b),
            _ => self.escape_info.lambda_escapes_definition(func.id),
        };
        dies_here && !escapes
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
            for arg in args {
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
                let borrowed = is_tail && self.tail_arg_is_borrowed(&arg.expr);
                if borrowed {
                    self.deferred_decref_points.insert(arg.expr.id);
                }
                let reg = self.lower_expr(&arg.expr)?;
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
                    self.deferred_decref_points.remove(&arg.expr.id);
                    self.emit_decrefs_for(arg.expr.id, Some(reg));
                }
                arg_regs.push(reg);
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
                // Move-on-tail-call, per argument (docs/impl/region-rules.md Rule 5).
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
                let adopt_callee = self.tail_callee_adopts(func);
                // A letrec body tail-calling a NON-member out of a closure-cycle
                // merged arena carries the arena's root slot: the binding-scope
                // `DecrefRegion` is dead past this frame-replacing `TailCall`, so a
                // closure callee's new activation adopts and frees the arena at the
                // recursion's completion (a native callee never consumes it and the
                // live scope-exit drop fires). Keyed by the tail-call HirId in
                // `cycle_tail_adopt`; canonicalized through `merged_root` by
                // `static_slot` like every merge slot. A MEMBER callee is absent from
                // the map and keeps `adopt_callee` (the two never both fire).
                let adopt_region_slot = self
                    .current_hir_id
                    .and_then(|id| self.region_info.cycle_tail_adopt.get(&id).copied())
                    .map(|root| self.static_slot(root));
                self.emit_alloc(|region| LirInstr::TailCall {
                    region,
                    dst,
                    func: func_reg,
                    args: arg_regs,
                    arity_checked,
                    adopt_callee,
                    adopt_region_slot,
                });
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
                // docs/impl/region-rules.md Rules 4/5/8). This incref must PRECEDE the dead
                // owned-arg `DecrefValueRegion`s (emitted next by the enclosing
                // `lower_expr`), matching `lower_return`'s retain-before-decref
                // ordering. The frame-replacing closure tail call never reaches
                // this instruction (the callee emits its own `Return` retain), and
                // an immediate result no-ops the incref — so it is correct
                // unconditionally. The emitter's `IncrefValueRegion` peeks the
                // operand-stack top, which is exactly the native's pushed result.
                //
                // This is the tail-position twin of `lower_return`'s return mint.
                self.emit(LirInstr::IncrefValueRegion { src: dst });
                Ok(dst)
            } else {
                let dst = self.fresh_reg();
                if call_signals
                    .intersects(crate::signals::SIG_YIELD.union(crate::signals::SIG_DEBUG))
                {
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
                // fixed (region-splice-tail-return.lisp; docs/impl/region-rules.md
                // Rules 4/5). Dead for a frame-replacing closure tail call;
                // no-op for an immediate result. The emitter peeks the operand
                // stack top — the native's pushed result.
                let dst = self.fresh_reg();
                self.emit(LirInstr::IncrefValueRegion { src: dst });
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
