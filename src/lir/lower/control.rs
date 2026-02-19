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

        let exit_label_id = self.next_label;
        self.next_label += 1;

        let result_reg = self.fresh_reg();

        for (i, expr) in exprs.iter().enumerate() {
            let expr_reg = self.lower_expr(expr)?;

            if i < exprs.len() - 1 {
                // Not the last expression - need to handle short-circuit
                // Dup the value: one copy for potential return, one for the test
                self.emit(LirInstr::Dup {
                    dst: result_reg,
                    src: expr_reg,
                });

                // If false, jump to exit (short-circuit)
                // Test the duplicate (result_reg) which is on top of the stack.
                // This pops result_reg, leaving expr_reg on the stack.
                self.emit(LirInstr::JumpIfFalseInline {
                    cond: result_reg,
                    label_id: exit_label_id,
                });

                // If we didn't short-circuit, pop the original
                // (we'll compute a new result in the next iteration)
                self.emit(LirInstr::Pop { src: expr_reg });
            } else {
                // Last expression - just move to result_reg
                self.emit(LirInstr::Move {
                    dst: result_reg,
                    src: expr_reg,
                });
            }
        }

        // Exit label
        self.emit(LirInstr::LabelMarker {
            label_id: exit_label_id,
        });

        Ok(result_reg)
    }

    pub(super) fn lower_or(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        if exprs.is_empty() {
            return self.emit_const(LirConst::Bool(false));
        }

        let exit_label_id = self.next_label;
        self.next_label += 1;

        let result_reg = self.fresh_reg();

        for (i, expr) in exprs.iter().enumerate() {
            let expr_reg = self.lower_expr(expr)?;

            if i < exprs.len() - 1 {
                // Not the last expression - need to handle short-circuit
                // Dup the value: one copy for potential return, one for the test
                self.emit(LirInstr::Dup {
                    dst: result_reg,
                    src: expr_reg,
                });

                // If true, jump to exit (short-circuit)
                // We use JumpIfFalseInline to skip to next, then JumpInline to exit
                let next_label_id = self.next_label;
                self.next_label += 1;

                // If false, jump to next (don't short-circuit)
                // Test the duplicate (result_reg) which is on top of the stack.
                // This pops result_reg, leaving expr_reg on the stack.
                self.emit(LirInstr::JumpIfFalseInline {
                    cond: result_reg,
                    label_id: next_label_id,
                });

                // If we get here, expr was true - short-circuit to exit
                // expr_reg is still on stack and will be our result
                self.emit(LirInstr::JumpInline {
                    label_id: exit_label_id,
                });

                // Next label - we didn't short-circuit (expr was false)
                self.emit(LirInstr::LabelMarker {
                    label_id: next_label_id,
                });

                // Pop the original (we'll compute a new result in the next iteration)
                self.emit(LirInstr::Pop { src: expr_reg });
            } else {
                // Last expression - just move to result_reg
                self.emit(LirInstr::Move {
                    dst: result_reg,
                    src: expr_reg,
                });
            }
        }

        self.emit(LirInstr::LabelMarker {
            label_id: exit_label_id,
        });
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

        // Allocate end label
        let end_label_id = self.next_label;
        self.next_label += 1;

        // Process each arm
        for (pattern, guard, body) in arms {
            // Allocate label for next arm (if pattern doesn't match)
            let next_arm_label_id = self.next_label;
            self.next_label += 1;

            // Reload the match value from the local slot for each arm.
            // This ensures the value is available even after control flow jumps.
            let arm_value_reg = self.fresh_reg();
            self.emit(LirInstr::LoadLocal {
                dst: arm_value_reg,
                slot: match_value_slot,
            });

            // Generate pattern matching code
            // This will emit JumpIfFalseInline to next_arm_label_id if pattern doesn't match
            self.lower_pattern_match(pattern, arm_value_reg, next_arm_label_id)?;

            // If we reach here, pattern matched
            // Check guard if present
            if let Some(guard_expr) = guard {
                let guard_reg = self.lower_expr(guard_expr)?;
                self.emit(LirInstr::JumpIfFalseInline {
                    cond: guard_reg,
                    label_id: next_arm_label_id,
                });
            }

            // Lower body
            let body_reg = self.lower_expr(body)?;
            self.emit(LirInstr::Move {
                dst: result_reg,
                src: body_reg,
            });

            // Jump to end
            self.emit(LirInstr::JumpInline {
                label_id: end_label_id,
            });

            // Next arm label
            self.emit(LirInstr::LabelMarker {
                label_id: next_arm_label_id,
            });
        }

        // If no arm matched, return nil (or could be an error)
        let nil_reg = self.emit_const(LirConst::Nil)?;
        self.emit(LirInstr::Move {
            dst: result_reg,
            src: nil_reg,
        });

        // End label
        self.emit(LirInstr::LabelMarker {
            label_id: end_label_id,
        });

        Ok(result_reg)
    }

    pub(super) fn lower_handler_case(
        &mut self,
        body: &Hir,
        handlers: &[(u32, BindingId, Box<Hir>)],
    ) -> Result<Reg, String> {
        let result_reg = self.fresh_reg();

        // Labels
        let handler_start_label = self.next_label;
        self.next_label += 1;
        let end_label = self.next_label;
        self.next_label += 1;

        // Emit PushHandler pointing to handler code
        self.emit(LirInstr::PushHandler {
            handler_label: Label(handler_start_label),
        });

        // Compile protected body
        let body_reg = self.lower_expr(body)?;
        self.emit(LirInstr::Move {
            dst: result_reg,
            src: body_reg,
        });

        // Success path: pop handler and jump to end
        self.emit(LirInstr::PopHandler);
        self.emit(LirInstr::JumpInline {
            label_id: end_label,
        });

        // Handler code starts here
        self.emit(LirInstr::LabelMarker {
            label_id: handler_start_label,
        });
        self.emit(LirInstr::CheckException);

        // Compile each handler clause
        for (exception_id, var_id, handler_body) in handlers.iter() {
            let next_handler_label = self.next_label;
            self.next_label += 1;

            // Match exception type
            let match_result = self.fresh_reg();
            self.emit(LirInstr::MatchException {
                dst: match_result,
                exception_id: *exception_id as u16,
            });
            self.emit(LirInstr::JumpIfFalseInline {
                cond: match_result,
                label_id: next_handler_label,
            });

            // Allocate slot for the exception variable and store exception to it
            let var_slot = self.allocate_slot(*var_id);

            // Load the current exception and store to local
            let exc_reg = self.fresh_reg();
            self.emit(LirInstr::LoadException { dst: exc_reg });
            self.emit(LirInstr::StoreLocal {
                slot: var_slot,
                src: exc_reg,
            });

            // Clear the exception BEFORE executing the handler body.
            // This ensures that if the handler body yields, the exception
            // won't be propagated to the caller.
            self.emit(LirInstr::ClearException);

            // Compile handler body (now can find var_id via LoadLocal)
            let handler_reg = self.lower_expr(handler_body)?;
            self.emit(LirInstr::Move {
                dst: result_reg,
                src: handler_reg,
            });

            // Jump to end
            self.emit(LirInstr::JumpInline {
                label_id: end_label,
            });

            // Next handler label
            self.emit(LirInstr::LabelMarker {
                label_id: next_handler_label,
            });
        }

        // No handler matched — re-raise exception to propagate
        // to next enclosing handler
        self.emit(LirInstr::ReraiseException);
        // Jump to unreachable label to prevent fall-through to ClearException
        // The exception interrupt mechanism will jump to the next handler
        let unreachable_label = self.next_label;
        self.next_label += 1;
        self.emit(LirInstr::JumpInline {
            label_id: unreachable_label,
        });

        // End label (reached by success path and matched handler paths)
        self.emit(LirInstr::LabelMarker {
            label_id: end_label,
        });
        // Note: ClearException is now emitted BEFORE the handler body,
        // not after, to ensure it's executed even if the handler yields.

        Ok(result_reg)
    }
}
