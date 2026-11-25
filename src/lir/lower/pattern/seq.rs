// audited: 2026-11-25
//! Lowering the sequence patterns: pair, list, and the two arrays.
//!
//! src/lir/lower/overview.md
//! docs/destructuring.md
//!
//! Each one parks the scrutinee in a temp slot and reloads it. A nested pattern
//! may end the block, so a register read across that boundary is a register the
//! emitter has already consumed; only the slot survives.

use super::*;
use crate::value::Mutability;

impl<'a> Lowerer<'a> {
    pub(in crate::lir::lower) fn lower_seq_pattern(
        &mut self,
        pattern: &HirPattern,
        value_reg: Reg,
        fail_label: Label,
    ) -> Result<(), String> {
        match pattern {
            HirPattern::Pair { head, tail } => {
                // A temp slot is a stack local, never a capture cell.
                let temp_slot = self.fresh_local();
                self.emit(InstrRef::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });
                let reloaded = self.reload(temp_slot);
                let is_pair_reg = self.fresh_reg();
                self.emit(InstrRef::IsPair {
                    dst: is_pair_reg,
                    src: reloaded,
                });
                self.continue_if(is_pair_reg, fail_label);

                // The head's match may end the block, so the tail is read
                // after it, from the slot, in whatever block it left open.
                let reloaded = self.reload(temp_slot);
                let head_reg = self.fresh_reg();
                self.emit(InstrRef::First {
                    dst: head_reg,
                    pair: reloaded,
                });
                self.lower_pattern_match(head, head_reg, fail_label)?;

                let reloaded = self.reload(temp_slot);
                let tail_reg = self.fresh_reg();
                self.emit(InstrRef::Rest {
                    dst: tail_reg,
                    pair: reloaded,
                });
                // The tail is a borrowed subview of the scrutinee — mark
                // its bindings (see `destructure_alias_bindings`).
                for b in tail.bindings().bindings {
                    self.destructure_alias_bindings.insert(b);
                }
                self.lower_pattern_match(tail, tail_reg, fail_label)
            }
            HirPattern::List { elements, rest } => {
                let mut current_reg = value_reg;

                for pat in elements.iter() {
                    let temp_slot = self.fresh_local();
                    self.emit(InstrRef::StoreLocal {
                        slot: temp_slot,
                        src: current_reg,
                    });
                    let reloaded = self.reload(temp_slot);
                    let is_pair_reg = self.fresh_reg();
                    self.emit(InstrRef::IsPair {
                        dst: is_pair_reg,
                        src: reloaded,
                    });
                    self.continue_if(is_pair_reg, fail_label);

                    let reloaded = self.reload(temp_slot);
                    let head_reg = self.fresh_reg();
                    self.emit(InstrRef::First {
                        dst: head_reg,
                        pair: reloaded,
                    });
                    self.lower_pattern_match(pat, head_reg, fail_label)?;

                    // The tail is needed for the next element, the rest, or
                    // the check that the list ends here.
                    let reloaded = self.reload(temp_slot);
                    let tail_reg = self.fresh_reg();
                    self.emit(InstrRef::Rest {
                        dst: tail_reg,
                        pair: reloaded,
                    });
                    current_reg = tail_reg;
                }

                if let Some(rest_pat) = rest {
                    // The tail is a borrowed subview — mark its bindings
                    // (see `destructure_alias_bindings`).
                    for b in rest_pat.bindings().bindings {
                        self.destructure_alias_bindings.insert(b);
                    }
                    self.lower_pattern_match(rest_pat, current_reg, fail_label)?;
                } else {
                    // With no rest, the list must end here.
                    let empty_list_reg = self.fresh_reg();
                    self.emit(InstrRef::ValueConst {
                        dst: empty_list_reg,
                        value: Value::EMPTY_LIST,
                    });
                    let is_empty_reg = self.fresh_reg();
                    self.emit(InstrRef::compare(
                        is_empty_reg,
                        CmpOp::Eq,
                        current_reg,
                        empty_list_reg,
                    ));
                    self.continue_if(is_empty_reg, fail_label);
                }

                Ok(())
            }
            HirPattern::Tuple { elements, rest } | HirPattern::Array { elements, rest } => {
                // `[...]` matches an array and `@[...]` an @array; the rest
                // slices the same kind.
                let kind = pattern.array_mutability().expect("an array pattern");
                let has_rest = rest.is_some();
                let rest = rest.as_deref().filter(|_| pattern.own_rest_builds());
                self.lower_array_pattern(kind, elements, has_rest, rest, value_reg, fail_label)
            }
            _ => unreachable!("lower_seq_pattern: unexpected pattern"),
        }
    }

    /// An array pattern of the `kind` of array it tests for, against the view
    /// in `value_reg`: the value must be an array of that kind, as long as the
    /// pattern's elements or, with a rest (`has_rest`), longer. Then each
    /// element's pattern matches its element, and `rest`, when the pattern
    /// builds one, matches the slice of that kind past them.
    fn lower_array_pattern(
        &mut self,
        kind: Mutability,
        elements: &[HirPattern],
        has_rest: bool,
        rest: Option<&HirPattern>,
        value_reg: Reg,
        fail_label: Label,
    ) -> Result<(), String> {
        // A temp slot is a stack local, never a capture cell.
        let temp_slot = self.fresh_local();
        self.emit(InstrRef::StoreLocal {
            slot: temp_slot,
            src: value_reg,
        });

        let reloaded = self.reload(temp_slot);
        let is_kind = self.fresh_reg();
        self.emit(InstrRef::is_array(kind, is_kind, reloaded));
        self.continue_if(is_kind, fail_label);

        let reloaded = self.reload(temp_slot);
        let len_reg = self.fresh_reg();
        self.emit(InstrRef::ArrayMutLen {
            dst: len_reg,
            src: reloaded,
        });
        let expected_len = self.emit_const(ConstRef::Int(elements.len() as i64))?;
        let len_ok_reg = self.fresh_reg();
        let len_cmp = if has_rest { CmpOp::Ge } else { CmpOp::Eq };
        self.emit(InstrRef::compare(
            len_ok_reg,
            len_cmp,
            len_reg,
            expected_len,
        ));
        self.continue_if(len_ok_reg, fail_label);

        for (i, element_pat) in elements.iter().enumerate() {
            let reloaded = self.reload(temp_slot);
            let elem_reg = self.fresh_reg();
            self.emit(InstrRef::ArrayMutRefDestructure {
                dst: elem_reg,
                src: reloaded,
                index: i as u16,
            });
            self.lower_pattern_match(element_pat, elem_reg, fail_label)?;
        }

        if let Some(rest_pat) = rest {
            let reloaded = self.reload(temp_slot);
            let slice_reg = self.fresh_reg();
            self.emit(InstrRef::array_slice(
                kind,
                slice_reg,
                reloaded,
                elements.len() as u16,
            ));
            let slice_reg = self.park_rest_collection(rest_pat, slice_reg);
            self.lower_pattern_match(rest_pat, slice_reg, fail_label)?;
        }

        Ok(())
    }
}
