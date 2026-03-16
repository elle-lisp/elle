//! Pattern matching lowering

use super::decision::{AccessPath, Constructor, DecisionTree};
use super::*;
use crate::hir::{HirPattern, PatternKey, PatternLiteral};

impl Lowerer {
    // ── Decision tree lowering ─────────────────────────────────────

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
                self.emit(LirInstr::ArrayMutRefDestructure {
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
                self.emit(LirInstr::StructGetOrNil {
                    dst,
                    src: parent,
                    key: lir_key,
                });
                Ok(dst)
            }
            AccessPath::StructRest(inner, exclude_keys) => {
                let src_reg = self.load_access_path(inner, scrutinee_slot)?;
                let rest_reg = self.fresh_reg();
                let lir_exclude: Vec<LirConst> = exclude_keys
                    .iter()
                    .map(|k| match k {
                        PatternKey::Keyword(s) => LirConst::Keyword(s.clone()),
                        PatternKey::Symbol(sid) => LirConst::Symbol(*sid),
                    })
                    .collect();
                self.emit(LirInstr::StructRest {
                    dst: rest_reg,
                    src: src_reg,
                    exclude_keys: lir_exclude,
                });
                Ok(rest_reg)
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

    /// Lower pattern matching code
    /// Emits code that checks if value_reg matches the pattern
    /// If it doesn't match, branches to fail_label
    /// If it matches, binds any variables and continues in the current block
    pub(super) fn lower_pattern_match(
        &mut self,
        pattern: &HirPattern,
        value_reg: Reg,
        fail_label: Label,
    ) -> Result<(), String> {
        match pattern {
            HirPattern::Wildcard => {
                // Wildcard always matches, do nothing
                Ok(())
            }
            HirPattern::Nil => {
                // Check if value is nil (NOT empty_list)
                // nil and '() are distinct values with distinct semantics
                let is_nil_reg = self.fresh_reg();
                self.emit(LirInstr::IsNil {
                    dst: is_nil_reg,
                    src: value_reg,
                });

                // If NOT nil, fail; otherwise continue
                let continue_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: is_nil_reg,
                    then_label: continue_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(continue_label);

                Ok(())
            }
            HirPattern::Literal(lit) => {
                // Check if value equals literal
                let lit_reg = match lit {
                    PatternLiteral::Bool(b) => self.emit_const(LirConst::Bool(*b))?,
                    PatternLiteral::Int(n) => self.emit_const(LirConst::Int(*n))?,
                    PatternLiteral::Float(f) => self.emit_const(LirConst::Float(*f))?,
                    PatternLiteral::String(s) => self.emit_const(LirConst::String(s.clone()))?,
                    PatternLiteral::Keyword(name) => {
                        self.emit_const(LirConst::Keyword(name.clone()))?
                    }
                };

                let eq_reg = self.fresh_reg();
                self.emit(LirInstr::Compare {
                    dst: eq_reg,
                    op: CmpOp::Eq,
                    lhs: value_reg,
                    rhs: lit_reg,
                });

                let continue_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: eq_reg,
                    then_label: continue_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(continue_label);

                Ok(())
            }
            HirPattern::Var(binding) => {
                // Bind the value to the variable.
                // If the binding already has a slot (e.g., from a previous
                // or-pattern alternative), reuse it instead of allocating a new one.
                let slot = if let Some(&existing) = self.binding_to_slot.get(binding) {
                    existing
                } else {
                    self.allocate_slot(*binding)
                };
                // Inside lambdas, pattern-bound variables are part of the closure environment
                if self.in_lambda {
                    self.upvalue_bindings.insert(*binding);
                    self.emit(LirInstr::StoreCapture {
                        index: slot,
                        src: value_reg,
                    });
                } else {
                    self.emit(LirInstr::StoreLocal {
                        slot,
                        src: value_reg,
                    });
                }
                Ok(())
            }
            HirPattern::Cons { head, tail } => {
                // Store value to temp slot before any operations, so we can
                // reload it after the block boundary.
                // Inside a lambda, slots need to account for the captures offset.
                let temp_slot = if self.in_lambda {
                    self.num_captures + self.current_func.num_locals
                } else {
                    self.current_func.num_locals
                };
                self.current_func.num_locals += 1;

                if self.in_lambda {
                    self.emit(LirInstr::StoreCapture {
                        index: temp_slot,
                        src: value_reg,
                    });
                } else {
                    self.emit(LirInstr::StoreLocal {
                        slot: temp_slot,
                        src: value_reg,
                    });
                }

                // Reload for type check (auto-pop consumed value_reg)
                let reloaded_for_check = self.fresh_reg();
                if self.in_lambda {
                    self.emit(LirInstr::LoadCapture {
                        dst: reloaded_for_check,
                        index: temp_slot,
                    });
                } else {
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded_for_check,
                        slot: temp_slot,
                    });
                }

                // Check if value is a pair
                let is_pair_reg = self.fresh_reg();
                self.emit(LirInstr::IsPair {
                    dst: is_pair_reg,
                    src: reloaded_for_check,
                });

                let continue_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: is_pair_reg,
                    then_label: continue_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(continue_label);

                // Extract car, match head pattern, THEN extract cdr and match tail.
                // This ordering is critical: the head pattern match may create
                // block boundaries (e.g., nested cons, or-patterns), which
                // invalidate registers from the current block. By extracting
                // cdr AFTER the head match, we reload from the temp slot in
                // whatever block the head match left us in.

                // Reload for car
                let reloaded_for_car = self.fresh_reg();
                if self.in_lambda {
                    self.emit(LirInstr::LoadCapture {
                        dst: reloaded_for_car,
                        index: temp_slot,
                    });
                } else {
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded_for_car,
                        slot: temp_slot,
                    });
                }

                let head_reg = self.fresh_reg();
                self.emit(LirInstr::Car {
                    dst: head_reg,
                    pair: reloaded_for_car,
                });

                // Match head pattern first (may create block boundaries)
                self.lower_pattern_match(head, head_reg, fail_label)?;

                // Now reload for cdr — in whatever block the head match left us in
                let reloaded_for_cdr = self.fresh_reg();
                if self.in_lambda {
                    self.emit(LirInstr::LoadCapture {
                        dst: reloaded_for_cdr,
                        index: temp_slot,
                    });
                } else {
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded_for_cdr,
                        slot: temp_slot,
                    });
                }

                let tail_reg = self.fresh_reg();
                self.emit(LirInstr::Cdr {
                    dst: tail_reg,
                    pair: reloaded_for_cdr,
                });

                // Match tail pattern
                self.lower_pattern_match(tail, tail_reg, fail_label)?;

                Ok(())
            }
            HirPattern::List { elements, rest } => {
                // Check if value is a list of the right length
                // Iterate through patterns and match each element

                let mut current_reg = value_reg;

                for pat in elements.iter() {
                    // Store current to a temporary slot BEFORE IsPair, so we can
                    // reload it after the block boundary.
                    // Inside a lambda, slots need to account for the captures offset.
                    let temp_slot = if self.in_lambda {
                        self.num_captures + self.current_func.num_locals
                    } else {
                        self.current_func.num_locals
                    };
                    self.current_func.num_locals += 1;

                    if self.in_lambda {
                        self.emit(LirInstr::StoreCapture {
                            index: temp_slot,
                            src: current_reg,
                        });
                    } else {
                        self.emit(LirInstr::StoreLocal {
                            slot: temp_slot,
                            src: current_reg,
                        });
                    }

                    // Reload for type check (auto-pop consumed current_reg)
                    let reloaded_for_check = self.fresh_reg();
                    if self.in_lambda {
                        self.emit(LirInstr::LoadCapture {
                            dst: reloaded_for_check,
                            index: temp_slot,
                        });
                    } else {
                        self.emit(LirInstr::LoadLocal {
                            dst: reloaded_for_check,
                            slot: temp_slot,
                        });
                    }

                    // Check if current is a pair
                    let is_pair_reg = self.fresh_reg();
                    self.emit(LirInstr::IsPair {
                        dst: is_pair_reg,
                        src: reloaded_for_check,
                    });

                    let continue_label = self.fresh_label();
                    self.terminate(Terminator::Branch {
                        cond: is_pair_reg,
                        then_label: continue_label,
                        else_label: fail_label,
                    });
                    self.finish_block();
                    self.current_block = BasicBlock::new(continue_label);

                    // Load for car extraction
                    let current_for_car = self.fresh_reg();
                    if self.in_lambda {
                        self.emit(LirInstr::LoadCapture {
                            dst: current_for_car,
                            index: temp_slot,
                        });
                    } else {
                        self.emit(LirInstr::LoadLocal {
                            dst: current_for_car,
                            slot: temp_slot,
                        });
                    }

                    // Extract head
                    let head_reg = self.fresh_reg();
                    self.emit(LirInstr::Car {
                        dst: head_reg,
                        pair: current_for_car,
                    });

                    // Match head against pattern
                    self.lower_pattern_match(pat, head_reg, fail_label)?;

                    // Load for cdr extraction — always needed for next
                    // element, rest binding, or EMPTY_LIST check at end
                    let current_for_cdr = self.fresh_reg();
                    if self.in_lambda {
                        self.emit(LirInstr::LoadCapture {
                            dst: current_for_cdr,
                            index: temp_slot,
                        });
                    } else {
                        self.emit(LirInstr::LoadLocal {
                            dst: current_for_cdr,
                            slot: temp_slot,
                        });
                    }

                    // Extract tail for next iteration
                    let tail_reg = self.fresh_reg();
                    self.emit(LirInstr::Cdr {
                        dst: tail_reg,
                        pair: current_for_cdr,
                    });

                    current_reg = tail_reg;
                }

                if let Some(rest_pat) = rest {
                    // With & rest: bind remaining tail to rest pattern
                    self.lower_pattern_match(rest_pat, current_reg, fail_label)?;
                } else {
                    // Without rest: check that tail is empty_list (exact length)
                    let empty_list_reg = self.fresh_reg();
                    self.emit(LirInstr::ValueConst {
                        dst: empty_list_reg,
                        value: Value::EMPTY_LIST,
                    });
                    let is_empty_reg = self.fresh_reg();
                    self.emit(LirInstr::Compare {
                        dst: is_empty_reg,
                        op: CmpOp::Eq,
                        lhs: current_reg,
                        rhs: empty_list_reg,
                    });

                    let continue_label = self.fresh_label();
                    self.terminate(Terminator::Branch {
                        cond: is_empty_reg,
                        then_label: continue_label,
                        else_label: fail_label,
                    });
                    self.finish_block();
                    self.current_block = BasicBlock::new(continue_label);
                }

                Ok(())
            }
            HirPattern::Tuple { elements, rest } => {
                // Array [...] pattern matching for `match`.
                // Check if value is an array, then use ArrayMutRefDestructure for each element.
                let temp_slot = if self.in_lambda {
                    self.num_captures + self.current_func.num_locals
                } else {
                    self.current_func.num_locals
                };
                self.current_func.num_locals += 1;

                if self.in_lambda {
                    self.emit(LirInstr::StoreCapture {
                        index: temp_slot,
                        src: value_reg,
                    });
                } else {
                    self.emit(LirInstr::StoreLocal {
                        slot: temp_slot,
                        src: value_reg,
                    });
                }

                // Step 2: Check if value is an array
                let is_tuple_reg = self.fresh_reg();
                self.emit(LirInstr::IsArray {
                    dst: is_tuple_reg,
                    src: value_reg,
                });

                let type_ok_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: is_tuple_reg,
                    then_label: type_ok_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(type_ok_label);

                // Step 3: Check array length
                // Reload from temp slot
                let reloaded_for_len = self.fresh_reg();
                if self.in_lambda {
                    self.emit(LirInstr::LoadCapture {
                        dst: reloaded_for_len,
                        index: temp_slot,
                    });
                } else {
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded_for_len,
                        slot: temp_slot,
                    });
                }

                let len_reg = self.fresh_reg();
                self.emit(LirInstr::ArrayMutLen {
                    dst: len_reg,
                    src: reloaded_for_len,
                });

                let expected_len = self.emit_const(LirConst::Int(elements.len() as i64))?;
                let len_ok_reg = self.fresh_reg();

                if rest.is_some() {
                    // With & rest: length must be >= number of fixed elements
                    self.emit(LirInstr::Compare {
                        dst: len_ok_reg,
                        op: CmpOp::Ge,
                        lhs: len_reg,
                        rhs: expected_len,
                    });
                } else {
                    // Without rest: length must be exactly equal
                    self.emit(LirInstr::Compare {
                        dst: len_ok_reg,
                        op: CmpOp::Eq,
                        lhs: len_reg,
                        rhs: expected_len,
                    });
                }

                let len_ok_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: len_ok_reg,
                    then_label: len_ok_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(len_ok_label);

                // Step 4: Match each element using ArrayMutRefDestructure
                for (i, element_pat) in elements.iter().enumerate() {
                    // Reload the array from temp slot for each element
                    let reloaded = self.fresh_reg();
                    if self.in_lambda {
                        self.emit(LirInstr::LoadCapture {
                            dst: reloaded,
                            index: temp_slot,
                        });
                    } else {
                        self.emit(LirInstr::LoadLocal {
                            dst: reloaded,
                            slot: temp_slot,
                        });
                    }

                    let elem_reg = self.fresh_reg();
                    self.emit(LirInstr::ArrayMutRefDestructure {
                        dst: elem_reg,
                        src: reloaded,
                        index: i as u16,
                    });

                    // Recursively match the element
                    self.lower_pattern_match(element_pat, elem_reg, fail_label)?;
                }

                // Step 5: Handle & rest
                if let Some(rest_pat) = rest {
                    let reloaded = self.fresh_reg();
                    if self.in_lambda {
                        self.emit(LirInstr::LoadCapture {
                            dst: reloaded,
                            index: temp_slot,
                        });
                    } else {
                        self.emit(LirInstr::LoadLocal {
                            dst: reloaded,
                            slot: temp_slot,
                        });
                    }

                    let slice_reg = self.fresh_reg();
                    self.emit(LirInstr::ArrayMutSliceFrom {
                        dst: slice_reg,
                        src: reloaded,
                        index: elements.len() as u16,
                    });

                    self.lower_pattern_match(rest_pat, slice_reg, fail_label)?;
                }

                Ok(())
            }
            HirPattern::Array { elements, rest } => {
                // Array @[...] pattern matching for `match`.
                // Check if value is an array, then use ArrayMutRefDestructure for each element.
                let temp_slot = if self.in_lambda {
                    self.num_captures + self.current_func.num_locals
                } else {
                    self.current_func.num_locals
                };
                self.current_func.num_locals += 1;

                if self.in_lambda {
                    self.emit(LirInstr::StoreCapture {
                        index: temp_slot,
                        src: value_reg,
                    });
                } else {
                    self.emit(LirInstr::StoreLocal {
                        slot: temp_slot,
                        src: value_reg,
                    });
                }

                // Step 2: Check if value is an array
                let is_array_reg = self.fresh_reg();
                self.emit(LirInstr::IsArrayMut {
                    dst: is_array_reg,
                    src: value_reg,
                });

                let type_ok_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: is_array_reg,
                    then_label: type_ok_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(type_ok_label);

                // Step 3: Check array length
                // Reload from temp slot
                let reloaded_for_len = self.fresh_reg();
                if self.in_lambda {
                    self.emit(LirInstr::LoadCapture {
                        dst: reloaded_for_len,
                        index: temp_slot,
                    });
                } else {
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded_for_len,
                        slot: temp_slot,
                    });
                }

                let len_reg = self.fresh_reg();
                self.emit(LirInstr::ArrayMutLen {
                    dst: len_reg,
                    src: reloaded_for_len,
                });

                let expected_len = self.emit_const(LirConst::Int(elements.len() as i64))?;
                let len_ok_reg = self.fresh_reg();

                if rest.is_some() {
                    // With & rest: length must be >= number of fixed elements
                    self.emit(LirInstr::Compare {
                        dst: len_ok_reg,
                        op: CmpOp::Ge,
                        lhs: len_reg,
                        rhs: expected_len,
                    });
                } else {
                    // Without rest: length must be exactly equal
                    self.emit(LirInstr::Compare {
                        dst: len_ok_reg,
                        op: CmpOp::Eq,
                        lhs: len_reg,
                        rhs: expected_len,
                    });
                }

                let len_ok_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: len_ok_reg,
                    then_label: len_ok_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(len_ok_label);

                // Step 4: Match each element using ArrayMutRefOrNil
                for (i, element_pat) in elements.iter().enumerate() {
                    // Reload the array from temp slot for each element
                    let reloaded = self.fresh_reg();
                    if self.in_lambda {
                        self.emit(LirInstr::LoadCapture {
                            dst: reloaded,
                            index: temp_slot,
                        });
                    } else {
                        self.emit(LirInstr::LoadLocal {
                            dst: reloaded,
                            slot: temp_slot,
                        });
                    }

                    let elem_reg = self.fresh_reg();
                    self.emit(LirInstr::ArrayMutRefDestructure {
                        dst: elem_reg,
                        src: reloaded,
                        index: i as u16,
                    });

                    // Recursively match the element
                    self.lower_pattern_match(element_pat, elem_reg, fail_label)?;
                }

                // Step 5: Handle & rest
                if let Some(rest_pat) = rest {
                    let reloaded = self.fresh_reg();
                    if self.in_lambda {
                        self.emit(LirInstr::LoadCapture {
                            dst: reloaded,
                            index: temp_slot,
                        });
                    } else {
                        self.emit(LirInstr::LoadLocal {
                            dst: reloaded,
                            slot: temp_slot,
                        });
                    }

                    let slice_reg = self.fresh_reg();
                    self.emit(LirInstr::ArrayMutSliceFrom {
                        dst: slice_reg,
                        src: reloaded,
                        index: elements.len() as u16,
                    });

                    self.lower_pattern_match(rest_pat, slice_reg, fail_label)?;
                }

                Ok(())
            }
            HirPattern::Struct { entries, rest } => {
                // Struct {...} pattern matching for `match`.
                // Check if value is a struct, then use StructGetOrNil for each key.
                let temp_slot = if self.in_lambda {
                    self.num_captures + self.current_func.num_locals
                } else {
                    self.current_func.num_locals
                };
                self.current_func.num_locals += 1;

                if self.in_lambda {
                    self.emit(LirInstr::StoreCapture {
                        index: temp_slot,
                        src: value_reg,
                    });
                } else {
                    self.emit(LirInstr::StoreLocal {
                        slot: temp_slot,
                        src: value_reg,
                    });
                }

                // Type guard: reject non-struct values
                let is_struct_reg = self.fresh_reg();
                self.emit(LirInstr::IsStruct {
                    dst: is_struct_reg,
                    src: value_reg,
                });

                let continue_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: is_struct_reg,
                    then_label: continue_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(continue_label);

                for (key, sub_pattern) in entries.iter() {
                    let reloaded = self.fresh_reg();
                    if self.in_lambda {
                        self.emit(LirInstr::LoadCapture {
                            dst: reloaded,
                            index: temp_slot,
                        });
                    } else {
                        self.emit(LirInstr::LoadLocal {
                            dst: reloaded,
                            slot: temp_slot,
                        });
                    }

                    let elem_reg = self.fresh_reg();
                    let lir_key = match key {
                        PatternKey::Keyword(k) => LirConst::Keyword(k.clone()),
                        PatternKey::Symbol(sid) => LirConst::Symbol(*sid),
                    };
                    self.emit(LirInstr::StructGetOrNil {
                        dst: elem_reg,
                        src: reloaded,
                        key: lir_key,
                    });

                    self.lower_pattern_match(sub_pattern, elem_reg, fail_label)?;
                }

                if let Some(rest_pat) = rest {
                    let reloaded = self.fresh_reg();
                    if self.in_lambda {
                        self.emit(LirInstr::LoadCapture {
                            dst: reloaded,
                            index: temp_slot,
                        });
                    } else {
                        self.emit(LirInstr::LoadLocal {
                            dst: reloaded,
                            slot: temp_slot,
                        });
                    }
                    let rest_reg = self.fresh_reg();
                    let exclude: Vec<LirConst> = entries
                        .iter()
                        .map(|(key, _)| match key {
                            PatternKey::Keyword(k) => LirConst::Keyword(k.clone()),
                            PatternKey::Symbol(sid) => LirConst::Symbol(*sid),
                        })
                        .collect();
                    self.emit(LirInstr::StructRest {
                        dst: rest_reg,
                        src: reloaded,
                        exclude_keys: exclude,
                    });
                    self.lower_pattern_match(rest_pat, rest_reg, fail_label)?;
                }

                Ok(())
            }
            HirPattern::Table { entries, rest } => {
                // @struct @{...} pattern matching for `match`.
                // Check if value is a @struct, then use StructGetOrNil for each key.
                let temp_slot = if self.in_lambda {
                    self.num_captures + self.current_func.num_locals
                } else {
                    self.current_func.num_locals
                };
                self.current_func.num_locals += 1;

                if self.in_lambda {
                    self.emit(LirInstr::StoreCapture {
                        index: temp_slot,
                        src: value_reg,
                    });
                } else {
                    self.emit(LirInstr::StoreLocal {
                        slot: temp_slot,
                        src: value_reg,
                    });
                }

                // Type guard: reject non-@struct values
                let is_table_reg = self.fresh_reg();
                self.emit(LirInstr::IsStructMut {
                    dst: is_table_reg,
                    src: value_reg,
                });

                let continue_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: is_table_reg,
                    then_label: continue_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(continue_label);

                for (key, sub_pattern) in entries.iter() {
                    let reloaded = self.fresh_reg();
                    if self.in_lambda {
                        self.emit(LirInstr::LoadCapture {
                            dst: reloaded,
                            index: temp_slot,
                        });
                    } else {
                        self.emit(LirInstr::LoadLocal {
                            dst: reloaded,
                            slot: temp_slot,
                        });
                    }

                    let elem_reg = self.fresh_reg();
                    let lir_key = match key {
                        PatternKey::Keyword(k) => LirConst::Keyword(k.clone()),
                        PatternKey::Symbol(sid) => LirConst::Symbol(*sid),
                    };
                    self.emit(LirInstr::StructGetOrNil {
                        dst: elem_reg,
                        src: reloaded,
                        key: lir_key,
                    });

                    self.lower_pattern_match(sub_pattern, elem_reg, fail_label)?;
                }

                if let Some(rest_pat) = rest {
                    let reloaded = self.fresh_reg();
                    if self.in_lambda {
                        self.emit(LirInstr::LoadCapture {
                            dst: reloaded,
                            index: temp_slot,
                        });
                    } else {
                        self.emit(LirInstr::LoadLocal {
                            dst: reloaded,
                            slot: temp_slot,
                        });
                    }
                    let rest_reg = self.fresh_reg();
                    let exclude: Vec<LirConst> = entries
                        .iter()
                        .map(|(key, _)| match key {
                            PatternKey::Keyword(k) => LirConst::Keyword(k.clone()),
                            PatternKey::Symbol(sid) => LirConst::Symbol(*sid),
                        })
                        .collect();
                    self.emit(LirInstr::StructRest {
                        dst: rest_reg,
                        src: reloaded,
                        exclude_keys: exclude,
                    });
                    self.lower_pattern_match(rest_pat, rest_reg, fail_label)?;
                }

                Ok(())
            }
            HirPattern::Or(alternatives) => {
                // Or-pattern: try each alternative sequentially.
                // Store value to temp slot so we can reload for each alternative.
                let temp_slot = if self.in_lambda {
                    self.num_captures + self.current_func.num_locals
                } else {
                    self.current_func.num_locals
                };
                self.current_func.num_locals += 1;

                if self.in_lambda {
                    self.emit(LirInstr::StoreCapture {
                        index: temp_slot,
                        src: value_reg,
                    });
                } else {
                    self.emit(LirInstr::StoreLocal {
                        slot: temp_slot,
                        src: value_reg,
                    });
                }

                let success_label = self.fresh_label();

                for (i, alt) in alternatives.iter().enumerate() {
                    let next_alt_label = if i + 1 < alternatives.len() {
                        self.fresh_label()
                    } else {
                        fail_label
                    };

                    // Reload value for this alternative
                    let reloaded = self.fresh_reg();
                    if self.in_lambda {
                        self.emit(LirInstr::LoadCapture {
                            dst: reloaded,
                            index: temp_slot,
                        });
                    } else {
                        self.emit(LirInstr::LoadLocal {
                            dst: reloaded,
                            slot: temp_slot,
                        });
                    }

                    self.lower_pattern_match(alt, reloaded, next_alt_label)?;

                    // This alternative matched — jump to success
                    self.terminate(Terminator::Jump(success_label));
                    self.finish_block();

                    if i + 1 < alternatives.len() {
                        self.current_block = BasicBlock::new(next_alt_label);
                    }
                }

                self.current_block = BasicBlock::new(success_label);
                Ok(())
            }
            HirPattern::Set { binding } => {
                // Type guard: check if value is a set
                let is_set_reg = self.fresh_reg();
                self.emit(LirInstr::IsSet {
                    dst: is_set_reg,
                    src: value_reg,
                });

                let type_ok_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: is_set_reg,
                    then_label: type_ok_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(type_ok_label);

                // Recursively match the binding (if any)
                self.lower_pattern_match(binding, value_reg, fail_label)?;
                Ok(())
            }
            HirPattern::SetMut { binding } => {
                // Type guard: check if value is a mutable set
                let is_set_mut_reg = self.fresh_reg();
                self.emit(LirInstr::IsSetMut {
                    dst: is_set_mut_reg,
                    src: value_reg,
                });

                let type_ok_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: is_set_mut_reg,
                    then_label: type_ok_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(type_ok_label);

                // Recursively match the binding (if any)
                self.lower_pattern_match(binding, value_reg, fail_label)?;
                Ok(())
            }
            HirPattern::NamedStruct { .. } => {
                // NamedStruct only appears in &named parameter destructuring, never in match.
                unreachable!("NamedStruct in lower_pattern_match")
            }
        }
    }
}
