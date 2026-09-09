// audited: 2026-09-08
//! `match` lowering: the decision tree, and the sequential fallback a suspending
//! guard forces.
//!
//! docs/match.md

use super::*;

impl<'a> Lowerer<'a> {
    pub(in crate::lir::lower) fn lower_match(
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

        // The done block is reached through an arm body or the no-match block,
        // each of which seals its relocation points here for the merge to
        // inherit (docs/impl/region/replicate.md § "The relocation point outlives
        // the block"). The no-match block makes no tail call, so it contributes
        // none — which costs nothing, since a point is only ever a licence to
        // replicate, never an obligation to.
        let branch_hoists = self.begin_branch_arms();

        if any_guard_yields {
            self.lower_match_sequential(arms, scrutinee_slot, result_slot, result_reg, done_label)?;
            self.open_branch_merge(branch_hoists);
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
        self.open_branch_merge(branch_hoists);
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
            self.seal_arm_hoists();
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
