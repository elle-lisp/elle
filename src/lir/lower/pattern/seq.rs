//! Sequence pattern lowering: Pair / List / Tuple / Array.

use super::*;

impl<'a> Lowerer<'a> {
    pub(in crate::lir::lower) fn lower_seq_pattern(
        &mut self,
        pattern: &HirPattern,
        value_reg: Reg,
        fail_label: Label,
    ) -> Result<(), String> {
        match pattern {
            HirPattern::Pair { head, tail } => {
                // Store value to temp slot before any operations, so we can
                // reload it after the block boundary.
                // Inside a lambda, slots need to account for the captures offset.
                // Temp slots are always stack-local (never LBox cells).
                let temp_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                // Reload for type check (auto-pop consumed value_reg)
                let reloaded_for_check = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: reloaded_for_check,
                    slot: temp_slot,
                });

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
                self.emit(LirInstr::LoadLocal {
                    dst: reloaded_for_car,
                    slot: temp_slot,
                });

                let head_reg = self.fresh_reg();
                self.emit(LirInstr::First {
                    dst: head_reg,
                    pair: reloaded_for_car,
                });

                // Match head pattern first (may create block boundaries)
                self.lower_pattern_match(head, head_reg, fail_label)?;

                // Now reload for cdr — in whatever block the head match left us in
                let reloaded_for_cdr = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: reloaded_for_cdr,
                    slot: temp_slot,
                });

                let tail_reg = self.fresh_reg();
                self.emit(LirInstr::Rest {
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
                    let temp_slot = self.current_func.num_locals;
                    self.current_func.num_locals += 1;
                    self.emit(LirInstr::StoreLocal {
                        slot: temp_slot,
                        src: current_reg,
                    });

                    // Reload for type check (auto-pop consumed current_reg)
                    let reloaded_for_check = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded_for_check,
                        slot: temp_slot,
                    });

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
                    self.emit(LirInstr::LoadLocal {
                        dst: current_for_car,
                        slot: temp_slot,
                    });

                    // Extract head
                    let head_reg = self.fresh_reg();
                    self.emit(LirInstr::First {
                        dst: head_reg,
                        pair: current_for_car,
                    });

                    // Match head against pattern
                    self.lower_pattern_match(pat, head_reg, fail_label)?;

                    // Load for cdr extraction — always needed for next
                    // element, rest binding, or EMPTY_LIST check at end
                    let current_for_cdr = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: current_for_cdr,
                        slot: temp_slot,
                    });

                    // Extract tail for next iteration
                    let tail_reg = self.fresh_reg();
                    self.emit(LirInstr::Rest {
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
                // Temp slots are always stack-local (never LBox cells).
                let temp_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                // Step 2: Check if value is an array.
                // Reload from temp slot — value_reg was consumed by StoreLocal
                // and cannot be reused in stack-based bytecode emission.
                let reloaded_for_type = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: reloaded_for_type,
                    slot: temp_slot,
                });
                let is_tuple_reg = self.fresh_reg();
                self.emit(LirInstr::IsArray {
                    dst: is_tuple_reg,
                    src: reloaded_for_type,
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
                self.emit(LirInstr::LoadLocal {
                    dst: reloaded_for_len,
                    slot: temp_slot,
                });

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
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });

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
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });

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
                // Temp slots are always stack-local (never LBox cells).
                let temp_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                // Step 2: Check if value is a mutable array.
                // Reload from temp slot — value_reg was consumed by StoreLocal.
                let reloaded_for_type = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: reloaded_for_type,
                    slot: temp_slot,
                });
                let is_array_reg = self.fresh_reg();
                self.emit(LirInstr::IsArrayMut {
                    dst: is_array_reg,
                    src: reloaded_for_type,
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
                self.emit(LirInstr::LoadLocal {
                    dst: reloaded_for_len,
                    slot: temp_slot,
                });

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
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });

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
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });

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
            _ => unreachable!("lower_seq_pattern: unexpected pattern"),
        }
    }
}
