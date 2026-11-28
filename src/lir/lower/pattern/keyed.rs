// audited: 2026-11-24
//! Keyed and or-pattern lowering: `Struct`, `Table`, `Or`.
//!
//! docs/match.md

use super::*;
use crate::hir::PatternKey;
use crate::lir::lower::access::{excluded_keys, pattern_key_const};

impl<'a> Lowerer<'a> {
    pub(in crate::lir::lower) fn lower_keyed_pattern(
        &mut self,
        pattern: &HirPattern,
        value_reg: Reg,
        fail_label: Label,
    ) -> Result<(), String> {
        match pattern {
            HirPattern::Struct { entries, rest } | HirPattern::Table { entries, rest } => {
                let mutable = matches!(pattern, HirPattern::Table { .. });
                let rest = rest.as_deref().filter(|_| pattern.own_rest_builds());
                self.lower_struct_pattern(mutable, entries, rest, value_reg, fail_label)
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

    /// A struct pattern (`mutable` for an `@struct` one) against the view in
    /// `value_reg`: the value must be a struct of that kind and hold every key
    /// the pattern names, a key that holds nil included. Then each entry's
    /// pattern matches the key's value, and `rest`, when the pattern builds
    /// one, matches the keys it does not name.
    fn lower_struct_pattern(
        &mut self,
        mutable: bool,
        entries: &[(PatternKey, HirPattern)],
        rest: Option<&HirPattern>,
        value_reg: Reg,
        fail_label: Label,
    ) -> Result<(), String> {
        // Each read consumes its register, so the value waits in a stack slot
        // and every test reloads it.
        let temp_slot = self.fresh_local();
        self.emit(InstrRef::StoreLocal {
            slot: temp_slot,
            src: value_reg,
        });

        let reloaded = self.reload(temp_slot);
        let is_kind = self.fresh_reg();
        self.emit(if mutable {
            InstrRef::IsStructMut {
                dst: is_kind,
                src: reloaded,
            }
        } else {
            InstrRef::IsStruct {
                dst: is_kind,
                src: reloaded,
            }
        });
        self.continue_if(is_kind, fail_label);

        for (key, sub_pattern) in entries {
            let reloaded = self.reload(temp_slot);
            let held = self.fresh_reg();
            self.emit(InstrRef::StructHasKey {
                dst: held,
                src: reloaded,
                key: pattern_key_const(key),
            });
            self.continue_if(held, fail_label);

            let reloaded = self.reload(temp_slot);
            let elem_reg = self.fresh_reg();
            self.emit(InstrRef::StructGetOrNil {
                dst: elem_reg,
                src: reloaded,
                key: pattern_key_const(key),
            });
            self.lower_pattern_match(sub_pattern, elem_reg, fail_label)?;
        }

        if let Some(rest_pat) = rest {
            let reloaded = self.reload(temp_slot);
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
}
