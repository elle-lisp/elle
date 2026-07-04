//! Expression lowering - the main `lower_expr` dispatch

use super::*;

mod intrinsic;

mod loops;

impl<'a> Lowerer<'a> {
    /// Lower a HIR expression to LIR
    pub(super) fn lower_expr(&mut self, hir: &Hir) -> Result<Reg, String> {
        let saved_span = self.current_span.clone();
        let saved_hir_id = self.current_hir_id;
        self.current_span = hir.span.clone();
        self.current_hir_id = Some(hir.id);

        // Per-path branch compensation: if this node is a branch arm body whose
        // sibling arm holds a live-in region's `decref_point`, free that region at
        // this arm's head (it would otherwise leak on this path). Emitted into the
        // arm's basic block, before the arm body — hence before any tail call.
        self.emit_branch_compensation(hir.id);

        let result = match &hir.kind {
            HirKind::Nil => self.emit_const(LirConst::Nil),
            HirKind::EmptyList => self.emit_const(LirConst::EmptyList),
            HirKind::Bool(b) => self.emit_const(LirConst::Bool(*b)),
            HirKind::Int(n) => self.emit_const(LirConst::Int(*n)),
            HirKind::Float(f) => self.emit_const(LirConst::Float(*f)),
            HirKind::String(s) => {
                // A string literal is an ordinary allocation (not a pool load):
                // materialize it fresh into its OWN solver-assigned region. The
                // region is resolved from `current_hir_id` (this String node),
                // which the solver gave a region via `alloc_here`. `emit_alloc`
                // stamps the region (arming its `DecrefRegion` at `decref_point`).
                let dst = self.fresh_reg();
                let template = crate::value::ConstTemplate::String(s.clone());
                self.emit_alloc(|region| LirInstr::MaterializeConst {
                    dst,
                    template,
                    region,
                });
                Ok(dst)
            }
            HirKind::Keyword(name) => self.emit_const(LirConst::Keyword(name.clone())),

            HirKind::Var(binding) => self.lower_var(binding, &hir.span),
            HirKind::Let { bindings, body } => self.lower_let(bindings, body, hir.id),
            HirKind::Letrec { bindings, body } => self.lower_letrec(bindings, body, hir.id),
            HirKind::Lambda {
                params,
                num_required,
                rest_param,
                vararg_kind,
                captures,
                body,
                num_locals,
                inferred_signals,
                param_bounds,
                doc,
                syntax,
                assert_numeric,
            } => self.lower_lambda_expr(
                params,
                *num_required,
                rest_param.as_ref(),
                vararg_kind,
                captures,
                body,
                *num_locals,
                inferred_signals,
                param_bounds,
                doc.clone(),
                syntax.clone(),
                *assert_numeric,
            ),

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.lower_if(cond, then_branch, else_branch),

            HirKind::Begin(exprs) => self.lower_begin(exprs),
            HirKind::Block { block_id, body, .. } => self.lower_block(block_id, body, hir.id),
            HirKind::Break { block_id, value } => self.lower_break(block_id, value),

            HirKind::Call {
                func,
                args,
                is_tail,
            } => self.lower_call(func, args.as_slice(), *is_tail, hir.signal.bits),

            HirKind::Assign { target, value } => self.lower_assign(target, value),
            HirKind::Define { binding, value } => self.lower_define(*binding, value),
            HirKind::Destructure {
                pattern,
                value,
                strict,
            } => self.lower_destructure_expr(pattern, value, *strict, &hir.span),

            HirKind::While { cond, body } => self.lower_while(cond, body, hir.id),
            HirKind::Loop { bindings, body } => self.lower_loop(bindings, body, hir.id),
            HirKind::Recur { args } => self.lower_recur(args),

            HirKind::And(exprs) => self.lower_and(exprs),
            HirKind::Or(exprs) => self.lower_or(exprs),

            HirKind::Emit { signal, value } => self.lower_emit(*signal, value),
            HirKind::Quote(value) => self.emit_value_const(*value),
            HirKind::QuoteConst(template) => {
                // Quoted compound data is an ordinary allocation: materialize a
                // FRESH structure from the template into this literal's OWN
                // solver-assigned region each execution (docs/impl/region-model.md
                // § "Constants lower as ordinary allocations"). `emit_alloc` stamps the
                // region (arming its `DecrefRegion` at `decref_point`), exactly
                // like `HirKind::String`.
                let dst = self.fresh_reg();
                let template = template.clone();
                self.emit_alloc(|region| LirInstr::MaterializeConst {
                    dst,
                    template,
                    region,
                });
                Ok(dst)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => self.lower_cond(clauses, else_branch),

            HirKind::Match { value, arms } => self.lower_match(value, arms),
            HirKind::Eval { expr, env } => self.lower_eval(expr, env),
            HirKind::Parameterize { bindings, body } => self.lower_parameterize(bindings, body),

            HirKind::MakeCell { value } => self.lower_make_cell(value),
            HirKind::DerefCell { cell } => self.lower_deref_cell(cell),
            HirKind::SetCell { cell, value } => self.lower_set_cell(cell, value),

            HirKind::Intrinsic { op, args } => self.lower_intrinsic(*op, args),

            HirKind::Return { value } => self.lower_return(value),

            HirKind::Error => Err(format!(
                "internal: error poison node in lowerer at {}",
                hir.span
            )),
        };

        // Emit IncrefRegion for cross-region references at this node,
        // then DecrefRegion for every region whose `decref_point` HirId is
        // this node: the lowerer is driven by per-region last-use, not by
        // scope exits.
        if let Ok(result_reg) = result {
            self.emit_increfs_for(hir.id);
            // A caller may defer this node's decrefs to emit them itself at
            // a better point (e.g. `lower_let` emits a binding init's decref
            // only after storing the init value into the slot the decref
            // reloads — otherwise it decrefs the slot's stamped `nil` and the
            // value leaks).
            if !self.deferred_decref_points.contains(&hir.id) {
                self.emit_decrefs_for(hir.id, Some(result_reg));
            }
            // Per-arm sibling-arm releases (used-in-multiple-arms): after the
            // node's own decrefs, so the release follows the arm's use of the value.
            self.emit_arm_decrefs(hir.id);
        }

        self.current_span = saved_span;
        self.current_hir_id = saved_hir_id;
        result
    }

    fn lower_var(&mut self, binding: &Binding, span: &Span) -> Result<Reg, String> {
        // Check immutable_values first — primitive bindings and immutable
        // globals with literal values are compiled to LoadConst without
        // needing a slot allocation.
        if let Some(&literal_value) = self.immutable_values.get(binding) {
            return self.emit_value_const(literal_value);
        }

        // A reference to the enclosing lambda's own self-recursive binding resolves
        // to the executing closure (`LoadSelf`), not a load of its forward cell — in
        // EVERY position. In value position the closure is materialized and used; in
        // call position the callee IS the executing closure, so the call re-enters the
        // same code+env with new args (self-call re-dispatch). Both are RC-identical to
        // the forward-cell load without naming the cell. `current_self_binding` is set
        // only inside that lambda's body, and only for a same-binding self-edge (a
        // sibling/foreign capture stays `Local`/`Capture` and keeps its cell for the
        // closure-cycle merge), so this fires for exactly the self-references.
        if self.current_self_binding == Some(*binding) {
            let dst = self.fresh_reg();
            self.emit(LirInstr::LoadSelf { dst });
            return Ok(dst);
        }

        if let Some(&slot) = self.binding_to_slot.get(binding) {
            // Check if this binding needs cell unwrapping
            let needs_capture = self.arena.get(*binding).needs_capture();

            // Check if this is an upvalue (capture or parameter) or a local
            let is_upvalue = self.upvalue_bindings.contains(binding);

            let dst = self.fresh_reg();
            if self.in_lambda && is_upvalue {
                if needs_capture {
                    self.emit(LirInstr::LoadCapture { dst, index: slot });
                } else {
                    self.emit(LirInstr::LoadCaptureRaw { dst, index: slot });
                }
                Ok(dst)
            } else {
                // A plain stack slot: every non-upvalue binding — outside
                // lambdas, and in-lambda for a compiled-cell letrec binding
                // (letrec_compiled_cell) whose slot holds the MakeCaptureCell.
                self.emit(LirInstr::LoadLocal { dst, slot });

                if needs_capture {
                    // Unwrap the cell to get the actual value
                    // Only needed for locals, not captures (LoadCapture auto-unwraps)
                    let value_reg = self.fresh_reg();
                    self.emit(LirInstr::LoadCaptureCell {
                        dst: value_reg,
                        cell: dst,
                    });
                    Ok(value_reg)
                } else {
                    Ok(dst)
                }
            }
        } else {
            // Binding not found in immutable_values or binding_to_slot.
            // This happens when the analyzer's resolve_primitive fallback
            // creates a dangling binding for an undefined variable.
            let sym_id = self.arena.get(*binding).name;
            let name = self
                .symbol_names
                .get(&sym_id.0)
                .cloned()
                .unwrap_or_else(|| format!("symbol #{}", sym_id.0));
            Err(format!("{}: undefined variable: {}", span, name))
        }
    }

    fn lower_if(
        &mut self,
        cond: &Hir,
        then_branch: &Hir,
        else_branch: &Hir,
    ) -> Result<Reg, String> {
        let cond_reg = self.lower_expr(cond)?;

        // Allocate result slot (same pattern as lower_cond)
        let result_reg = self.fresh_reg();
        let result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;

        let then_label = self.fresh_label();
        let else_label = self.fresh_label();
        let merge_label = self.fresh_label();

        // Terminate current block with branch
        self.terminate(Terminator::Branch {
            cond: cond_reg,
            then_label,
            else_label,
        });
        self.finish_block();

        // Then block: store result to slot, jump to merge
        self.current_block = BasicBlock::new(then_label);
        let then_reg = self.lower_expr(then_branch)?;
        self.emit(LirInstr::StoreLocal {
            slot: result_slot,
            src: then_reg,
        });
        self.terminate(Terminator::Jump(merge_label));
        self.finish_block();

        // Else block: store result to slot, jump to merge
        self.current_block = BasicBlock::new(else_label);
        let else_reg = self.lower_expr(else_branch)?;
        self.emit(LirInstr::StoreLocal {
            slot: result_slot,
            src: else_reg,
        });
        self.terminate(Terminator::Jump(merge_label));
        self.finish_block();

        // Merge block: load result from slot
        self.current_block = BasicBlock::new(merge_label);
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

        Ok(result_reg)
    }

    /// Collect Define and Destructure bindings reachable through
    /// structural wrappings (Let, Begin, Loop, Block) without crossing
    /// Lambda or branching boundaries (If, Match, Cond). Used by the
    /// Begin pre-pass to pre-allocate slots for mutual recursion.
    ///
    /// Only scans through Let/Begin/Loop/Block — these are structural
    /// wrappers. Does NOT scan into If/Match/Cond because different
    /// branches may define bindings with overlapping slot allocation.
    fn collect_preallocate_bindings(hir: &Hir, out: &mut Vec<Binding>) {
        match &hir.kind {
            HirKind::Define { binding, .. } => out.push(*binding),
            HirKind::Destructure { pattern, .. } => out.extend(pattern.bindings().bindings),
            HirKind::Lambda { .. } => {}
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (_, init) in bindings {
                    Self::collect_preallocate_bindings(init, out);
                }
                Self::collect_preallocate_bindings(body, out);
            }
            HirKind::Begin(exprs) => {
                for e in exprs {
                    Self::collect_preallocate_bindings(e, out);
                }
            }
            HirKind::Loop { bindings, body } => {
                for (_, init) in bindings {
                    Self::collect_preallocate_bindings(init, out);
                }
                Self::collect_preallocate_bindings(body, out);
            }
            HirKind::Block { body, .. } => {
                for e in body {
                    Self::collect_preallocate_bindings(e, out);
                }
            }
            _ => {}
        }
    }

