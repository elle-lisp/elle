//! Expression lowering - the main `lower_expr` dispatch

use super::*;

impl<'a> Lowerer<'a> {
    /// Lower a HIR expression to LIR
    pub(super) fn lower_expr(&mut self, hir: &Hir) -> Result<Reg, String> {
        // Set the current span for all instructions emitted while lowering this HIR node
        self.current_span = hir.span.clone();

        match &hir.kind {
            HirKind::Nil => self.emit_const(LirConst::Nil),
            HirKind::EmptyList => self.emit_const(LirConst::EmptyList),
            HirKind::Bool(b) => self.emit_const(LirConst::Bool(*b)),
            HirKind::Int(n) => self.emit_const(LirConst::Int(*n)),
            HirKind::Float(f) => self.emit_const(LirConst::Float(*f)),
            HirKind::String(s) => self.emit_const(LirConst::String(s.clone())),
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
                *doc,
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

            HirKind::Error => Err(format!(
                "internal: error poison node in lowerer at {}",
                hir.span
            )),
        }
    }

    fn lower_var(&mut self, binding: &Binding, span: &Span) -> Result<Reg, String> {
        // Check immutable_values first — primitive bindings and immutable
        // globals with literal values are compiled to LoadConst without
        // needing a slot allocation.
        if let Some(&literal_value) = self.immutable_values.get(binding) {
            return self.emit_value_const(literal_value);
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
                // Outside lambdas, local variables use LoadLocal
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
                    // and updated when the Define is lowered
                    let nil_reg = self.emit_const(LirConst::Nil)?;
                    let cell_reg = self.fresh_reg();
                    self.emit(LirInstr::MakeCaptureCell {
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
        let scoped = self.region_scope_check(hir_id);

        // Record region depth BEFORE emitting RegionEnter so that breaks
        // targeting this block include the block's own region in their
        // compensating RegionExit count.
        let depth_before = self.region_depth;

        if scoped {
            self.emit_region_enter();
        }

        self.block_lower_contexts.push(BlockLowerContext {
            block_id: *block_id,
            result_reg,
            result_slot: block_result_slot,
            exit_label,
            region_depth_at_entry: depth_before,
            flip_depth_at_entry: self.flip_depth,
        });

        // Lower body (same as lower_begin but simpler — body is typically a single Begin node)
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

        if scoped {
            self.emit_region_exit();
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

    fn lower_break(&mut self, block_id: &BlockId, value: &Hir) -> Result<Reg, String> {
        // Find the target block context
        let target = self
            .block_lower_contexts
            .iter()
            .rev()
            .find(|ctx| ctx.block_id == *block_id)
            .ok_or_else(|| format!("Internal error: no block context for {:?}", block_id))?;

        let target_result_slot = target.result_slot;
        let target_exit_label = target.exit_label;
        let target_region_depth = target.region_depth_at_entry;
        let target_flip_depth = target.flip_depth_at_entry;

        // Lower the value expression
        let value_reg = self.lower_expr(value)?;

        // Store value to the block's result slot
        self.emit(LirInstr::StoreLocal {
            slot: target_result_slot,
            src: value_reg,
        });

        // Emit compensating FlipExit for each while-loop flip frame
        // entered since the target block was opened.
        let compensating_flips = self.flip_depth - target_flip_depth;
        for _ in 0..compensating_flips {
            self.emit(LirInstr::FlipExit);
        }

        // Emit compensating RegionExit for each region entered since the
        // target block was opened. This ensures scope marks are popped
        // correctly on early exit.
        let compensating_exits = self.region_depth - target_region_depth;
        for _ in 0..compensating_exits {
            self.emit(LirInstr::RegionExit);
        }
        // Note: we emit raw RegionExit (not emit_region_exit) because we
        // don't want to decrement region_depth — the break jumps out of
        // the block entirely, and the dead code after the break is
        // unreachable. The block's RegionExit at the normal exit path
        // handles the depth bookkeeping for the normal flow.

        self.terminate(Terminator::Jump(target_exit_label));

        // Start a new (unreachable) block for any dead code after the break
        let dead_label = self.fresh_label();
        self.start_new_block(dead_label);

        // Return a dummy register (code after break is dead)
        Ok(self.fresh_reg())
    }

    fn lower_while(&mut self, cond: &Hir, body: &Hir, _hir_id: HirId) -> Result<Reg, String> {
        let result_reg = self.fresh_reg();
        let flip_eligible = self.can_flip_while_loop(body, &[]);
        // All flip-eligible loops get double-buffered scope marks.
        let scope_eligible = flip_eligible;
        let dealloc_eligible = scope_eligible && self.can_dealloc_in_loop(body, &[]);

        let cond_label = self.fresh_label();
        let body_label = self.fresh_label();
        let done_label = self.fresh_label();

        // The entry block is the current block before we jump to cond.
        let entry_label = self.current_block.label;

        // Double-buffered scope marks: push prev (guard) + curr before loop.
        if scope_eligible {
            self.emit_region_enter(); // prev (guard mark)
            self.emit_region_enter(); // curr (first iteration)
        }

        // Jump to condition check
        self.terminate(Terminator::Jump(cond_label));
        self.finish_block();

        // Condition block
        self.current_block = BasicBlock::new(cond_label);
        let cond_reg = self.lower_expr(cond)?;
        self.terminate(Terminator::Branch {
            cond: cond_reg,
            then_label: body_label,
            else_label: done_label,
        });
        self.finish_block();

        // Body block — track flip_depth and region_depth so breaks can compensate
        if flip_eligible {
            self.flip_depth += 1;
        }
        self.current_block = BasicBlock::new(body_label);

        let _body_reg = self.lower_expr(body)?;

        // Back-edge: rotate scope marks (free prev iteration, start new curr)
        if scope_eligible {
            if dealloc_eligible {
                self.emit_region_rotate_dealloc();
            } else {
                self.emit_region_rotate();
            }
        }

        // The back-edge block is whatever block we're in after lowering
        // the body (body lowering may have created intermediate blocks).
        let back_edge_label = self.current_block.label;
        self.terminate(Terminator::Jump(cond_label));
        self.finish_block();
        if flip_eligible {
            self.flip_depth -= 1;
        }

        // Record the loop triple for inject_flip to use later.
        if flip_eligible {
            self.current_func
                .while_loops
                .push((entry_label, back_edge_label, done_label));
        }

        // Done block — release both scope marks (curr + prev)
        self.current_block = BasicBlock::new(done_label);
        if scope_eligible {
            self.emit_region_exit(); // curr
            self.emit_region_exit(); // prev
        }
        self.emit(LirInstr::Const {
            dst: result_reg,
            value: LirConst::Nil,
        });
        Ok(result_reg)
    }

    fn lower_loop(
        &mut self,
        bindings: &[(Binding, Hir)],
        body: &Hir,
        _hir_id: HirId,
    ) -> Result<Reg, String> {
        let result_reg = self.fresh_reg();
        let loop_scope: Vec<(Binding, &Hir)> = bindings.iter().map(|(b, h)| (*b, h)).collect();
        let flip_eligible = self.can_flip_while_loop(body, &loop_scope);
        // All flip-eligible loops get double-buffered scope marks.
        let scope_eligible = flip_eligible;
        let dealloc_eligible = scope_eligible && self.can_dealloc_in_loop(body, &loop_scope);

        let loop_label = self.fresh_label();
        let done_label = self.fresh_label();

        let entry_label = self.current_block.label;

        // Initialize loop bindings
        let mut binding_slots = Vec::new();
        for (binding, init) in bindings {
            let init_reg = self.lower_expr(init)?;
            let slot = self.allocate_slot(*binding);
            self.emit(LirInstr::StoreLocal {
                slot,
                src: init_reg,
            });
            binding_slots.push(slot);
        }

        // Double-buffered scope marks: push prev (guard) + curr before loop.
        if scope_eligible {
            self.emit_region_enter(); // prev (guard mark)
            self.emit_region_enter(); // curr (first iteration)
        }

        // Jump to loop header
        self.terminate(Terminator::Jump(loop_label));
        self.finish_block();

        // Loop body
        if flip_eligible {
            self.flip_depth += 1;
        }
        self.current_block = BasicBlock::new(loop_label);

        // Save depth counters — Recur emits RegionRotate which doesn't
        // change region_depth, but the normal exit path needs original depths.
        let saved_region_depth = self.region_depth;
        let saved_flip_depth = self.flip_depth;

        // Push loop context so Recur can find us
        self.loop_lower_contexts.push(LoopLowerContext {
            loop_label,
            binding_slots: binding_slots.clone(),
            scope_eligible,
            dealloc_eligible,
        });

        let body_reg = self.lower_expr(body)?;

        self.loop_lower_contexts.pop();

        // Restore depth counters for normal exit path
        self.region_depth = saved_region_depth;
        self.flip_depth = saved_flip_depth;

        // If we reach here (no Recur), body_reg is the loop result.
        let result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        self.emit(LirInstr::StoreLocal {
            slot: result_slot,
            src: body_reg,
        });

        // Release both scope marks (curr + prev)
        if scope_eligible {
            self.emit_region_exit(); // curr
            self.emit_region_exit(); // prev
        }

        let back_edge_label = self.current_block.label;
        self.terminate(Terminator::Jump(done_label));
        self.finish_block();

        if flip_eligible {
            self.flip_depth -= 1;
            self.current_func
                .while_loops
                .push((entry_label, back_edge_label, done_label));
        }

        // Done block — load result from slot
        self.current_block = BasicBlock::new(done_label);
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });
        Ok(result_reg)
    }

    fn lower_recur(&mut self, args: &[Hir]) -> Result<Reg, String> {
        let ctx = self
            .loop_lower_contexts
            .last()
            .ok_or_else(|| "recur outside of loop".to_string())?;

        let loop_label = ctx.loop_label;
        let binding_slots = ctx.binding_slots.clone();
        let scope_eligible = ctx.scope_eligible;
        let dealloc_eligible = ctx.dealloc_eligible;

        if args.len() != binding_slots.len() {
            return Err(format!(
                "recur: expected {} arguments, got {}",
                binding_slots.len(),
                args.len()
            ));
        }

        // Evaluate all args before storing (avoid order-dependent overwrites)
        let mut arg_regs = Vec::with_capacity(args.len());
        for arg in args {
            arg_regs.push(self.lower_expr(arg)?);
        }

        // Store new values to loop binding slots BEFORE rotating scope marks.
        // With double-buffered marks, RegionRotate frees the PREVIOUS
        // iteration's allocs, not the current one — so recur arg values
        // survive the rotation even if they reference current-iteration allocs.
        for (reg, &slot) in arg_regs.iter().zip(&binding_slots) {
            self.emit(LirInstr::StoreLocal { slot, src: *reg });
        }

        // Rotate scope marks: free prev iteration, start new curr
        if scope_eligible {
            if dealloc_eligible {
                self.emit_region_rotate_dealloc();
            } else {
                self.emit_region_rotate();
            }
        }

        // Jump back to loop header
        self.terminate(Terminator::Jump(loop_label));
        self.finish_block();

        // Dead block after unconditional jump
        let dead_label = self.fresh_label();
        self.current_block = BasicBlock::new(dead_label);
        let nil_reg = self.emit_const(LirConst::Nil)?;
        Ok(nil_reg)
    }

    fn lower_cond(
        &mut self,
        clauses: &[(Hir, Hir)],
        else_branch: &Option<Box<Hir>>,
    ) -> Result<Reg, String> {
        if clauses.is_empty() {
            return if let Some(else_expr) = else_branch {
                self.lower_expr(else_expr)
            } else {
                self.emit_const(LirConst::Nil)
            };
        }

        let result_reg = self.fresh_reg();
        let cond_result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        let done_label = self.fresh_label();

        // Generate labels for each clause's body and the next test
        let mut clause_labels: Vec<(Label, Label)> = Vec::new();
        for _ in clauses {
            let body_label = self.fresh_label();
            let test_label = self.fresh_label();
            clause_labels.push((body_label, test_label));
        }
        let else_label = self.fresh_label();

        // Process each clause
        for (i, (test, body)) in clauses.iter().enumerate() {
            let (body_label, _) = clause_labels[i];

            // Test block (current block for first clause, or test_label for subsequent)
            let test_reg = self.lower_expr(test)?;

            // Determine where to jump if test fails
            let fail_label = if i + 1 < clauses.len() {
                clause_labels[i + 1].1 // Next clause's test label
            } else {
                else_label
            };

            // Branch to body_label if true, fail_label if false
            self.terminate(Terminator::Branch {
                cond: test_reg,
                then_label: body_label,
                else_label: fail_label,
            });
            self.finish_block();

            // Body block
            self.current_block = BasicBlock::new(body_label);
            let body_reg = self.lower_expr(body)?;
            self.emit(LirInstr::StoreLocal {
                slot: cond_result_slot,
                src: body_reg,
            });
            self.terminate(Terminator::Jump(done_label));
            self.finish_block();

            // Start next test block (if not last clause)
            if i + 1 < clauses.len() {
                self.current_block = BasicBlock::new(clause_labels[i + 1].1);
            }
        }

        // Else block
        self.current_block = BasicBlock::new(else_label);
        if let Some(else_expr) = else_branch {
            let else_reg = self.lower_expr(else_expr)?;
            self.emit(LirInstr::StoreLocal {
                slot: cond_result_slot,
                src: else_reg,
            });
        } else {
            let nil_reg = self.emit_const(LirConst::Nil)?;
            self.emit(LirInstr::StoreLocal {
                slot: cond_result_slot,
                src: nil_reg,
            });
        }
        self.terminate(Terminator::Jump(done_label));
        self.finish_block();

        // Done block (continue here)
        self.current_block = BasicBlock::new(done_label);
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: cond_result_slot,
        });

        Ok(result_reg)
    }

    fn lower_intrinsic(
        &mut self,
        op: crate::hir::IntrinsicOp,
        args: &[Hir],
    ) -> Result<Reg, String> {
        use crate::hir::IntrinsicOp;

        // Lower all arguments first
        let mut arg_regs = Vec::with_capacity(args.len());
        for arg in args {
            arg_regs.push(self.lower_expr(arg)?);
        }

        let dst = self.fresh_reg();
        match op {
            // Binary arithmetic
            IntrinsicOp::Add => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Add,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Sub => {
                if arg_regs.len() == 1 {
                    self.emit(LirInstr::UnaryOp {
                        dst,
                        op: UnaryOp::Neg,
                        src: arg_regs[0],
                    });
                } else {
                    self.emit(LirInstr::BinOp {
                        dst,
                        op: BinOp::Sub,
                        lhs: arg_regs[0],
                        rhs: arg_regs[1],
                    });
                }
            }
            IntrinsicOp::Mul => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Mul,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Div => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Div,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Rem => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Rem,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Mod => {
                // Floored modulus: ((a % b) + b) % b
                // The stack-based emitter consumes registers on use, so spill b
                // to a local slot and reload fresh copies for each operation.
                let b_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: b_slot,
                    src: arg_regs[1],
                });
                // Step 1: t = a % b (uses original arg_regs, but b was consumed by StoreLocal)
                let b1 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: b1,
                    slot: b_slot,
                });
                let t = self.fresh_reg();
                self.emit(LirInstr::BinOp {
                    dst: t,
                    op: BinOp::Rem,
                    lhs: arg_regs[0],
                    rhs: b1,
                });
                // Step 2: t2 = t + b
                let b2 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: b2,
                    slot: b_slot,
                });
                let t2 = self.fresh_reg();
                self.emit(LirInstr::BinOp {
                    dst: t2,
                    op: BinOp::Add,
                    lhs: t,
                    rhs: b2,
                });
                // Step 3: result = t2 % b
                let b3 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: b3,
                    slot: b_slot,
                });
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Rem,
                    lhs: t2,
                    rhs: b3,
                });
            }
            // Comparisons
            IntrinsicOp::Eq => {
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Eq,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Lt => {
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Lt,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Gt => {
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Gt,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Le => {
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Le,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Ge => {
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Ge,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            // Logical
            IntrinsicOp::Not => {
                self.emit(LirInstr::UnaryOp {
                    dst,
                    op: UnaryOp::Not,
                    src: arg_regs[0],
                });
            }
            // Conversion
            IntrinsicOp::Int => {
                self.emit(LirInstr::Convert {
                    dst,
                    op: ConvOp::FloatToInt,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::Float => {
                self.emit(LirInstr::Convert {
                    dst,
                    op: ConvOp::IntToFloat,
                    src: arg_regs[0],
                });
            }
            // List operations
            IntrinsicOp::Pair => {
                self.emit(LirInstr::List {
                    dst,
                    head: arg_regs[0],
                    tail: arg_regs[1],
                });
            }
            IntrinsicOp::First => {
                self.emit(LirInstr::First {
                    dst,
                    pair: arg_regs[0],
                });
            }
            IntrinsicOp::Rest => {
                self.emit(LirInstr::Rest {
                    dst,
                    pair: arg_regs[0],
                });
            }
            // Bitwise
            IntrinsicOp::BitAnd => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::BitAnd,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::BitOr => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::BitOr,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::BitXor => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::BitXor,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Shl => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Shl,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Shr => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Shr,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            // Bitwise NOT
            IntrinsicOp::BitNot => {
                self.emit(LirInstr::UnaryOp {
                    dst,
                    op: UnaryOp::BitNot,
                    src: arg_regs[0],
                });
            }
            // Not-equal comparison
            IntrinsicOp::Ne => {
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Ne,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            // Type predicates
            IntrinsicOp::IsNil => {
                self.emit(LirInstr::IsNil {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsEmpty => {
                self.emit(LirInstr::IsEmpty {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsBool => {
                self.emit(LirInstr::IsBool {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsInt => {
                self.emit(LirInstr::IsInt {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsFloat => {
                self.emit(LirInstr::IsFloat {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsString => {
                self.emit(LirInstr::IsString {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsKeyword => {
                self.emit(LirInstr::IsKeyword {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsSymbol => {
                self.emit(LirInstr::IsSymbolCheck {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsPair => {
                self.emit(LirInstr::IsPair {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsArray => {
                // %array? checks both immutable and mutable arrays.
                // Spill the source to a local so both checks can read it
                // (the stack-based emitter consumes the value on first use).
                let src_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: src_slot,
                    src: arg_regs[0],
                });
                let src1 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: src1,
                    slot: src_slot,
                });
                let imm = self.fresh_reg();
                self.emit(LirInstr::IsArray {
                    dst: imm,
                    src: src1,
                });
                let result_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                let then_label = self.fresh_label();
                let else_label = self.fresh_label();
                let merge_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: imm,
                    then_label,
                    else_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(then_label);
                let true_reg = self.emit_const(LirConst::Bool(true))?;
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: true_reg,
                });
                self.terminate(Terminator::Jump(merge_label));
                self.finish_block();
                self.current_block = BasicBlock::new(else_label);
                let src2 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: src2,
                    slot: src_slot,
                });
                let mut_r = self.fresh_reg();
                self.emit(LirInstr::IsArrayMut {
                    dst: mut_r,
                    src: src2,
                });
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: mut_r,
                });
                self.terminate(Terminator::Jump(merge_label));
                self.finish_block();
                self.current_block = BasicBlock::new(merge_label);
                self.emit(LirInstr::LoadLocal {
                    dst,
                    slot: result_slot,
                });
            }
            IntrinsicOp::IsStruct => {
                let src_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: src_slot,
                    src: arg_regs[0],
                });
                let src1 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: src1,
                    slot: src_slot,
                });
                let imm = self.fresh_reg();
                self.emit(LirInstr::IsStruct {
                    dst: imm,
                    src: src1,
                });
                let result_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                let then_label = self.fresh_label();
                let else_label = self.fresh_label();
                let merge_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: imm,
                    then_label,
                    else_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(then_label);
                let true_reg = self.emit_const(LirConst::Bool(true))?;
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: true_reg,
                });
                self.terminate(Terminator::Jump(merge_label));
                self.finish_block();
                self.current_block = BasicBlock::new(else_label);
                let src2 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: src2,
                    slot: src_slot,
                });
                let mut_r = self.fresh_reg();
                self.emit(LirInstr::IsStructMut {
                    dst: mut_r,
                    src: src2,
                });
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: mut_r,
                });
                self.terminate(Terminator::Jump(merge_label));
                self.finish_block();
                self.current_block = BasicBlock::new(merge_label);
                self.emit(LirInstr::LoadLocal {
                    dst,
                    slot: result_slot,
                });
            }
            IntrinsicOp::IsSet => {
                let src_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: src_slot,
                    src: arg_regs[0],
                });
                let src1 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: src1,
                    slot: src_slot,
                });
                let imm = self.fresh_reg();
                self.emit(LirInstr::IsSet {
                    dst: imm,
                    src: src1,
                });
                let result_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                let then_label = self.fresh_label();
                let else_label = self.fresh_label();
                let merge_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: imm,
                    then_label,
                    else_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(then_label);
                let true_reg = self.emit_const(LirConst::Bool(true))?;
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: true_reg,
                });
                self.terminate(Terminator::Jump(merge_label));
                self.finish_block();
                self.current_block = BasicBlock::new(else_label);
                let src2 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: src2,
                    slot: src_slot,
                });
                let mut_r = self.fresh_reg();
                self.emit(LirInstr::IsSetMut {
                    dst: mut_r,
                    src: src2,
                });
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: mut_r,
                });
                self.terminate(Terminator::Jump(merge_label));
                self.finish_block();
                self.current_block = BasicBlock::new(merge_label);
                self.emit(LirInstr::LoadLocal {
                    dst,
                    slot: result_slot,
                });
            }
            IntrinsicOp::IsBytes => {
                self.emit(LirInstr::IsBytes {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsBox => {
                self.emit(LirInstr::IsBox {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsClosure => {
                self.emit(LirInstr::IsClosure {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsFiber => {
                self.emit(LirInstr::IsFiber {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::TypeOf => {
                self.emit(LirInstr::TypeOf {
                    dst,
                    src: arg_regs[0],
                });
            }
            // Data access
            IntrinsicOp::Length => {
                self.emit(LirInstr::Length {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::Get => {
                self.emit(LirInstr::Get {
                    dst,
                    obj: arg_regs[0],
                    key: arg_regs[1],
                });
            }
            IntrinsicOp::Put => {
                self.emit(LirInstr::Put {
                    dst,
                    obj: arg_regs[0],
                    key: arg_regs[1],
                    val: arg_regs[2],
                });
            }
            IntrinsicOp::Del => {
                self.emit(LirInstr::Del {
                    dst,
                    obj: arg_regs[0],
                    key: arg_regs[1],
                });
            }
            IntrinsicOp::Has => {
                self.emit(LirInstr::Has {
                    dst,
                    obj: arg_regs[0],
                    key: arg_regs[1],
                });
            }
            IntrinsicOp::Push => {
                // %push mutates @array in place, returns new array for immutable.
                // Distinct from ArrayMutPush which is splice infrastructure.
                self.emit(LirInstr::IntrPush {
                    dst,
                    array: arg_regs[0],
                    value: arg_regs[1],
                });
            }
            IntrinsicOp::Pop => {
                self.emit(LirInstr::Pop {
                    dst,
                    src: arg_regs[0],
                });
            }
            // Mutability
            IntrinsicOp::Freeze => {
                self.emit(LirInstr::Freeze {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::Thaw => {
                self.emit(LirInstr::Thaw {
                    dst,
                    src: arg_regs[0],
                });
            }
            // Identity
            IntrinsicOp::Identical => {
                self.emit(LirInstr::Identical {
                    dst,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
        }
        Ok(dst)
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
