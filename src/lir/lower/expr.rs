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
                captures,
                body,
                num_locals,
                inferred_effect,
            } => self.lower_lambda_expr(params, captures, body, *num_locals, inferred_effect),

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.lower_if(cond, then_branch, else_branch),

            HirKind::Begin(exprs) => self.lower_begin(exprs),
            HirKind::Block(exprs) => self.lower_block(exprs),

            HirKind::Call {
                func,
                args,
                is_tail,
            } => self.lower_call(func, args, *is_tail),

            HirKind::Set { target, value } => self.lower_set(target, value),
            HirKind::Define { binding, value } => self.lower_define(*binding, value),

            HirKind::While { cond, body } => self.lower_while(cond, body),
            HirKind::For { var, iter, body } => self.lower_for(*var, iter, body),

            HirKind::And(exprs) => self.lower_and(exprs),
            HirKind::Or(exprs) => self.lower_or(exprs),

            HirKind::Yield(value) => self.lower_yield(value),
            HirKind::Quote(value) => self.emit_value_const(*value),
            HirKind::Cond {
                clauses,
                else_branch,
            } => self.lower_cond(clauses, else_branch),

            HirKind::Match { value, arms } => self.lower_match(value, arms),
            HirKind::Module { body, .. } => self.lower_expr(body),
            HirKind::Import { .. } => self.emit_const(LirConst::Nil),
            HirKind::ModuleRef { .. } => self.emit_const(LirConst::Nil),
        }
    }

    fn lower_var(&mut self, binding: &Binding) -> Result<Reg, String> {
        if let Some(&slot) = self.binding_to_slot.get(binding) {
            // Check if this binding needs cell unwrapping
            let needs_cell = binding.needs_cell();

            // Check if this is an upvalue (capture or parameter) or a local
            let is_upvalue = self.upvalue_bindings.contains(binding);

            let dst = self.fresh_reg();
            if self.in_lambda && is_upvalue {
                // In a lambda, captures, parameters, and locally-defined variables are accessed via LoadCapture
                // Note: LoadCapture (which emits LoadUpvalue) auto-unwraps LocalCell,
                // so we don't need to emit LoadCell for captured variables
                self.emit(LirInstr::LoadCapture { dst, index: slot });
                Ok(dst)
            } else {
                // Outside lambdas, local variables use LoadLocal
                self.emit(LirInstr::LoadLocal { dst, slot });

                if needs_cell {
                    // Unwrap the cell to get the actual value
                    // Only needed for locals, not captures (LoadCapture auto-unwraps)
                    let value_reg = self.fresh_reg();
                    self.emit(LirInstr::LoadCell {
                        dst: value_reg,
                        cell: dst,
                    });
                    Ok(value_reg)
                } else {
                    Ok(dst)
                }
            }
        } else if binding.is_global() {
            // Check if this is an immutable binding with a known literal value
            if let Some(&literal_value) = self.immutable_values.get(binding) {
                return self.emit_value_const(literal_value);
            }
            let dst = self.fresh_reg();
            self.emit(LirInstr::LoadGlobal {
                dst,
                sym: binding.name(),
            });
            Ok(dst)
        } else {
            Err(format!("Unknown binding: {:?}", binding))
        }
    }

    fn lower_if(
        &mut self,
        cond: &Hir,
        then_branch: &Hir,
        else_branch: &Hir,
    ) -> Result<Reg, String> {
        let cond_reg = self.lower_expr(cond)?;
        let result_reg = self.fresh_reg();

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

        // Then block
        self.current_block = BasicBlock::new(then_label);
        let then_reg = self.lower_expr(then_branch)?;
        self.emit(LirInstr::Move {
            dst: result_reg,
            src: then_reg,
        });
        self.terminate(Terminator::Jump(merge_label));
        self.finish_block();

        // Else block
        self.current_block = BasicBlock::new(else_label);
        let else_reg = self.lower_expr(else_branch)?;
        self.emit(LirInstr::Move {
            dst: result_reg,
            src: else_reg,
        });
        self.terminate(Terminator::Jump(merge_label));
        self.finish_block();

        // Merge block (continue here)
        self.current_block = BasicBlock::new(merge_label);

        Ok(result_reg)
    }

    fn lower_begin(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        // Pre-allocate slots for all local Define bindings
        // This enables mutual recursion where lambda A captures variable B
        // before B's Define has been lowered
        for expr in exprs {
            if let HirKind::Define { binding, .. } = &expr.kind {
                if binding.is_global() {
                    continue;
                }
                // Allocate slot now so captures can find it
                if !self.binding_to_slot.contains_key(binding) {
                    let slot = self.allocate_slot(*binding);

                    // Inside lambdas, local variables are part of the closure environment
                    if self.in_lambda {
                        self.upvalue_bindings.insert(*binding);
                    }

                    // Check if this binding needs a cell
                    let needs_cell = binding.needs_cell();

                    // Only create cells for top-level locals (outside lambdas)
                    // Inside lambdas, the VM creates cells for locally-defined variables
                    // when building the closure environment
                    if needs_cell && !self.in_lambda {
                        // Create a cell containing nil
                        // This cell will be captured by nested lambdas
                        // and updated when the Define is lowered
                        let nil_reg = self.emit_const(LirConst::Nil)?;
                        let cell_reg = self.fresh_reg();
                        self.emit(LirInstr::MakeCell {
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
            // Pop the previous result before evaluating the next expression
            self.emit(LirInstr::Pop { src: last_reg });
            last_reg = self.lower_expr(expr)?;
        }
        Ok(last_reg)
    }

    fn lower_block(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        // Pop intermediate results to keep the stack clean
        if exprs.is_empty() {
            return self.emit_const(LirConst::Nil);
        }
        let mut last_reg = self.lower_expr(&exprs[0])?;
        for expr in exprs.iter().skip(1) {
            // Pop the previous result before evaluating the next expression
            self.emit(LirInstr::Pop { src: last_reg });
            last_reg = self.lower_expr(expr)?;
        }
        Ok(last_reg)
    }

    fn lower_while(&mut self, cond: &Hir, body: &Hir) -> Result<Reg, String> {
        let result_reg = self.fresh_reg();
        // While returns nil
        self.emit(LirInstr::Const {
            dst: result_reg,
            value: LirConst::Nil,
        });

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

        // Done block
        self.current_block = BasicBlock::new(done_label);
        Ok(result_reg)
    }

    fn lower_for(&mut self, var: Binding, iter: &Hir, body: &Hir) -> Result<Reg, String> {
        // Allocate separate slots for iterator and loop variable
        // Inside a lambda, slots need to account for the captures offset.
        let iter_slot = if self.in_lambda {
            self.num_captures + self.current_func.num_locals
        } else {
            self.current_func.num_locals
        };
        self.current_func.num_locals += 1;

        let var_slot = self.allocate_slot(var);

        // Store initial iterator
        let iter_reg = self.lower_expr(iter)?;
        if self.in_lambda {
            self.emit(LirInstr::StoreCapture {
                index: iter_slot,
                src: iter_reg,
            });
        } else {
            self.emit(LirInstr::StoreLocal {
                slot: iter_slot,
                src: iter_reg,
            });
        }

        // Allocate result register (for returns nil)
        let result_reg = self.fresh_reg();
        self.emit(LirInstr::Const {
            dst: result_reg,
            value: LirConst::Nil,
        });

        let cond_label = self.fresh_label();
        let body_label = self.fresh_label();
        let done_label = self.fresh_label();

        // Jump to condition check
        self.terminate(Terminator::Jump(cond_label));
        self.finish_block();

        // Condition block: check if iterator is a pair
        self.current_block = BasicBlock::new(cond_label);
        let current_iter = self.fresh_reg();
        if self.in_lambda {
            self.emit(LirInstr::LoadCapture {
                dst: current_iter,
                index: iter_slot,
            });
        } else {
            self.emit(LirInstr::LoadLocal {
                dst: current_iter,
                slot: iter_slot,
            });
        }
        let is_pair = self.fresh_reg();
        self.emit(LirInstr::IsPair {
            dst: is_pair,
            src: current_iter,
        });
        self.terminate(Terminator::Branch {
            cond: is_pair,
            then_label: body_label,
            else_label: done_label,
        });
        self.finish_block();

        // Body block
        self.current_block = BasicBlock::new(body_label);

        // Extract car and store to VAR slot (not iter slot!)
        let iter_for_car = self.fresh_reg();
        if self.in_lambda {
            self.emit(LirInstr::LoadCapture {
                dst: iter_for_car,
                index: iter_slot,
            });
        } else {
            self.emit(LirInstr::LoadLocal {
                dst: iter_for_car,
                slot: iter_slot,
            });
        }
        let car_reg = self.fresh_reg();
        self.emit(LirInstr::Car {
            dst: car_reg,
            pair: iter_for_car,
        });
        // var_slot is allocated via allocate_slot, which handles lambda case
        // But we need to use the right instruction based on in_lambda
        if self.in_lambda {
            self.emit(LirInstr::StoreCapture {
                index: var_slot,
                src: car_reg,
            });
        } else {
            self.emit(LirInstr::StoreLocal {
                slot: var_slot,
                src: car_reg,
            });
        }

        // Evaluate body
        self.lower_expr(body)?;

        // Advance iterator: iter_slot = cdr(iter_slot)
        let iter_for_cdr = self.fresh_reg();
        if self.in_lambda {
            self.emit(LirInstr::LoadCapture {
                dst: iter_for_cdr,
                index: iter_slot,
            });
        } else {
            self.emit(LirInstr::LoadLocal {
                dst: iter_for_cdr,
                slot: iter_slot,
            });
        }
        let cdr_reg = self.fresh_reg();
        self.emit(LirInstr::Cdr {
            dst: cdr_reg,
            pair: iter_for_cdr,
        });
        if self.in_lambda {
            self.emit(LirInstr::StoreCapture {
                index: iter_slot,
                src: cdr_reg,
            });
        } else {
            self.emit(LirInstr::StoreLocal {
                slot: iter_slot,
                src: cdr_reg,
            });
        }

        // Loop back to condition
        self.terminate(Terminator::Jump(cond_label));
        self.finish_block();

        // Done block
        self.current_block = BasicBlock::new(done_label);
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
            self.emit(LirInstr::Move {
                dst: result_reg,
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
            self.emit(LirInstr::Move {
                dst: result_reg,
                src: else_reg,
            });
        } else {
            let nil_reg = self.emit_const(LirConst::Nil)?;
            self.emit(LirInstr::Move {
                dst: result_reg,
                src: nil_reg,
            });
        }
        self.terminate(Terminator::Jump(done_label));
        self.finish_block();

        // Done block (continue here)
        self.current_block = BasicBlock::new(done_label);

        Ok(result_reg)
    }
}