    fn lower_begin(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        // Pre-allocate slots for all local Define and Destructure bindings
        // reachable from this Begin (including inside Let/Loop/If bodies
        // but NOT inside Lambdas). This enables mutual recursion where
        // lambda A captures variable B before B's Define has been lowered.
        let mut bindings_to_preallocate = Vec::new();
        for expr in exprs {
            Self::collect_preallocate_bindings(expr, &mut bindings_to_preallocate);
        }
        for &binding in &bindings_to_preallocate {
            // Allocate slot now so captures can find it
            if !self.binding_to_slot.contains_key(&binding) {
                let needs_capture = self.arena.get(binding).needs_capture();
                let slot = self.allocate_slot(binding);

                // Inside lambdas, only LBox locals live in the closure
                // environment (LoadCapture/StoreCapture). Non-LBox locals
                // use fast local storage (LoadLocal/StoreLocal).
                if self.in_lambda && needs_capture {
                    self.upvalue_bindings.insert(binding);
                }

                // Only create cells for top-level locals (outside lambdas)
                // Inside lambdas, the VM creates cells for locally-defined variables
                // when building the closure environment
                if needs_capture && !self.in_lambda {
                    // Create a cell containing nil
                    // This cell will be captured by nested lambdas
                    // and updated when the Define is lowered.
                    // One region PER cell (`begin_cell_regions`): emitting all
                    // cells against this Begin's single slot orphans all but
                    // the last minted physical region — the shared-slot
                    // capture-cell leak (docs/impl/region-model.md, "one allocation
                    // execution per slot between drops").
                    let region = self.cell_region_for(binding);
                    let nil_reg = self.emit_const(LirConst::Nil)?;
                    let cell_reg = self.fresh_reg();
                    self.emit_alloc_in(region, |region| LirInstr::MakeCaptureCell {
                        region,
                        dst: cell_reg,
                        value: nil_reg,
                    });
                    self.emit(LirInstr::StoreLocal {
                        slot,
                        src: cell_reg,
                    });
                }
            }
        }
        // Now lower all expressions (slots are available for capture lookup)
        // Pop intermediate results to keep the stack clean
        if exprs.is_empty() {
            return self.emit_const(LirConst::Nil);
        }

        let mut last_reg = self.lower_expr(&exprs[0])?;
        for expr in exprs.iter().skip(1) {
            self.discard(last_reg);
            last_reg = self.lower_expr(expr)?;
        }
        Ok(last_reg)
    }

