//! Expression lowering - the main `lower_expr` dispatch

use super::*;

impl Lowerer {
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

            HirKind::Var(binding) => self.lower_var(binding),
            HirKind::Let { bindings, body } => self.lower_let(bindings, body),
            HirKind::Letrec { bindings, body } => self.lower_letrec(bindings, body),
            HirKind::Lambda {
                params,
                num_required,
                rest_param,
                vararg_kind,
                captures,
                body,
                num_locals,
                inferred_effects,
                param_bounds,
                doc,
                syntax,
            } => self.lower_lambda_expr(
                params,
                *num_required,
                rest_param.as_ref(),
                vararg_kind,
                captures,
                body,
                *num_locals,
                inferred_effects,
                param_bounds,
                *doc,
                syntax.clone(),
            ),

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.lower_if(cond, then_branch, else_branch),

            HirKind::Begin(exprs) => self.lower_begin(exprs),
            HirKind::Block { block_id, body, .. } => self.lower_block(block_id, body),
            HirKind::Break { block_id, value } => self.lower_break(block_id, value),

            HirKind::Call {
                func,
                args,
                is_tail,
            } => self.lower_call(func, args.as_slice(), *is_tail),

            HirKind::Assign { target, value } => self.lower_assign(target, value),
            HirKind::Define { binding, value } => self.lower_define(*binding, value),
            HirKind::Destructure { pattern, value } => {
                self.lower_destructure_expr(pattern, value, &hir.span)
            }

            HirKind::While { cond, body } => self.lower_while(cond, body),

            HirKind::And(exprs) => self.lower_and(exprs),
            HirKind::Or(exprs) => self.lower_or(exprs),

            HirKind::Yield(value) => self.lower_yield(value),
            HirKind::Quote(value) => self.emit_value_const(*value),
            HirKind::Cond {
                clauses,
                else_branch,
            } => self.lower_cond(clauses, else_branch),

            HirKind::Match { value, arms } => self.lower_match(value, arms),
            HirKind::Eval { expr, env } => self.lower_eval(expr, env),
            HirKind::Parameterize { bindings, body } => self.lower_parameterize(bindings, body),
        }
    }

    fn lower_var(&mut self, binding: &Binding) -> Result<Reg, String> {
        // Check immutable_values first — primitive bindings and immutable
        // globals with literal values are compiled to LoadConst without
        // needing a slot allocation.
        if let Some(&literal_value) = self.immutable_values.get(binding) {
            return self.emit_value_const(literal_value);
        }

        if let Some(&slot) = self.binding_to_slot.get(binding) {
            // Check if this binding needs cell unwrapping
            let needs_lbox = binding.needs_lbox();

            // Check if this is an upvalue (capture or parameter) or a local
            let is_upvalue = self.upvalue_bindings.contains(binding);

            let dst = self.fresh_reg();
            if self.in_lambda && is_upvalue {
                // In a lambda, captures, parameters, and locally-defined variables are accessed via LoadCapture
                // Note: LoadCapture (which emits LoadUpvalue) auto-unwraps LocalCell,
                // so we don't need to emit LoadLBox for captured variables
                self.emit(LirInstr::LoadCapture { dst, index: slot });
                Ok(dst)
            } else {
                // Outside lambdas, local variables use LoadLocal
                self.emit(LirInstr::LoadLocal { dst, slot });

                if needs_lbox {
                    // Unwrap the cell to get the actual value
                    // Only needed for locals, not captures (LoadCapture auto-unwraps)
                    let value_reg = self.fresh_reg();
                    self.emit(LirInstr::LoadLBox {
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
            let sym_id = binding.name();
            let name = self
                .symbol_names
                .get(&sym_id.0)
                .cloned()
                .unwrap_or_else(|| format!("symbol #{}", sym_id.0));
            Err(format!("undefined variable: {}", name))
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

    fn lower_begin(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        // Pre-allocate slots for all local Define and Destructure bindings
        // This enables mutual recursion where lambda A captures variable B
        // before B's Define has been lowered
        for expr in exprs {
            let bindings_to_preallocate: Vec<Binding> = match &expr.kind {
                HirKind::Define { binding, .. } => vec![*binding],
                HirKind::Destructure { pattern, .. } => pattern.bindings().bindings,
                _ => continue,
            };
            for binding in bindings_to_preallocate {
                // Allocate slot now so captures can find it
                if !self.binding_to_slot.contains_key(&binding) {
                    let slot = self.allocate_slot(binding);

                    // Inside lambdas, local variables are part of the closure environment
                    if self.in_lambda {
                        self.upvalue_bindings.insert(binding);
                    }

                    // Check if this binding needs a cell
                    let needs_lbox = binding.needs_lbox();

                    // Only create cells for top-level locals (outside lambdas)
                    // Inside lambdas, the VM creates cells for locally-defined variables
                    // when building the closure environment
                    if needs_lbox && !self.in_lambda {
                        // Create a cell containing nil
                        // This cell will be captured by nested lambdas
                        // and updated when the Define is lowered
                        let nil_reg = self.emit_const(LirConst::Nil)?;
                        let cell_reg = self.fresh_reg();
                        self.emit(LirInstr::MakeLBox {
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
        }

        // Now lower all expressions (slots are available for capture lookup)
        // Pop intermediate results to keep the stack clean
        if exprs.is_empty() {
            return self.emit_const(LirConst::Nil);
        }
        let mut last_reg = self.lower_expr(&exprs[0])?;
        for expr in exprs.iter().skip(1) {
            // Discard the previous result before evaluating the next expression
            self.discard(last_reg);
            last_reg = self.lower_expr(expr)?;
        }
        Ok(last_reg)
    }

    fn lower_block(&mut self, block_id: &BlockId, body: &[Hir]) -> Result<Reg, String> {
        let result_reg = self.fresh_reg();
        let block_result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        let exit_label = self.fresh_label();
        let scoped = self.can_scope_allocate_block(block_id, body);

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

        // Lower the value expression
        let value_reg = self.lower_expr(value)?;

        // Store value to the block's result slot
        self.emit(LirInstr::StoreLocal {
            slot: target_result_slot,
            src: value_reg,
        });

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

    fn lower_while(&mut self, cond: &Hir, body: &Hir) -> Result<Reg, String> {
        let result_reg = self.fresh_reg();

        let cond_label = self.fresh_label();
        let body_label = self.fresh_label();
        let done_label = self.fresh_label();

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

        // Body block
        self.current_block = BasicBlock::new(body_label);
        let _body_reg = self.lower_expr(body)?;
        self.terminate(Terminator::Jump(cond_label));
        self.finish_block();

        // Done block — emit nil result here so it's tracked in this block
        self.current_block = BasicBlock::new(done_label);
        self.emit(LirInstr::Const {
            dst: result_reg,
            value: LirConst::Nil,
        });
        Ok(result_reg)
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
