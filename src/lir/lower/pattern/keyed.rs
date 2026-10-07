// audited: 2026-10-06
//! Keyed and or-pattern lowering: `Struct`, `Table`, `Or`.
//!
//! docs/match.md

use super::*;
use crate::lir::lower::access::{excluded_keys, pattern_key_const};

impl<'a> Lowerer<'a> {
    pub(in crate::lir::lower) fn lower_keyed_pattern(
        &mut self,
        pattern: &HirPattern,
        value_reg: Reg,
        fail_label: Label,
    ) -> Result<(), String> {
        match pattern {
            HirPattern::Struct { entries, rest } => {
                // Struct {...} pattern matching for `match`.
                // Check if value is a struct, then use StructGetOrNil for each key.
                // Temp slots are always stack-local (never LBox cells).
                let temp_slot = self.fresh_local();
                self.emit(InstrRef::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                // Type guard: reject non-struct values.
                // Reload from temp slot — value_reg was consumed by StoreLocal.
                let reloaded_for_type = self.fresh_reg();
                self.emit(InstrRef::LoadLocal {
                    dst: reloaded_for_type,
                    slot: temp_slot,
                });
                let is_struct_reg = self.fresh_reg();
                self.emit(InstrRef::IsStruct {
                    dst: is_struct_reg,
                    src: reloaded_for_type,
                });

                let continue_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: is_struct_reg,
                    then_label: continue_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.open_block(continue_label);

                for (key, sub_pattern) in entries.iter() {
                    let reloaded = self.fresh_reg();
                    self.emit(InstrRef::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });

                    let elem_reg = self.fresh_reg();
                    self.emit(InstrRef::StructGetOrNil {
                        dst: elem_reg,
                        src: reloaded,
                        key: pattern_key_const(key),
                    });

                    self.lower_pattern_match(sub_pattern, elem_reg, fail_label)?;
                }

                if let Some(rest_pat) = rest.as_deref().filter(|_| pattern.own_rest_builds()) {
                    let reloaded = self.fresh_reg();
                    self.emit(InstrRef::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let rest_reg = self.fresh_reg();
                    let keys = excluded_keys(entries.iter().map(|(key, _)| key));
                    self.emit(InstrRef::StructRest {
                        dst: rest_reg,
                        src: reloaded,
                        exclude_keys: ConstList::new(&keys),
                    });
                    let rest_reg = self.park_rest_collection(rest_pat, rest_reg);
                    self.lower_pattern_match(rest_pat, rest_reg, fail_label)?;
                }

                Ok(())
            }
            HirPattern::Table { entries, rest } => {
                // @struct @{...} pattern matching for `match`.
                // Check if value is a @struct, then use StructGetOrNil for each key.
                // Temp slots are always stack-local (never LBox cells).
                let temp_slot = self.fresh_local();
                self.emit(InstrRef::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                // Type guard: reject non-@struct values.
                // Reload from temp slot — value_reg was consumed by StoreLocal.
                let reloaded_for_type = self.fresh_reg();
                self.emit(InstrRef::LoadLocal {
                    dst: reloaded_for_type,
                    slot: temp_slot,
                });
                let is_table_reg = self.fresh_reg();
                self.emit(InstrRef::IsStructMut {
                    dst: is_table_reg,
                    src: reloaded_for_type,
                });

                let continue_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: is_table_reg,
                    then_label: continue_label,
                    else_label: fail_label,
                });
                self.finish_block();
                self.open_block(continue_label);

                for (key, sub_pattern) in entries.iter() {
                    let reloaded = self.fresh_reg();
                    self.emit(InstrRef::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });

                    let elem_reg = self.fresh_reg();
                    self.emit(InstrRef::StructGetOrNil {
                        dst: elem_reg,
                        src: reloaded,
                        key: pattern_key_const(key),
                    });

                    self.lower_pattern_match(sub_pattern, elem_reg, fail_label)?;
                }

                if let Some(rest_pat) = rest.as_deref().filter(|_| pattern.own_rest_builds()) {
                    let reloaded = self.fresh_reg();
                    self.emit(InstrRef::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let rest_reg = self.fresh_reg();
                    let keys = excluded_keys(entries.iter().map(|(key, _)| key));
                    self.emit(InstrRef::StructRest {
                        dst: rest_reg,
                        src: reloaded,
                        exclude_keys: ConstList::new(&keys),
                    });
                    let rest_reg = self.park_rest_collection(rest_pat, rest_reg);
                    self.lower_pattern_match(rest_pat, rest_reg, fail_label)?;
                }

                Ok(())
            }
            HirPattern::Or(alternatives) => {
                // Or-pattern: try each alternative sequentially.
                // Store value to temp slot so we can reload for each alternative.
                // Temp slots are always stack-local (never LBox cells).
                let temp_slot = self.fresh_local();
                self.emit(InstrRef::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                let success_label = self.fresh_label();

                for (i, alt) in alternatives.iter().enumerate() {
                    let next_alt_label = if i + 1 < alternatives.len() {
                        self.fresh_label()
                    } else {
                        fail_label
                    };

                    // Reload value for this alternative
                    let reloaded = self.fresh_reg();
                    self.emit(InstrRef::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });

                    self.lower_pattern_match(alt, reloaded, next_alt_label)?;

                    // This alternative matched — jump to success
                    self.terminate(Terminator::Jump(success_label));
                    self.finish_block();

                    if i + 1 < alternatives.len() {
                        self.open_block(next_alt_label);
                    }
                }

                self.open_block(success_label);
                Ok(())
            }
            _ => unreachable!("lower_keyed_pattern: unexpected pattern"),
        }
    }
}