    fn lower_block(
        &mut self,
        block_id: &BlockId,
        body: &[Hir],
        hir_id: HirId,
    ) -> Result<Reg, String> {
        let result_reg = self.fresh_reg();
        let block_result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        let exit_label = self.fresh_label();
        let region_id = if self.region_scope_check(hir_id) {
            self.scope_region_id(hir_id)
        } else {
            None
        };

        // Record active_region_ids depth before the block so breaks
        // can emit FreeRegion for regions entered since.
        let region_stack_depth = self.active_region_ids.len();

        if let Some(rid) = region_id {
            self.active_region_ids.push(rid);
        }

        self.block_lower_contexts.push(BlockLowerContext {
            block_id: *block_id,
            result_reg,
            result_slot: block_result_slot,
            exit_label,
            region_depth_at_entry: region_stack_depth as u32,
        });

        // Lower body
        if body.is_empty() {
            let nil_reg = self.emit_const(LirConst::Nil)?;
            self.emit(LirInstr::StoreLocal {
                slot: block_result_slot,
                src: nil_reg,
            });
        } else {
            let mut last_reg = self.lower_expr(&body[0])?;
            for expr in body.iter().skip(1) {
                self.discard(last_reg);
                last_reg = self.lower_expr(expr)?;
            }
            self.emit(LirInstr::StoreLocal {
                slot: block_result_slot,
                src: last_reg,
            });
        }

        self.block_lower_contexts.pop();

        // Region-demise DecrefRegion is emitted by `lower_expr` at each
        // region's `decref_point` HirId. This function emits none; it only
        // keeps the active_region_ids bookkeeping so break compensation (if
        // any) can still walk it.
        if region_id.is_some() {
            self.active_region_ids.pop();
        }

        // Normal exit: jump to the exit label
        self.terminate(Terminator::Jump(exit_label));
        self.start_new_block(exit_label);
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: block_result_slot,
        });

        Ok(result_reg)
    }

    /// Lower a `Return` ownership boundary: evaluate the value, then
    /// incref its result region so the caller receives one owning reference.
    /// Region-transparent — returns the value's own register. The mint is
    /// emitted here, before the node's own `emit_decrefs_for` (which the
    /// regions pass arranges to fire at this Return node, after the retain —
    /// see the `return_sites` decref_point extension), so a freshly-allocated
    /// result region survives its callee-side release.
    ///
    /// Two encodings, chosen by `coalescible_region` (the staticness predicate,
    /// docs/impl/region-rules.md § "Compile-time region selection (coalescing)"):
    ///
    /// - **slot-resolved** when the returned value is a fresh local allocation
    ///   whose region is a known static slot — emit the equivalence oracle
    ///   `AssertRegionMatches { slot }` (debug builds only) then
    ///   `IncrefRegion { slot }`. The slot resolves, through the activation map,
    ///   to the same physical region `region_of(value)` would return (the alloc
    ///   stamped it; a value never moves regions), so the RC trajectory is
    ///   bit-identical — one fewer runtime deref, stack-neutral.
    /// - **value-resolved** otherwise (the dynamic boundary — a borrowed
    ///   captured upvalue, a pass-through arg, a branch-dependent mix, an opaque
    ///   call result): emit `IncrefValueRegion { src }`, reading the region from
    ///   the value at runtime.
    ///
    /// Either way the caller balances the mint with a `DecrefValueRegion` at the
    /// result binding's `decref_point` (the caller cannot name the callee's
    /// region — prediction-free — so the substitution is purely callee-mint-side).
    fn lower_return(&mut self, value: &Hir) -> Result<Reg, String> {
        let reg = self.lower_expr(value)?;
        // Transform 1 (docs/impl/region-rules.md § "Compile-time region selection
        // (coalescing)"): when the returned value is a fresh local allocation whose
        // region is a known static slot, the mint is slot-resolved; otherwise the
        // region is a genuine runtime fact (the dynamic boundary) and the mint stays
        // value-resolved. `coalescible_region` is the staticness predicate; this
        // function's doc-comment carries the two encodings and why the caller side
        // is unaffected (prediction-free).
        let slot = self.coalescible_region(value);
        super::rcstats::record_return_mint(slot.is_some());
        if crate::config::get().has_trace("rc") {
            eprintln!(
                "[trace:rc:emit] return_mint hir_id={:?} coalescible={:?} span={}",
                value.id, slot, self.current_span,
            );
        }
        match slot {
            Some(slot) => {
                // The equivalence oracle: assert the slot resolves to the same
                // physical region the value actually lives in — a mis-coalesce
                // would let the cascade free a live region (a UAF). Debug-only; it
                // peeks `reg`, leaving it on top for the following `IncrefRegion`
                // and the `Return`. Release builds omit it entirely, so the
                // bytecode never carries it (C0 emit contract).
                #[cfg(debug_assertions)]
                self.emit(LirInstr::AssertRegionMatches {
                    region_id: slot,
                    src: reg,
                });
                self.emit(LirInstr::IncrefRegion { region_id: slot });
            }
            None => self.emit(LirInstr::IncrefValueRegion { src: reg }),
        }
        Ok(reg)
    }

    fn lower_parameterize(&mut self, bindings: &[(Hir, Hir)], body: &Hir) -> Result<Reg, String> {
        // Lower all param/value pairs
        let mut pairs = Vec::new();
        for (param, value) in bindings {
            let param_reg = self.lower_expr(param)?;
            let value_reg = self.lower_expr(value)?;
            pairs.push((param_reg, value_reg));
        }

        // Emit PushParamFrame
        self.emit(LirInstr::PushParamFrame { pairs });

        // Lower body
        let body_reg = self.lower_expr(body)?;

        // Store result in a local slot so PopParamFrame doesn't interfere
        let result_reg = self.fresh_reg();
        let result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        self.emit(LirInstr::StoreLocal {
            slot: result_slot,
            src: body_reg,
        });

        // Emit PopParamFrame
        self.emit(LirInstr::PopParamFrame);

        // Reload result
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

        Ok(result_reg)
    }
}
