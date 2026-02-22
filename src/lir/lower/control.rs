//! Control flow lowering: and, or, match, handler-case, yield, call

use super::*;
use crate::hir::HirPattern;

impl Lowerer {
    pub(super) fn lower_call(
        &mut self,
        func: &Hir,
        args: &[Hir],
        is_tail: bool,
    ) -> Result<Reg, String> {
        // Lower arguments first, then function
        // This ensures the stack is in the right order for the Call instruction
        let mut arg_regs = Vec::new();
        for arg in args {
            arg_regs.push(self.lower_expr(arg)?);
        }
        let func_reg = self.lower_expr(func)?;

        if is_tail {
            self.emit(LirInstr::TailCall {
                func: func_reg,
                args: arg_regs,
            });
            // After tail call, we need a placeholder reg
            Ok(self.fresh_reg())
        } else {
            let dst = self.fresh_reg();
            self.emit(LirInstr::Call {
                dst,
                func: func_reg,
                args: arg_regs,
            });
            Ok(dst)
        }
    }

    pub(super) fn lower_and(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        if exprs.is_empty() {
            return self.emit_const(LirConst::Bool(true));
        }
        if exprs.len() == 1 {
            return self.lower_expr(&exprs[0]);
        }

        // Use a shared result register. Each branch leaves its result on the stack.
        // At the merge point, the result is on top of the stack.
        let result_reg = self.fresh_reg();
        let done_label = self.fresh_label();

        for (i, expr) in exprs.iter().enumerate() {
            let val_reg = self.lower_expr(expr)?;

            if i < exprs.len() - 1 {
                // Not the last expression — branch on truthiness
                // If falsy, short-circuit to done with this value
                // If truthy, pop this value and continue to next expression
                //
                // Dup the value: one copy for the branch test, one for the result
                let dup_reg = self.fresh_reg();
                self.emit(LirInstr::Dup {
                    dst: dup_reg,
                    src: val_reg,
                });

                // Use Move to track the result with result_reg
                // This ensures the emitter knows result_reg is at the same position as val_reg
                self.emit(LirInstr::Move {
                    dst: result_reg,
                    src: val_reg,
                });

                let next_label = self.fresh_label();
                // Branch on the duplicate (which will be popped by JumpIfFalse)
                self.terminate(Terminator::Branch {
                    cond: dup_reg,
                    then_label: next_label,
                    else_label: done_label,
                });
                self.finish_block();

                // Next block: pop the original value and continue
                self.current_block = BasicBlock::new(next_label);
                self.emit(LirInstr::Pop { src: result_reg });
            } else {
                // Last expression — this is the result, jump to done
                self.emit(LirInstr::Move {
                    dst: result_reg,
                    src: val_reg,
                });
                self.terminate(Terminator::Jump(done_label));
                self.finish_block();
            }
        }

        // Done block (continue here)
        // The result is on top of the stack from whichever branch was taken
        self.current_block = BasicBlock::new(done_label);

        Ok(result_reg)
    }

    pub(super) fn lower_or(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        if exprs.is_empty() {
            return self.emit_const(LirConst::Bool(false));
        }
        if exprs.len() == 1 {
            return self.lower_expr(&exprs[0]);
        }

        // Use a shared result register. Each branch leaves its result on the stack.
        // At the merge point, the result is on top of the stack.
        let result_reg = self.fresh_reg();
        let done_label = self.fresh_label();

        for (i, expr) in exprs.iter().enumerate() {
            let val_reg = self.lower_expr(expr)?;

            if i < exprs.len() - 1 {
                // Not the last expression — branch on truthiness
                // If truthy, short-circuit to done with this value
                // If falsy, pop this value and continue to next expression
                //
                // Dup the value: one copy for the branch test, one for the result
                let dup_reg = self.fresh_reg();
                self.emit(LirInstr::Dup {
                    dst: dup_reg,
                    src: val_reg,
                });

                // Use Move to track the result with result_reg
                // This ensures the emitter knows result_reg is at the same position as val_reg
                self.emit(LirInstr::Move {
                    dst: result_reg,
                    src: val_reg,
                });

                let next_label = self.fresh_label();
                // Branch on the duplicate (which will be popped by JumpIfFalse)
                self.terminate(Terminator::Branch {
                    cond: dup_reg,
                    then_label: done_label,
                    else_label: next_label,
                });
                self.finish_block();

                // Next block: pop the original value and continue
                self.current_block = BasicBlock::new(next_label);
                self.emit(LirInstr::Pop { src: result_reg });
            } else {
                // Last expression — this is the result, jump to done
                self.emit(LirInstr::Move {
                    dst: result_reg,
                    src: val_reg,
                });
                self.terminate(Terminator::Jump(done_label));
                self.finish_block();
            }
        }

        // Done block (continue here)
        // The result is on top of the stack from whichever branch was taken
        self.current_block = BasicBlock::new(done_label);

        Ok(result_reg)
    }

