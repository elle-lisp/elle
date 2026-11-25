// audited: 2026-11-24
//! The pattern-match entry point: the scalar patterns lower here, and the sequence and keyed families go to `seq` and `keyed`.
//!
//! src/lir/lower/overview.md
//! docs/match.md

use super::*;

impl<'a> Lowerer<'a> {
    /// Lower `pattern` against `value_reg`, branching to `fail_label` on a
    /// mismatch. A match binds the pattern's variables and continues in the
    /// current block.
    ///
    /// A pattern that tests its value tests the value's view, and a name
    /// binds the value itself (docs/macros/syntax.md).
    pub(in crate::lir::lower) fn lower_pattern_match(
        &mut self,
        pattern: &HirPattern,
        value_reg: Reg,
        fail_label: Label,
    ) -> Result<(), String> {
        let value_reg = match pattern {
            HirPattern::Wildcard
            | HirPattern::Var(_)
            | HirPattern::Or(_)
            | HirPattern::Set { .. }
            | HirPattern::SetMut { .. } => value_reg,
            _ => self.emit_syntax_view(value_reg),
        };
        match pattern {
            HirPattern::Wildcard => {
                // Wildcard always matches, do nothing
                Ok(())
            }
            HirPattern::Nil => {
                // Check if value is nil (NOT empty_list)
                // nil and '() are distinct values with distinct semantics
                let is_nil_reg = self.fresh_reg();
                self.emit(InstrRef::IsNil {
                    dst: is_nil_reg,
                    src: value_reg,
                });
                self.continue_if(is_nil_reg, fail_label);
                Ok(())
            }
            HirPattern::Literal(lit) => {
                let eq_reg = self.emit_literal_eq(value_reg, lit)?;
                self.continue_if(eq_reg, fail_label);
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
                    self.emit(InstrRef::StoreCapture {
                        index: slot,
                        src: value_reg,
                    });
                } else {
                    self.emit(InstrRef::StoreLocal {
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
                self.lower_set_pattern(binding, value_reg, fail_label, true)
            }
            HirPattern::SetMut { binding } => {
                self.lower_set_pattern(binding, value_reg, fail_label, false)
            }
            HirPattern::NamedStruct { .. } => {
                // NamedStruct only appears in &named parameter destructuring, never in match.
                unreachable!("NamedStruct in lower_pattern_match")
            }
        }
    }

    /// A set pattern: the value's view must be a set (`immutable`) or a
    /// mutable set, and `binding` matches the whole value, as the decision
    /// tree binds it.
    fn lower_set_pattern(
        &mut self,
        binding: &HirPattern,
        value_reg: Reg,
        fail_label: Label,
        immutable: bool,
    ) -> Result<(), String> {
        // A temp slot is a stack local, never a capture cell. The test
        // consumes one reload and the binding another.
        let temp_slot = self.fresh_local();
        self.emit(InstrRef::StoreLocal {
            slot: temp_slot,
            src: value_reg,
        });
        let reloaded = self.reload(temp_slot);
        let view = self.emit_syntax_view(reloaded);
        let is_set_reg = self.fresh_reg();
        self.emit(if immutable {
            InstrRef::IsSet {
                dst: is_set_reg,
                src: view,
            }
        } else {
            InstrRef::IsSetMut {
                dst: is_set_reg,
                src: view,
            }
        });

        self.continue_if(is_set_reg, fail_label);
        let reloaded = self.reload(temp_slot);
        self.lower_pattern_match(binding, reloaded, fail_label)
    }
}
