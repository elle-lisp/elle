//! Control flow lowering: and, or, match, handler-case, yield, call

use super::*;
use crate::hir::{CallArg, HirPattern};

impl Lowerer {
    pub(super) fn lower_call(
        &mut self,
        func: &Hir,
        args: &[CallArg],
        is_tail: bool,
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
                arg_regs.push(self.lower_expr(&arg.expr)?);
            }
            let func_reg = self.lower_expr(func)?;

            if is_tail {
                self.emit(LirInstr::TailCall {
                    func: func_reg,
                    args: arg_regs,
                });
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
        } else {
            // === Splice path: build args array, then CallArray ===
            // Lower all args first
            let mut lowered: Vec<(Reg, bool)> = Vec::new();
            for arg in args {
                let reg = self.lower_expr(&arg.expr)?;
                lowered.push((reg, arg.spliced));
            }
            let func_reg = self.lower_expr(func)?;

            // Build the args array incrementally
            // Start with MakeArray of the first run of non-spliced args
            let mut args_reg: Option<Reg> = None;

            for (reg, spliced) in &lowered {
                match (args_reg, spliced) {
                    (None, false) => {
                        // First arg, not spliced: create array with one element
                        let dst = self.fresh_reg();
                        self.emit(LirInstr::MakeArray {
                            dst,
                            elements: vec![*reg],
                        });
                        args_reg = Some(dst);
                    }
                    (None, true) => {
                        // First arg, spliced: create empty array, then extend
                        let empty = self.fresh_reg();
                        self.emit(LirInstr::MakeArray {
                            dst: empty,
                            elements: vec![],
                        });
                        let dst = self.fresh_reg();
                        self.emit(LirInstr::ArrayExtend {
                            dst,
                            array: empty,
                            source: *reg,
                        });
                        args_reg = Some(dst);
                    }
                    (Some(arr), false) => {
                        let dst = self.fresh_reg();
                        self.emit(LirInstr::ArrayPush {
                            dst,
                            array: arr,
                            value: *reg,
                        });
                        args_reg = Some(dst);
                    }
                    (Some(arr), true) => {
                        let dst = self.fresh_reg();
                        self.emit(LirInstr::ArrayExtend {
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
                self.emit(LirInstr::MakeArray {
                    dst,
                    elements: vec![],
                });
                dst
            });

            if is_tail {
                self.emit(LirInstr::TailCallArray {
                    func: func_reg,
                    args: final_args,
                });
                Ok(self.fresh_reg())
            } else {
                let dst = self.fresh_reg();
                self.emit(LirInstr::CallArray {
                    dst,
                    func: func_reg,
                    args: final_args,
                });
                Ok(dst)
            }
        }
    }

    /// Try to lower a call as an intrinsic operation.
    ///
    /// Returns `Some(result_reg)` if the call was specialized, `None` to
    /// fall through to generic call. Only specializes when:
    /// - The function is a global variable reference
    /// - The global is not mutated (so it still holds the original primitive)
    /// - The SymbolId maps to a known intrinsic
    /// - The argument count matches (2 for binary/compare, 1 for unary)
    fn try_lower_intrinsic(&mut self, func: &Hir, args: &[&Hir]) -> Result<Option<Reg>, String> {
        // Must be a variable reference
        let HirKind::Var(binding) = &func.kind else {
            return Ok(None);
        };

        // Must be a global that hasn't been mutated
        if !binding.is_global() || binding.is_mutated() {
            return Ok(None);
        }

        let sym = binding.name();

        // Special case: `-` with 1 arg is negation
        if args.len() == 1 {
            if let Some(IntrinsicOp::Binary(BinOp::Sub)) = self.intrinsics.get(&sym) {
                let src = self.lower_expr(args[0])?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::UnaryOp {
                    dst,
                    op: UnaryOp::Neg,
                    src,
                });
                return Ok(Some(dst));
            }
        }

        let Some(&intrinsic) = self.intrinsics.get(&sym) else {
            return Ok(None);
        };

        match intrinsic {
            IntrinsicOp::Binary(op) => {
                if args.len() != 2 {
                    return Ok(None); // Variadic — fall through to generic call
                }
                let lhs = self.lower_expr(args[0])?;
                let rhs = self.lower_expr(args[1])?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::BinOp { dst, op, lhs, rhs });
                Ok(Some(dst))
            }
            IntrinsicOp::Compare(op) => {
                if args.len() != 2 {
                    // 0-1 args: fall through to generic call for arity error.
                    // 3+ args: fall through to generic call — the primitive
                    // handles chained comparison with short-circuit.
                    return Ok(None);
                }
                let lhs = self.lower_expr(args[0])?;
                let rhs = self.lower_expr(args[1])?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::Compare { dst, op, lhs, rhs });
                Ok(Some(dst))
            }
            IntrinsicOp::Unary(op) => {
                if args.len() != 1 {
                    return Ok(None);
                }
                let src = self.lower_expr(args[0])?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::UnaryOp { dst, op, src });
                Ok(Some(dst))
            }
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