    pub(super) fn lower_yield(&mut self, value: &Hir) -> Result<Reg, String> {
        let value_reg = self.lower_expr(value)?;

        // Allocate the resume block label
        let resume_label = self.fresh_label();

        // Terminate current block with Yield
        self.terminate(Terminator::Yield {
            value: value_reg,
            resume_label,
        });

        // Start the resume block
        self.start_new_block(resume_label);

        // The resume value is on the stack when execution resumes.
        // Load it into a register.
        let dst = self.fresh_reg();
        self.emit(LirInstr::LoadResumeValue { dst });

        Ok(dst)
    }

    pub(super) fn lower_match(
        &mut self,
        value: &Hir,
        arms: &[(HirPattern, Option<Hir>, Hir)],
    ) -> Result<Reg, String> {
        // Evaluate the value to match
        let value_reg = self.lower_expr(value)?;

        // Store the match value to a local slot so we can reload it for each arm.
        // This is necessary because the stack-based emitter loses track of registers
        // across control flow (jumps between match arms).
        let match_value_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        self.emit(LirInstr::StoreLocal {
            slot: match_value_slot,
            src: value_reg,
        });

        // Allocate result register
        let result_reg = self.fresh_reg();

        // Allocate done label
        let done_label = self.fresh_label();

        // Pre-allocate labels for each arm
        let arm_labels: Vec<Label> = (0..arms.len()).map(|_| self.fresh_label()).collect();
        let no_match_label = self.fresh_label();

        // Process each arm
        for (i, (pattern, guard, body)) in arms.iter().enumerate() {
            let next_arm_label = if i + 1 < arms.len() {
                arm_labels[i + 1]
            } else {
                no_match_label
            };

            // Reload the match value from the local slot for each arm.
            // This ensures the value is available even after control flow jumps.
            let arm_value_reg = self.fresh_reg();
            self.emit(LirInstr::LoadLocal {
                dst: arm_value_reg,
                slot: match_value_slot,
            });

            // Generate pattern matching code
            // This will branch to next_arm_label if pattern doesn't match
            self.lower_pattern_match(pattern, arm_value_reg, next_arm_label)?;

            // If we reach here, pattern matched
            // Check guard if present
            if let Some(guard_expr) = guard {
                let guard_reg = self.lower_expr(guard_expr)?;
                // If guard fails, go to next arm
                let guard_pass_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: guard_reg,
                    then_label: guard_pass_label,
                    else_label: next_arm_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(guard_pass_label);
            }

            // Lower body
            let body_reg = self.lower_expr(body)?;
            self.emit(LirInstr::Move {
                dst: result_reg,
                src: body_reg,
            });

            // Jump to done
            self.terminate(Terminator::Jump(done_label));
            self.finish_block();

            // Start next arm block
            if i + 1 < arms.len() {
                self.current_block = BasicBlock::new(arm_labels[i + 1]);
            }
        }

        // No match block: return nil
        self.current_block = BasicBlock::new(no_match_label);
        let nil_reg = self.emit_const(LirConst::Nil)?;
        self.emit(LirInstr::Move {
            dst: result_reg,
            src: nil_reg,
        });
        self.terminate(Terminator::Jump(done_label));
        self.finish_block();

        // Done block (continue here)
        self.current_block = BasicBlock::new(done_label);

        Ok(result_reg)
    }
}
