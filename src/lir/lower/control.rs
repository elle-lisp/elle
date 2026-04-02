//! Control flow lowering: and, or, match, handler-case, yield, call

use super::*;
use crate::hir::{CallArg, HirPattern};

impl<'a> Lowerer<'a> {
    pub(super) fn lower_call(
        &mut self,
        func: &Hir,
        args: &[CallArg],
        is_tail: bool,
        call_may_suspend: bool,
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
                if call_may_suspend {
                    self.emit(LirInstr::SuspendingCall {
                        dst,
                        func: func_reg,
                        args: arg_regs,
                    });
                } else {
                    self.emit(LirInstr::Call {
                        dst,
                        func: func_reg,
                        args: arg_regs,
                    });
                }
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
            let func_reg = self.lower_expr(func)?;

            // Build the args array incrementally
            // Start with MakeArrayMut of the first run of non-spliced args
            let mut args_reg: Option<Reg> = None;

            for (reg, spliced) in &lowered {
                match (args_reg, spliced) {
                    (None, false) => {
                        // First arg, not spliced: create array with one element
                        let dst = self.fresh_reg();
                        self.emit(LirInstr::MakeArrayMut {
                            dst,
                            elements: vec![*reg],
                        });
                        args_reg = Some(dst);
                    }
                    (None, true) => {
                        // First arg, spliced: create empty array, then extend
                        let empty = self.fresh_reg();
                        self.emit(LirInstr::MakeArrayMut {
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
                self.emit(LirInstr::MakeArrayMut {
                    dst,
                    elements: vec![],
                });
                dst
            });

            if is_tail {
                self.emit(LirInstr::TailCallArrayMut {
                    func: func_reg,
                    args: final_args,
                });
                Ok(self.fresh_reg())
            } else {
                let dst = self.fresh_reg();
                self.emit(LirInstr::CallArrayMut {
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

        // Must be an immutable binding that hasn't been mutated
        let bi = self.arena.get(*binding);
        if !bi.is_immutable || bi.is_mutated {
            return Ok(None);
        }

        let sym = bi.name;

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

        // Allocate result slot (same pattern as lower_cond/lower_if)
        let result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        let done_label = self.fresh_label();

        for (i, expr) in exprs.iter().enumerate() {
            let val_reg = self.lower_expr(expr)?;

            // Store value to result slot
            self.emit(LirInstr::StoreLocal {
                slot: result_slot,
                src: val_reg,
            });

            if i < exprs.len() - 1 {
                // Not the last expression: reload for branch test
                let cond_reg = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: cond_reg,
                    slot: result_slot,
                });

                let next_label = self.fresh_label();
                // If falsy, short-circuit to done (value already in slot)
                // If truthy, continue to next expression
                self.terminate(Terminator::Branch {
                    cond: cond_reg,
                    then_label: next_label,
                    else_label: done_label,
                });
                self.finish_block();

                self.current_block = BasicBlock::new(next_label);
            } else {
                // Last expression: jump to done (value already in slot)
                self.terminate(Terminator::Jump(done_label));
                self.finish_block();
            }
        }

        // Done block: load result from slot
        self.current_block = BasicBlock::new(done_label);
        let result_reg = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

        Ok(result_reg)
    }

    pub(super) fn lower_or(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        if exprs.is_empty() {
            return self.emit_const(LirConst::Bool(false));
        }
        if exprs.len() == 1 {
            return self.lower_expr(&exprs[0]);
        }

        let result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        let done_label = self.fresh_label();

        for (i, expr) in exprs.iter().enumerate() {
            let val_reg = self.lower_expr(expr)?;

            self.emit(LirInstr::StoreLocal {
                slot: result_slot,
                src: val_reg,
            });

            if i < exprs.len() - 1 {
                let cond_reg = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: cond_reg,
                    slot: result_slot,
                });

                let next_label = self.fresh_label();
                // If truthy, short-circuit to done
                // If falsy, continue to next expression
                self.terminate(Terminator::Branch {
                    cond: cond_reg,
                    then_label: done_label, // ← inverted from lower_and
                    else_label: next_label, // ← inverted from lower_and
                });
                self.finish_block();

                self.current_block = BasicBlock::new(next_label);
            } else {
                self.terminate(Terminator::Jump(done_label));
                self.finish_block();
            }
        }

        self.current_block = BasicBlock::new(done_label);
        let result_reg = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

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

        // Allocate result register and result slot
        let result_reg = self.fresh_reg();
        let result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        let done_label = self.fresh_label();

        // Guard signal safety valve: if any guard may suspend, the decision
        // tree cannot safely backtrack past the guard (it may have yielded).
        // Fall back to sequential matching which doesn't share tests.
        let any_guard_yields = arms
            .iter()
            .any(|(_pat, guard, _body)| guard.as_ref().is_some_and(|g| g.signal.may_suspend()));

        if any_guard_yields {
            self.lower_match_sequential(arms, scrutinee_slot, result_slot, result_reg, done_label)?;
            return Ok(result_reg);
        }

        // Build decision tree
        use super::decision::{AccessPath, PatternMatrix};
        let matrix = PatternMatrix::from_arms(arms);
        let tree = matrix.compile(vec![AccessPath::Root]);

        // Lower decision tree
        let mut lowered_arms = std::collections::HashMap::new();
        self.lower_decision_tree(
            &tree,
            arms,
            scrutinee_slot,
            result_slot,
            done_label,
            &mut lowered_arms,
        )?;

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
