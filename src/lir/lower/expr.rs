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
            HirKind::Keyword(sym) => self.emit_const(LirConst::Keyword(*sym)),

            HirKind::Var(binding_id) => self.lower_var(binding_id),
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
            HirKind::Define { name, value } => self.lower_define(*name, value),
            HirKind::LocalDefine { binding, value } => self.lower_local_define(*binding, value),

            HirKind::While { cond, body } => self.lower_while(cond, body),
            HirKind::For { var, iter, body } => self.lower_for(*var, iter, body),

            HirKind::And(exprs) => self.lower_and(exprs),
            HirKind::Or(exprs) => self.lower_or(exprs),

            HirKind::Yield(value) => self.lower_yield(value),
            HirKind::Quote(value) => self.emit_value_const(*value),
            HirKind::Throw(value) => self.lower_throw(value),

            HirKind::Cond {
                clauses,
                else_branch,
            } => self.lower_cond(clauses, else_branch),

            HirKind::Match { value, arms } => self.lower_match(value, arms),
            HirKind::HandlerCase { body, handlers } => self.lower_handler_case(body, handlers),
            HirKind::HandlerBind { body, .. } => self.lower_expr(body),
            HirKind::Module { body, .. } => self.lower_expr(body),
            HirKind::Import { .. } => self.emit_const(LirConst::Nil),
            HirKind::ModuleRef { .. } => self.emit_const(LirConst::Nil),
        }
    }

    fn lower_var(&mut self, binding_id: &BindingId) -> Result<Reg, String> {
        if let Some(&slot) = self.binding_to_slot.get(binding_id) {
            // Check if this binding needs cell unwrapping
            let needs_cell = self
                .bindings
                .get(binding_id)
                .map(|info| info.needs_cell())
                .unwrap_or(false);

            // Check if this is an upvalue (capture or parameter) or a local
            let is_upvalue = self.upvalue_bindings.contains(binding_id);

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
        } else if let Some(info) = self.bindings.get(binding_id) {
            match info.kind {
                BindingKind::Global => {
                    let sym = info.name;
                    let dst = self.fresh_reg();
                    self.emit(LirInstr::LoadGlobal { dst, sym });
                    Ok(dst)
                }
                _ => Err(format!("Unbound variable: {:?}", binding_id)),
            }
        } else {
            Err(format!("Unknown binding: {:?}", binding_id))
        }
    }

    fn lower_if(
        &mut self,
        cond: &Hir,
        then_branch: &Hir,
        else_branch: &Hir,
    ) -> Result<Reg, String> {
        // Lower condition
        let cond_reg = self.lower_expr(cond)?;

        // Allocate a result register for the if expression
        let result_reg = self.fresh_reg();

        // Allocate label IDs for inline jumps
        let else_label_id = self.next_label;
        self.next_label += 1;
        let end_label_id = self.next_label;
        self.next_label += 1;

        // Emit conditional jump to else (will jump if condition is false)
        self.emit(LirInstr::JumpIfFalseInline {
            cond: cond_reg,
            label_id: else_label_id,
        });

        // Lower then branch
        let then_reg = self.lower_expr(then_branch)?;
        // Move then result to result register
        self.emit(LirInstr::Move {
            dst: result_reg,
            src: then_reg,
        });

        // Emit unconditional jump to end
        self.emit(LirInstr::JumpInline {
            label_id: end_label_id,
        });

        // Emit else label marker
        self.emit(LirInstr::LabelMarker {
            label_id: else_label_id,
        });

        // Lower else branch
        let else_reg = self.lower_expr(else_branch)?;
        // Move else result to result register
        self.emit(LirInstr::Move {
            dst: result_reg,
            src: else_reg,
        });

        // Emit end label marker
        self.emit(LirInstr::LabelMarker {
            label_id: end_label_id,
        });

        // Return the result register
        Ok(result_reg)
    }

    fn lower_begin(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        // Pre-allocate slots for all LocalDefine bindings
        // This enables mutual recursion where lambda A captures variable B
        // before B's LocalDefine has been lowered
        for expr in exprs {
            if let HirKind::LocalDefine { binding, .. } = &expr.kind {
                // Allocate slot now so captures can find it
                if !self.binding_to_slot.contains_key(binding) {
                    let slot = self.allocate_slot(*binding);

                    // Inside lambdas, local variables are part of the closure environment
                    if self.in_lambda {
                        self.upvalue_bindings.insert(*binding);
                    }

                    // Check if this binding needs a cell
                    let needs_cell = self
                        .bindings
                        .get(binding)
                        .map(|info| info.needs_cell())
                        .unwrap_or(false);

                    // Only create cells for top-level locals (outside lambdas)
                    // Inside lambdas, the VM creates cells for locally-defined variables
                    // when building the closure environment
                    if needs_cell && !self.in_lambda {
                        // Create a cell containing nil
                        // This cell will be captured by nested lambdas
                        // and updated when the LocalDefine is lowered
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
        let loop_label_id = self.next_label;
        self.next_label += 1;
        let exit_label_id = self.next_label;
        self.next_label += 1;

        // Emit loop label marker
        self.emit(LirInstr::LabelMarker {
            label_id: loop_label_id,
        });

        // Evaluate condition
        let cond_reg = self.lower_expr(cond)?;

        // Jump to exit if condition is false
        self.emit(LirInstr::JumpIfFalseInline {
            cond: cond_reg,
            label_id: exit_label_id,
        });

        // Evaluate body
        self.lower_expr(body)?;

        // Jump back to loop start
        self.emit(LirInstr::JumpInline {
            label_id: loop_label_id,
        });

        // Exit label
        self.emit(LirInstr::LabelMarker {
            label_id: exit_label_id,
        });

        self.emit_const(LirConst::Nil)
    }

    fn lower_for(&mut self, var: BindingId, iter: &Hir, body: &Hir) -> Result<Reg, String> {
        // Allocate separate slots for iterator and loop variable
        let iter_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;

        let var_slot = self.allocate_slot(var);

        // Store initial iterator
        let iter_reg = self.lower_expr(iter)?;
        self.emit(LirInstr::StoreLocal {
            slot: iter_slot,
            src: iter_reg,
        });

        let loop_label_id = self.next_label;
        self.next_label += 1;
        let exit_label_id = self.next_label;
        self.next_label += 1;

        // Loop start
        self.emit(LirInstr::LabelMarker {
            label_id: loop_label_id,
        });

        // Load iterator and check if it's a pair
        let current_iter = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: current_iter,
            slot: iter_slot,
        });
        let is_pair = self.fresh_reg();
        self.emit(LirInstr::IsPair {
            dst: is_pair,
            src: current_iter,
        });

        // Exit if not a pair
        self.emit(LirInstr::JumpIfFalseInline {
            cond: is_pair,
            label_id: exit_label_id,
        });

        // Extract car and store to VAR slot (not iter slot!)
        let iter_for_car = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: iter_for_car,
            slot: iter_slot,
        });
        let car_reg = self.fresh_reg();
        self.emit(LirInstr::Car {
            dst: car_reg,
            pair: iter_for_car,
        });
        self.emit(LirInstr::StoreLocal {
            slot: var_slot, // Store to loop variable, not iterator!
            src: car_reg,
        });

        // Evaluate body
        self.lower_expr(body)?;

        // Advance iterator: iter_slot = cdr(iter_slot)
        let iter_for_cdr = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: iter_for_cdr,
            slot: iter_slot,
        });
        let cdr_reg = self.fresh_reg();
        self.emit(LirInstr::Cdr {
            dst: cdr_reg,
            pair: iter_for_cdr,
        });
        self.emit(LirInstr::StoreLocal {
            slot: iter_slot, // Update iterator, not var
            src: cdr_reg,
        });

        // Loop back
        self.emit(LirInstr::JumpInline {
            label_id: loop_label_id,
        });

        // Exit label
        self.emit(LirInstr::LabelMarker {
            label_id: exit_label_id,
        });

        self.emit_const(LirConst::Nil)
    }

    fn lower_throw(&mut self, value: &Hir) -> Result<Reg, String> {
        let value_reg = self.lower_expr(value)?;
        self.emit(LirInstr::Throw { value: value_reg });
        self.emit_const(LirConst::Nil) // Unreachable but need a result
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

        let exit_label_id = self.next_label;
        self.next_label += 1;

        let result_reg = self.fresh_reg();

        for (test, body) in clauses {
            let test_reg = self.lower_expr(test)?;
            let next_label_id = self.next_label;
            self.next_label += 1;

            // Jump to next clause if test is false
            self.emit(LirInstr::JumpIfFalseInline {
                cond: test_reg,
                label_id: next_label_id,
            });

            // Evaluate body
            let body_reg = self.lower_expr(body)?;
            self.emit(LirInstr::Move {
                dst: result_reg,
                src: body_reg,
            });

            // Jump to exit
            self.emit(LirInstr::JumpInline {
                label_id: exit_label_id,
            });

            // Next clause label
            self.emit(LirInstr::LabelMarker {
                label_id: next_label_id,
            });
        }

        // Else branch
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

        // Exit label
        self.emit(LirInstr::LabelMarker {
            label_id: exit_label_id,
        });

        Ok(result_reg)
    }
}
