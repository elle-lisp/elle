use super::*;

impl<'a> Lowerer<'a> {
    pub(super) fn lower_while(
        &mut self,
        cond: &Hir,
        body: &Hir,
        hir_id: HirId,
    ) -> Result<Reg, String> {
        let result_reg = self.fresh_reg();
        let region_id = if self.region_loop_check(hir_id) {
            self.scope_region_id(hir_id)
        } else {
            None
        };

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

        self.current_block = BasicBlock::new(body_label);

        let _body_reg = self.lower_expr(body)?;

        // Per-iteration DecrefRegion is emitted by `lower_expr` at each
        // region's `decref_point` HirId. No back-edge scope cleanup needed.
        let _ = region_id;

        self.terminate(Terminator::Jump(cond_label));
        self.finish_block();

        self.current_block = BasicBlock::new(done_label);
        self.emit(LirInstr::Const {
            dst: result_reg,
            value: LirConst::Nil,
        });
        Ok(result_reg)
    }
    pub(super) fn lower_loop(
        &mut self,
        bindings: &[(Binding, Hir)],
        body: &Hir,
        hir_id: HirId,
    ) -> Result<Reg, String> {
        let result_reg = self.fresh_reg();
        let scope_eligible = self.region_loop_check(hir_id);
        let region_id = if scope_eligible {
            self.scope_region_id(hir_id)
        } else {
            None
        };

        let loop_label = self.fresh_label();
        let done_label = self.fresh_label();

        // Initialize loop bindings
        let mut binding_slots = Vec::new();
        for (binding, init) in bindings {
            let init_reg = self.lower_expr(init)?;
            let slot = self.allocate_slot(*binding);
            self.emit_binding_store(slot, init_reg);
            binding_slots.push(slot);
        }

        // Jump to loop header
        self.terminate(Terminator::Jump(loop_label));
        self.finish_block();

        // Loop body
        self.current_block = BasicBlock::new(loop_label);

        self.loop_lower_contexts.push(LoopLowerContext {
            loop_label,
            binding_slots: binding_slots.clone(),
            region_id,
        });

        let body_reg = self.lower_expr(body)?;

        self.loop_lower_contexts.pop();

        // If we reach here (no Recur), body_reg is the loop result.
        let result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        self.emit(LirInstr::StoreLocal {
            slot: result_slot,
            src: body_reg,
        });

        self.terminate(Terminator::Jump(done_label));
        self.finish_block();

        // Done block — load result from slot
        self.current_block = BasicBlock::new(done_label);
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });
        Ok(result_reg)
    }
    pub(super) fn lower_recur(&mut self, args: &[Hir]) -> Result<Reg, String> {
        let ctx = self
            .loop_lower_contexts
            .last()
            .ok_or_else(|| "recur outside of loop".to_string())?;

        let loop_label = ctx.loop_label;
        let binding_slots = ctx.binding_slots.clone();
        let region_id = ctx.region_id;

        if args.len() != binding_slots.len() {
            return Err(format!(
                "recur: expected {} arguments, got {}",
                binding_slots.len(),
                args.len()
            ));
        }

        // Evaluate all args before storing
        let mut arg_regs = Vec::with_capacity(args.len());
        for arg in args {
            arg_regs.push(self.lower_expr(arg)?);
        }

        // Store new values to loop binding slots.
        for (reg, &slot) in arg_regs.iter().zip(&binding_slots) {
            self.emit(LirInstr::StoreLocal { slot, src: *reg });
        }

        // Per-iteration DecrefRegion is emitted by `lower_expr` at each
        // region's `decref_point` HirId. No back-edge scope cleanup needed.
        let _ = region_id;

        // Jump back to loop header
        self.terminate(Terminator::Jump(loop_label));
        self.finish_block();

        // Dead block after unconditional jump
        let dead_label = self.fresh_label();
        self.current_block = BasicBlock::new(dead_label);
        let nil_reg = self.emit_const(LirConst::Nil)?;
        Ok(nil_reg)
    }
    pub(super) fn lower_break(&mut self, block_id: &BlockId, value: &Hir) -> Result<Reg, String> {
        let target = self
            .block_lower_contexts
            .iter()
            .rev()
            .find(|ctx| ctx.block_id == *block_id)
            .ok_or_else(|| format!("Internal error: no block context for {:?}", block_id))?;

        let target_result_slot = target.result_slot;
        let target_exit_label = target.exit_label;

        let value_reg = self.lower_expr(value)?;

        self.emit(LirInstr::StoreLocal {
            slot: target_result_slot,
            src: value_reg,
        });

        // Break emits no region instruction of its own, on either face. The
        // value it carries is TRANSFERRED to the block, so its release is
        // anchored where the block's value is consumed — reached by this jump —
        // rather than inside the body (docs/impl/region/mechanism.md § "`break`
        // transfers its value; it does not consume it"); a compensating release
        // here would free the value the block is about to hand to its consumer.
        // Every OTHER release this jump passes over is anchored on the block by
        // the same solver pin (§ "A release the break jumps over is not a
        // release"), so there is nothing to walk here either. Pinned by
        // tests/elle/region-break-transfer.lisp and region-break-skip.lisp.
        self.terminate(Terminator::Jump(target_exit_label));

        let dead_label = self.fresh_label();
        self.start_new_block(dead_label);

        Ok(self.fresh_reg())
    }
    pub(super) fn lower_cond(
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

        // Every path into the done block leaves through a clause body or the
        // else block, so each seals its relocation points for the merge to
        // inherit (docs/impl/region/mechanism.md § "The relocation point outlives
        // the block"). A `cond` with no `else` contributes an empty-handed nil
        // path, which simply carries no point.
        let saved_arm_hoists = self.begin_branch_arms();

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
            self.seal_arm_hoists();
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
        self.seal_arm_hoists();
        self.finish_block();

        // Done block (continue here)
        self.current_block = BasicBlock::new(done_label);
        self.open_branch_merge(saved_arm_hoists);
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: cond_result_slot,
        });

        Ok(result_reg)
    }
}
