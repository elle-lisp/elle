//! Pattern matching lowering

use super::*;
use crate::hir::decision::{Constructor, DecisionTree};
use crate::hir::{HirPattern, PatternKey, PatternLiteral};

mod keyed;
mod matching;
mod seq;

impl<'a> Lowerer<'a> {
    // ── Decision tree lowering ─────────────────────────────────────

    /// Emit the no-match path: raise :match-error carrying the scrutinee.
    /// The store and jump after MatchFail are NOT dead: errors are
    /// resumable, and a fiber that catches SIG_ERROR resumes here with
    /// the handler's value pushed — the store makes that value the
    /// match expression's result (same convention as the destructure
    /// instructions).
    pub(super) fn emit_no_match(
        &mut self,
        scrutinee_slot: u16,
        result_slot: u16,
        done_label: Label,
    ) -> Result<(), String> {
        let scrut = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: scrut,
            slot: scrutinee_slot,
        });
        let dst = self.fresh_reg();
        self.emit(LirInstr::MatchFail { dst, src: scrut });
        self.emit(LirInstr::StoreLocal {
            slot: result_slot,
            src: dst,
        });
        self.terminate(Terminator::Jump(done_label));
        self.finish_block();
        Ok(())
    }

    /// Lower a compiled decision tree to LIR instructions.
    ///
    /// Walks the tree recursively, emitting constructor tests, bindings,
    /// guard checks, and arm bodies. Each tree node becomes one or more
    /// basic blocks.
    ///
    /// The scrutinee and result live in local slots (not on the operand
    /// stack).  The emitter pre-allocates space for all locals at the
    /// start of the entry block, so StoreLocal never clobbers operand
    /// values from enclosing expressions.
    pub(super) fn lower_decision_tree(
        &mut self,
        tree: &DecisionTree,
        arms: &[(HirPattern, Option<Hir>, Hir)],
        scrutinee_slot: u16,
        result_slot: u16,
        done_label: Label,
        lowered_arms: &mut std::collections::HashMap<usize, Label>,
    ) -> Result<(), String> {
        match tree {
            DecisionTree::Fail => self.emit_no_match(scrutinee_slot, result_slot, done_label),
            DecisionTree::Leaf {
                arm_index,
                bindings,
            } => {
                // Establish bindings by loading values at their access paths.
                // Pop after each store — the value lives in the slot/capture
                // and keeping it on the operand stack would leak intermediates.
                for (binding, access) in bindings {
                    let val_reg = self.load_access_path(access, scrutinee_slot)?;
                    let slot = if let Some(&existing) = self.binding_to_slot.get(binding) {
                        existing
                    } else {
                        self.allocate_slot(*binding)
                    };
                    let needs_capture = self.arena.get(*binding).needs_capture();
                    if self.in_lambda && needs_capture {
                        self.upvalue_bindings.insert(*binding);
                        self.emit(LirInstr::StoreCapture {
                            index: slot,
                            src: val_reg,
                        });
                    } else {
                        self.emit(LirInstr::StoreLocal { slot, src: val_reg });
                    }
                }

                // If this arm's body was already lowered (e.g., multiple cases
                // in an or-pattern reaching the same arm), jump to the existing
                // body instead of re-lowering it.  Re-lowering would share
                // binding slots but only initialize cells (MakeCapture) in the
                // first copy, causing "Expected cell, got ..." panics when a
                // later copy runs at runtime.
                if let Some(&body_label) = lowered_arms.get(arm_index) {
                    self.terminate(Terminator::Jump(body_label));
                    self.finish_block();
                    return Ok(());
                }

                // First time lowering this arm — record its label for reuse.
                let body_label = self.fresh_label();
                lowered_arms.insert(*arm_index, body_label);
                self.terminate(Terminator::Jump(body_label));
                self.finish_block();
                self.current_block = BasicBlock::new(body_label);

                // Lower body
                let body = &arms[*arm_index].2;
                let body_reg = self.lower_expr(body)?;
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: body_reg,
                });
                self.terminate(Terminator::Jump(done_label));
                self.finish_block();
                Ok(())
            }
            DecisionTree::Guard {
                arm_index,
                bindings,
                otherwise,
            } => {
                // Establish bindings — pop after each store (same as Leaf).
                for (binding, access) in bindings {
                    let val_reg = self.load_access_path(access, scrutinee_slot)?;
                    let slot = if let Some(&existing) = self.binding_to_slot.get(binding) {
                        existing
                    } else {
                        self.allocate_slot(*binding)
                    };
                    let needs_capture = self.arena.get(*binding).needs_capture();
                    if self.in_lambda && needs_capture {
                        self.upvalue_bindings.insert(*binding);
                        self.emit(LirInstr::StoreCapture {
                            index: slot,
                            src: val_reg,
                        });
                    } else {
                        self.emit(LirInstr::StoreLocal { slot, src: val_reg });
                    }
                }
                // Evaluate guard
                let guard_expr = arms[*arm_index]
                    .1
                    .as_ref()
                    .expect("Guard node must have guard expression");
                let guard_reg = self.lower_expr(guard_expr)?;

                let pass_label = self.fresh_label();
                let fail_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: guard_reg,
                    then_label: pass_label,
                    else_label: fail_label,
                });
                self.finish_block();

                // Guard passed: lower body
                self.current_block = BasicBlock::new(pass_label);
                let body = &arms[*arm_index].2;
                let body_reg = self.lower_expr(body)?;
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: body_reg,
                });
                self.terminate(Terminator::Jump(done_label));
                self.finish_block();

                // Guard failed: continue with otherwise
                self.current_block = BasicBlock::new(fail_label);
                self.lower_decision_tree(
                    otherwise,
                    arms,
                    scrutinee_slot,
                    result_slot,
                    done_label,
                    lowered_arms,
                )
            }
            DecisionTree::Switch {
                access,
                cases,
                default,
            } => {
                // Load value at access path, store to temp slot, then pop
                // from the operand stack.  The value lives in the local
                // slot and is reloaded via LoadLocal for each constructor
                // test.
                let value_reg = self.load_access_path(access, scrutinee_slot)?;
                let temp_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                let default_label = self.fresh_label();

                // Emit if-else chain for each constructor
                for (i, (ctor, subtree)) in cases.iter().enumerate() {
                    let match_label = self.fresh_label();
                    let next_label = if i + 1 < cases.len() {
                        self.fresh_label()
                    } else {
                        default_label
                    };

                    // Reload value for this test
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });

                    // Emit constructor test (may create blocks for Array/@array)
                    let test_reg = self.emit_constructor_test(reloaded, ctor)?;
                    self.terminate(Terminator::Branch {
                        cond: test_reg,
                        then_label: match_label,
                        else_label: next_label,
                    });
                    self.finish_block();

                    // Match block: recurse into subtree
                    self.current_block = BasicBlock::new(match_label);
                    self.lower_decision_tree(
                        subtree,
                        arms,
                        scrutinee_slot,
                        result_slot,
                        done_label,
                        lowered_arms,
                    )?;

                    // Start next test block (if not the last case)
                    if i + 1 < cases.len() {
                        self.current_block = BasicBlock::new(next_label);
                    }
                }

                // Default block
                self.current_block = BasicBlock::new(default_label);
                if let Some(def) = default {
                    self.lower_decision_tree(
                        def,
                        arms,
                        scrutinee_slot,
                        result_slot,
                        done_label,
                        lowered_arms,
                    )?;
                } else {
                    // No default → no constructor matched the scrutinee
                    self.emit_no_match(scrutinee_slot, result_slot, done_label)?;
                }

                Ok(())
            }
        }
    }

    /// Emit a constructor test, returning a register holding the boolean result.
    ///
    /// For simple constructors (literals, Pair, Nil, EmptyList, Struct, Table),
    /// emits a single test instruction. For Tuple and Array, emits a multi-block
    /// type+length check sequence.
    fn emit_constructor_test(&mut self, value_reg: Reg, ctor: &Constructor) -> Result<Reg, String> {
        match ctor {
            Constructor::Literal(lit) => {
                // A STRING literal compares by content but is a HEAP value, so —
                // unlike the immediate literals below — it cannot be a pooled
                // constant. Materialize it FRESH into a transient per-activation
                // region, compare, then free that region immediately: a heap
                // literal is an ordinary, reclaimable allocation (region-model.md,
                // "Constants lower as ordinary allocations"), never a process-
                // pinned constant. The string is dead the instant the comparison
                // reads it, so the region's whole life is these three instructions.
                if let PatternLiteral::String(s) = lit {
                    let str_reg = self.fresh_reg();
                    let region = self.fresh_managed_region();
                    self.emit(LirInstr::MaterializeConst {
                        dst: str_reg,
                        template: crate::value::ConstTemplate::String(s.clone()),
                        region,
                    });
                    let dst = self.fresh_reg();
                    self.emit(LirInstr::Compare {
                        dst,
                        op: CmpOp::Eq,
                        lhs: value_reg,
                        rhs: str_reg,
                    });
                    self.emit(LirInstr::DecrefRegion { region_id: region });
                    return Ok(dst);
                }
                let lit_reg = match lit {
                    PatternLiteral::Bool(b) => self.emit_const(LirConst::Bool(*b))?,
                    PatternLiteral::Int(n) => self.emit_const(LirConst::Int(*n))?,
                    PatternLiteral::Float(f) => self.emit_const(LirConst::Float(*f))?,
                    PatternLiteral::Keyword(k) => self.emit_const(LirConst::Keyword(k.clone()))?,
                    PatternLiteral::String(_) => unreachable!("string handled above"),
                };
                let dst = self.fresh_reg();
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Eq,
                    lhs: value_reg,
                    rhs: lit_reg,
                });
                Ok(dst)
            }
            Constructor::Pair => {
                let dst = self.fresh_reg();
                self.emit(LirInstr::IsPair {
                    dst,
                    src: value_reg,
                });
                Ok(dst)
            }
            Constructor::Nil => {
                let dst = self.fresh_reg();
                self.emit(LirInstr::IsNil {
                    dst,
                    src: value_reg,
                });
                Ok(dst)
            }
            Constructor::EmptyList => {
                let empty_reg = self.fresh_reg();
                self.emit(LirInstr::ValueConst {
                    dst: empty_reg,
                    value: Value::EMPTY_LIST,
                });
                let dst = self.fresh_reg();
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Eq,
                    lhs: value_reg,
                    rhs: empty_reg,
                });
                Ok(dst)
            }
            Constructor::Array(n) => self.emit_type_and_length_test(value_reg, *n, true, CmpOp::Eq),
            Constructor::ArrayRest(n) => {
                self.emit_type_and_length_test(value_reg, *n, true, CmpOp::Ge)
            }
            Constructor::ArrayMut(n) => {
                self.emit_type_and_length_test(value_reg, *n, false, CmpOp::Eq)
            }
            Constructor::ArrayMutRest(n) => {
                self.emit_type_and_length_test(value_reg, *n, false, CmpOp::Ge)
            }
            Constructor::Struct(_) => {
                let dst = self.fresh_reg();
                self.emit(LirInstr::IsStruct {
                    dst,
                    src: value_reg,
                });
                Ok(dst)
            }
            Constructor::Table(_) => {
                let dst = self.fresh_reg();
                self.emit(LirInstr::IsStructMut {
                    dst,
                    src: value_reg,
                });
                Ok(dst)
            }
            Constructor::Set => {
                let dst = self.fresh_reg();
                self.emit(LirInstr::IsSet {
                    dst,
                    src: value_reg,
                });
                Ok(dst)
            }
            Constructor::SetMut => {
                let dst = self.fresh_reg();
                self.emit(LirInstr::IsSetMut {
                    dst,
                    src: value_reg,
                });
                Ok(dst)
            }
        }
    }

    /// Emit a type check + length check for Tuple or Array constructors.
    ///
    /// Creates multiple blocks: type check → length check → result merge.
    /// Returns a register holding the boolean result in the merge block.
    fn emit_type_and_length_test(
        &mut self,
        value_reg: Reg,
        n: usize,
        is_tuple: bool,
        len_cmp: CmpOp,
    ) -> Result<Reg, String> {
        // Store value to temp slot so we can reload after block boundaries.
        let val_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        self.emit(LirInstr::StoreLocal {
            slot: val_slot,
            src: value_reg,
        });

        // Reload for type check (auto-pop consumed value_reg)
        let reloaded_for_type = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: reloaded_for_type,
            slot: val_slot,
        });

        let type_check_reg = self.fresh_reg();
        if is_tuple {
            self.emit(LirInstr::IsArray {
                dst: type_check_reg,
                src: reloaded_for_type,
            });
        } else {
            self.emit(LirInstr::IsArrayMut {
                dst: type_check_reg,
                src: reloaded_for_type,
            });
        }

        let len_check_label = self.fresh_label();
        let fail_label = self.fresh_label();
        let pass_label = self.fresh_label();
        self.terminate(Terminator::Branch {
            cond: type_check_reg,
            then_label: len_check_label,
            else_label: fail_label,
        });
        self.finish_block();

        // Length check block — reload value from temp slot
        self.current_block = BasicBlock::new(len_check_label);
        let reloaded = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: reloaded,
            slot: val_slot,
        });
        let len_reg = self.fresh_reg();
        self.emit(LirInstr::ArrayMutLen {
            dst: len_reg,
            src: reloaded,
        });
        let expected_reg = self.emit_const(LirConst::Int(n as i64))?;
        let len_ok = self.fresh_reg();
        self.emit(LirInstr::Compare {
            dst: len_ok,
            op: len_cmp,
            lhs: len_reg,
            rhs: expected_reg,
        });
        self.terminate(Terminator::Branch {
            cond: len_ok,
            then_label: pass_label,
            else_label: fail_label,
        });
        self.finish_block();

        // Use a local slot to merge the boolean result across blocks
        let merge_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;

        // Fail block: result = false
        self.current_block = BasicBlock::new(fail_label);
        let false_reg = self.emit_const(LirConst::Bool(false))?;
        let result_label = self.fresh_label();
        self.emit(LirInstr::StoreLocal {
            slot: merge_slot,
            src: false_reg,
        });
        self.terminate(Terminator::Jump(result_label));
        self.finish_block();

        // Pass block: result = true
        self.current_block = BasicBlock::new(pass_label);
        let true_reg = self.emit_const(LirConst::Bool(true))?;
        self.emit(LirInstr::StoreLocal {
            slot: merge_slot,
            src: true_reg,
        });
        self.terminate(Terminator::Jump(result_label));
        self.finish_block();

        // Result block: load the boolean
        self.current_block = BasicBlock::new(result_label);
        let dst = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst,
            slot: merge_slot,
        });
        Ok(dst)
    }
}
