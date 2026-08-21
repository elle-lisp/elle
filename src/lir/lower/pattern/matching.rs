use super::*;

impl<'a> Lowerer<'a> {
    /// Lower pattern matching code
    /// Emits code that checks if value_reg matches the pattern
    /// If it doesn't match, branches to fail_label
    /// If it matches, binds any variables and continues in the current block
    pub(in crate::lir::lower) fn lower_pattern_match(
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
                let needs_capture = self.arena.get(*binding).needs_capture();
                if self.in_lambda && needs_capture {
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
            HirPattern::Pair { .. }
            | HirPattern::List { .. }
            | HirPattern::Tuple { .. }
            | HirPattern::Array { .. } => self.lower_seq_pattern(pattern, value_reg, fail_label),
            HirPattern::Struct { .. } | HirPattern::Table { .. } | HirPattern::Or(..) => {
                self.lower_keyed_pattern(pattern, value_reg, fail_label)
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
