//! Decision tree lowering to LIR instructions

use super::decision::{AccessPath, Constructor, DecisionTree};
use super::*;
use crate::hir::{HirPattern, PatternKey, PatternLiteral};

impl Lowerer {
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
    ) -> Result<(), String> {
        match tree {
            DecisionTree::Fail => {
                // Defensive: should not happen with exhaustiveness checking.
                // Emit nil as the result.
                let nil_reg = self.emit_const(LirConst::Nil)?;
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: nil_reg,
                });
                self.terminate(Terminator::Jump(done_label));
                self.finish_block();
                Ok(())
            }
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
                    if self.in_lambda {
                        self.upvalue_bindings.insert(*binding);
                        self.emit(LirInstr::StoreCapture {
                            index: slot,
                            src: val_reg,
                        });
                    } else {
                        self.emit(LirInstr::StoreLocal { slot, src: val_reg });
                    }
                }
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
                    if self.in_lambda {
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
                self.lower_decision_tree(otherwise, arms, scrutinee_slot, result_slot, done_label)
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

                    // Emit constructor test (may create blocks for Tuple/Array)
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
                    )?;

                    // Start next test block (if not the last case)
                    if i + 1 < cases.len() {
                        self.current_block = BasicBlock::new(next_label);
                    }
                }

                // Default block
                self.current_block = BasicBlock::new(default_label);
                if let Some(def) = default {
                    self.lower_decision_tree(def, arms, scrutinee_slot, result_slot, done_label)?;
                } else {
                    // No default → fail (non-exhaustive)
                    let nil_reg = self.emit_const(LirConst::Nil)?;
                    self.emit(LirInstr::StoreLocal {
                        slot: result_slot,
                        src: nil_reg,
                    });
                    self.terminate(Terminator::Jump(done_label));
                    self.finish_block();
                }

                Ok(())
            }
        }
    }

    /// Load a value by following an access path from the scrutinee.
    fn load_access_path(
        &mut self,
        access: &AccessPath,
        scrutinee_slot: u16,
    ) -> Result<Reg, String> {
        match access {
            AccessPath::Root => {
                let dst = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst,
                    slot: scrutinee_slot,
                });
                Ok(dst)
            }
            AccessPath::Car(inner) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::Car { dst, pair: parent });
                Ok(dst)
            }
            AccessPath::Cdr(inner) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::Cdr { dst, pair: parent });
                Ok(dst)
            }
            AccessPath::Index(inner, idx) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::ArrayMutRefOrNil {
                    dst,
                    src: parent,
                    index: *idx as u16,
                });
                Ok(dst)
            }
            AccessPath::Slice(inner, start) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::ArrayMutSliceFrom {
                    dst,
                    src: parent,
                    index: *start as u16,
                });
                Ok(dst)
            }
            AccessPath::Key(inner, key) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                let lir_key = match key {
                    PatternKey::Keyword(k) => LirConst::Keyword(k.clone()),
                    PatternKey::Symbol(sid) => LirConst::Symbol(*sid),
                };
                self.emit(LirInstr::TableGetOrNil {
                    dst,
                    src: parent,
                    key: lir_key,
                });
                Ok(dst)
            }
        }
    }

    /// Emit a constructor test, returning a register holding the boolean result.
    ///
    /// For simple constructors (literals, Cons, Nil, EmptyList, Struct, Table),
    /// emits a single test instruction. For Tuple and Array, emits a multi-block
    /// type+length check sequence.
    fn emit_constructor_test(&mut self, value_reg: Reg, ctor: &Constructor) -> Result<Reg, String> {
        match ctor {
            Constructor::Literal(lit) => {
                let lit_reg = match lit {
                    PatternLiteral::Bool(b) => self.emit_const(LirConst::Bool(*b))?,
                    PatternLiteral::Int(n) => self.emit_const(LirConst::Int(*n))?,
                    PatternLiteral::Float(f) => self.emit_const(LirConst::Float(*f))?,
                    PatternLiteral::String(s) => self.emit_const(LirConst::String(s.clone()))?,
                    PatternLiteral::Keyword(k) => self.emit_const(LirConst::Keyword(k.clone()))?,
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
            Constructor::Cons => {
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
            Constructor::Tuple(n) => self.emit_type_and_length_test(value_reg, *n, true, CmpOp::Eq),
            Constructor::TupleRest(n) => {
                self.emit_type_and_length_test(value_reg, *n, true, CmpOp::Ge)
            }
            Constructor::Array(n) => {
                self.emit_type_and_length_test(value_reg, *n, false, CmpOp::Eq)
            }
            Constructor::ArrayRest(n) => {
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
                self.emit(LirInstr::IsTable {
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
            self.emit(LirInstr::IsTuple {
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
