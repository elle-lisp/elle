//! Control flow lowering: and, or, match, handler-case, yield, call

use super::*;
use crate::hir::{CallArg, HirPattern};
use crate::value::fiber::SignalBits;

mod call;

impl<'a> Lowerer<'a> {
    /// Whether a tail-call argument is BORROWED — held by a reference this
    /// activation does not own, so pure-moving it into the callee would
    /// over-free it (see the move-on-tail-call comment in `lower_call`).
    ///
    /// This is a **structural ownership-location** question, NOT true-escape: an
    /// argument is borrowed iff `b` is a captured upvalue of the current lambda
    /// (`upvalue_bindings`) — the closure env owns the capture-incref, so this
    /// frame has no transferable owning reference. The authoritative escape
    /// analysis (`EscapeInfo`) is deliberately NOT used here: among tail-args its
    /// escape set is a strict superset of the borrowed set (a born-here value that
    /// merely flows to a tail *escapes* but is *owned*), and minting for those
    /// owned-escaping args double-releases across a fiber suspend/resume — a
    /// phantom `DecrefRegion` / use-after-free witnessed on `contracts.lisp`. The
    /// env-ownership fact is structural capture, which `is_captured`/`upvalue_bindings`
    /// answer exactly and escape does not; this is the structural-only role of
    /// lexical capture (see `hir::escape` and `docs/impl/escape.md`).
    ///
    /// This borrowed/mint compensation is **transitional value-RC machinery, not a
    /// permanent fixture**: it exists only because today's model mints per
    /// value-escape event. A future ownership-forest model subsumes it — an
    /// intra-fiber captured upvalue lives in an Owned subtree reclaimed by drop
    /// (no mint, no over-free), and only genuine cross-fiber Shared regions keep
    /// edge-RC. The structural capture hint persists (demoted to layout-only);
    /// this predicate does not.
    ///
    /// After ANF a call argument is atomic, and a variable reference takes one of
    /// two shapes (see `functionalize`): a plain `Var(b)`, or `DerefCell(Var(b))`
    /// for a binding that `needs_capture()`. BOTH are borrowed when `b` is a
    /// captured upvalue, so we look THROUGH the `DerefCell` wrapper to the `Var`
    /// (matching only the bare `Var` let a cell-backed top-level binding tail-pass
    /// without the fresh incref — region-tail-move-toplevel-uaf.lisp).
    fn tail_arg_is_borrowed(&self, arg: &Hir) -> bool {
        if !self.in_lambda {
            return false;
        }
        // Look through the `DerefCell` wrapper `functionalize` adds around a
        // needs-capture binding read; the borrowed atom underneath is a `Var`.
        let inner = match &arg.kind {
            HirKind::DerefCell { cell } => cell,
            _ => arg,
        };
        match &inner.kind {
            HirKind::Var(binding) => self.upvalue_bindings.contains(binding),
            _ => false,
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

        let Some(&intrinsic) = self.intrinsics.get(&sym) else {
            return Ok(None);
        };

        match intrinsic {
            IntrinsicOp::Conversion(op) => {
                if args.len() != 1 {
                    return Ok(None); // 2-arg (integer str radix) falls through to Call
                }
                let src = self.lower_expr(args[0])?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::Convert { dst, op, src });
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
        // Eval's result lives in a region the outer compilation
        // didn't allocate; `emit_decrefs_for` uses `region_to_slot`
        // (recorded by the enclosing binding site after ANF) and
        // gates the runtime decref on the actual region.
        Ok(dst)
    }

    pub(super) fn lower_emit(
        &mut self,
        signal: crate::value::fiber::SignalBits,
        value: &Hir,
    ) -> Result<Reg, String> {
        // Region inference stamps yield-bound allocations with the Parent
        // region via alloc_region. No OutboxEnter/OutboxExit toggle needed.
        let value_reg = self.lower_expr(value)?;

        let resume_label = self.fresh_label();

        self.terminate(Terminator::Emit {
            signal,
            value: value_reg,
            resume_label,
        });

        self.start_new_block(resume_label);

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
        use crate::hir::decision::{AccessPath, PatternMatrix};
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
    ///
    /// Each arm's top-level or-pattern is expanded into its alternatives, and
    /// **each alternative re-checks the arm's guard**: a failed guard retries
    /// the next alternative (re-binding from a different structural position)
    /// before the match moves on to the next arm (docs/match.md § Guards). All
    /// alternatives of one arm share a single lowered body — the or-pattern
    /// binds the same variables in every alternative, so the body reads them
    /// from the same slots regardless of which alternative matched, and one
    /// body copy keeps cell initialization (`MakeCapture`) from being emitted
    /// only on the first alternative's path.
    fn lower_match_sequential(
        &mut self,
        arms: &[(HirPattern, Option<Hir>, Hir)],
        scrutinee_slot: u16,
        result_slot: u16,
        result_reg: Reg,
        done_label: Label,
    ) -> Result<(), String> {
        use crate::hir::decision::expand_or_pattern;

        // Pre-allocate an entry label for each arm.
        let arm_labels: Vec<Label> = (0..arms.len()).map(|_| self.fresh_label()).collect();
        let no_match_label = self.fresh_label();

        for (i, (pattern, guard, body)) in arms.iter().enumerate() {
            let next_arm_label = if i + 1 < arms.len() {
                arm_labels[i + 1]
            } else {
                no_match_label
            };

            // The body is lowered once and shared by every alternative that
            // reaches it (via its guard passing, or unconditionally when the
            // arm has no guard).
            let body_label = self.fresh_label();
            let alternatives = expand_or_pattern(pattern);

            for (j, alt) in alternatives.iter().enumerate() {
                // Where a structural mismatch or a failed guard on this
                // alternative goes: the next alternative, or the next arm when
                // this is the last alternative.
                let next_label = if j + 1 < alternatives.len() {
                    self.fresh_label()
                } else {
                    next_arm_label
                };

                // Reload the scrutinee for this alternative's test.
                let alt_value_reg = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: alt_value_reg,
                    slot: scrutinee_slot,
                });

                self.lower_pattern_match(alt, alt_value_reg, next_label)?;

                if let Some(guard_expr) = guard {
                    let guard_reg = self.lower_expr(guard_expr)?;
                    self.terminate(Terminator::Branch {
                        cond: guard_reg,
                        then_label: body_label,
                        else_label: next_label,
                    });
                } else {
                    self.terminate(Terminator::Jump(body_label));
                }
                self.finish_block();

                // Start the next alternative's block (the last alternative's
                // `next_label` is another arm's block, opened by the outer loop).
                if j + 1 < alternatives.len() {
                    self.current_block = BasicBlock::new(next_label);
                }
            }

            // Shared body block for this arm.
            self.current_block = BasicBlock::new(body_label);
            let body_reg = self.lower_expr(body)?;
            self.emit(LirInstr::StoreLocal {
                slot: result_slot,
                src: body_reg,
            });
            self.terminate(Terminator::Jump(done_label));
            self.finish_block();

            // Start the next arm's block.
            if i + 1 < arms.len() {
                self.current_block = BasicBlock::new(arm_labels[i + 1]);
            }
        }

        // No match block: raise :match-error carrying the scrutinee
        self.current_block = BasicBlock::new(no_match_label);
        self.emit_no_match(scrutinee_slot, result_slot, done_label)?;

        // Done block
        self.current_block = BasicBlock::new(done_label);
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

        Ok(())
    }
}
