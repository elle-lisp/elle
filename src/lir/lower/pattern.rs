//! Pattern matching lowering

use super::*;
use crate::hir::{HirPattern, PatternLiteral};

impl Lowerer {
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
                // Bind the value to the variable
                let slot = self.allocate_slot(*binding);
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

                // Check if value is a pair
                let is_pair_reg = self.fresh_reg();
                self.emit(LirInstr::IsPair {
                    dst: is_pair_reg,
                    src: value_reg,
                });

                let continue_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: is_pair_reg,
                    then_label: continue_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(continue_label);

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

                // Reload for cdr
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

                // Recursively match head and tail
                self.lower_pattern_match(head, head_reg, fail_label)?;
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

                    // Check if current is a pair
                    let is_pair_reg = self.fresh_reg();
                    self.emit(LirInstr::IsPair {
                        dst: is_pair_reg,
                        src: current_reg,
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
            HirPattern::Array { elements, rest } => {
                // TODO: Implement array pattern matching
                // For now, arrays in match patterns are not supported
                if !elements.is_empty() || rest.is_some() {
                    Err("Array pattern matching not yet implemented".to_string())
                } else {
                    Ok(())
                }
            }
        }
    }
}