    pub(super) fn lower_eval(&mut self, expr: &Hir, env: &Hir) -> Result<Reg, String> {
        let env_reg = self.lower_expr(env)?;
        let expr_reg = self.lower_expr(expr)?;
        let dst = self.fresh_reg();
        self.emit(LirInstr::Eval {
            dst,
            expr: expr_reg,
            env: env_reg,
        });
        Ok(dst)
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
        // Evaluate the scrutinee and store to a local slot.
        // The emitter pre-allocates space for all locals at the start of
        // the entry block, so StoreLocal never clobbers operand values
        // from enclosing expressions.
        let value_reg = self.lower_expr(value)?;
        let scrutinee_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        self.emit(LirInstr::StoreLocal {
            slot: scrutinee_slot,
            src: value_reg,
        });
        // Pop the pushed-back value — the scrutinee lives in the local
        // slot and is reloaded via LoadLocal.  Leaving it on the operand
        // stack would leak an intermediate between enclosing operands.
        self.emit(LirInstr::Pop { src: value_reg });

        // Allocate result register and result slot
        let result_reg = self.fresh_reg();
        let result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        let done_label = self.fresh_label();

        // Guard effect safety valve: if any guard may suspend, the decision
        // tree cannot safely backtrack past the guard (it may have yielded).
        // Fall back to sequential matching which doesn't share tests.
        let any_guard_yields = arms
            .iter()
            .any(|(_pat, guard, _body)| guard.as_ref().is_some_and(|g| g.effect.may_suspend()));

        if any_guard_yields {
            self.lower_match_sequential(arms, scrutinee_slot, result_slot, result_reg, done_label)?;
            return Ok(result_reg);
        }

        // Build decision tree
        use super::decision::{AccessPath, PatternMatrix};
        let matrix = PatternMatrix::from_arms(arms);
        let tree = matrix.compile(vec![AccessPath::Root]);

        // Lower decision tree
        self.lower_decision_tree(&tree, arms, scrutinee_slot, result_slot, done_label)?;

        // Done block: reload result
        self.current_block = BasicBlock::new(done_label);
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

        Ok(result_reg)
    }

    /// Sequential match lowering: try each arm in order. Used as fallback
    /// when guards may suspend (yield/debug/polymorphic), since the decision
    /// tree cannot safely backtrack past a suspending guard.
    fn lower_match_sequential(
        &mut self,
        arms: &[(HirPattern, Option<Hir>, Hir)],
        scrutinee_slot: u16,
        result_slot: u16,
        result_reg: Reg,
        done_label: Label,
    ) -> Result<(), String> {
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
            let arm_value_reg = self.fresh_reg();
            self.emit(LirInstr::LoadLocal {
                dst: arm_value_reg,
                slot: scrutinee_slot,
            });

            // Generate pattern matching code
            self.lower_pattern_match(pattern, arm_value_reg, next_arm_label)?;

            // Check guard if present
            if let Some(guard_expr) = guard {
                let guard_reg = self.lower_expr(guard_expr)?;
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
            self.emit(LirInstr::StoreLocal {
                slot: result_slot,
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
        self.emit(LirInstr::StoreLocal {
            slot: result_slot,
            src: nil_reg,
        });
        self.terminate(Terminator::Jump(done_label));
        self.finish_block();

        // Done block
        self.current_block = BasicBlock::new(done_label);
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

        Ok(())
    }
}
