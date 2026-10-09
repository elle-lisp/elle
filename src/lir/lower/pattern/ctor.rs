// audited: 2026-10-06
//! The constructor tests a decision-tree switch branches on, and the literal comparison patterns share.
//!
//! src/lir/lower/AGENTS.md
//! docs/match.md
//!
//! Each test answers one question about the scrutinee and leaves the answer in a
//! register. Most are a single instruction; the array constructors need a type
//! check and a length check, so they span blocks and merge through a slot.

use super::*;

impl<'a> Lowerer<'a> {
    /// Emit a constructor test, returning a register holding the boolean result.
    pub(super) fn emit_constructor_test(
        &mut self,
        value_reg: Reg,
        ctor: &Constructor,
    ) -> Result<Reg, String> {
        match ctor {
            Constructor::Literal(lit) => self.emit_literal_eq(value_reg, lit),
            Constructor::Pair => {
                let dst = self.fresh_reg();
                self.emit(InstrRef::IsPair {
                    dst,
                    src: value_reg,
                });
                Ok(dst)
            }
            Constructor::Nil => {
                let dst = self.fresh_reg();
                self.emit(InstrRef::IsNil {
                    dst,
                    src: value_reg,
                });
                Ok(dst)
            }
            Constructor::EmptyList => {
                let empty_reg = self.fresh_reg();
                self.emit(InstrRef::ValueConst {
                    dst: empty_reg,
                    value: Value::EMPTY_LIST,
                });
                let dst = self.fresh_reg();
                self.emit(InstrRef::compare(dst, CmpOp::Eq, value_reg, empty_reg));
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
                self.emit(InstrRef::IsStruct {
                    dst,
                    src: value_reg,
                });
                Ok(dst)
            }
            Constructor::Table(_) => {
                let dst = self.fresh_reg();
                self.emit(InstrRef::IsStructMut {
                    dst,
                    src: value_reg,
                });
                Ok(dst)
            }
            Constructor::Set => {
                let dst = self.fresh_reg();
                self.emit(InstrRef::IsSet {
                    dst,
                    src: value_reg,
                });
                Ok(dst)
            }
            Constructor::SetMut => {
                let dst = self.fresh_reg();
                self.emit(InstrRef::IsSetMut {
                    dst,
                    src: value_reg,
                });
                Ok(dst)
            }
        }
    }

    /// Emit a comparison of `value_reg` with a pattern literal, returning a
    /// register holding the boolean result.
    ///
    /// A STRING literal compares by content but is a HEAP value, so — unlike
    /// the immediates — it cannot be a pooled constant. It is materialized
    /// fresh into a transient per-activation region, compared, and that region
    /// freed at once: a heap literal is an ordinary, reclaimable allocation
    /// (docs/impl/region/model.md), never a process-pinned constant. The string
    /// is dead the instant the comparison reads it, so the region's whole life
    /// is these three instructions.
    pub(super) fn emit_literal_eq(
        &mut self,
        value_reg: Reg,
        lit: &PatternLiteral,
    ) -> Result<Reg, String> {
        if let PatternLiteral::String(s) = lit {
            let region = self.fresh_managed_region();
            let str_reg =
                self.emit_materialize_in(region, &crate::value::ConstTemplate::String(s.clone()));
            let dst = self.fresh_reg();
            self.emit(InstrRef::compare(dst, CmpOp::Eq, value_reg, str_reg));
            self.emit(InstrRef::DecrefRegion { region_id: region });
            return Ok(dst);
        }
        let lit_reg = match lit {
            PatternLiteral::Bool(b) => self.emit_const(ConstRef::Bool(*b))?,
            PatternLiteral::Int(n) => self.emit_const(ConstRef::Int(*n))?,
            PatternLiteral::Float(f) => self.emit_const(ConstRef::Float(*f))?,
            PatternLiteral::Keyword(k) => {
                self.emit_const(ConstRef::Keyword(crate::value::keyword::keyword_hash(k)))?
            }
            PatternLiteral::String(_) => unreachable!("string handled above"),
        };
        let dst = self.fresh_reg();
        self.emit(InstrRef::compare(dst, CmpOp::Eq, value_reg, lit_reg));
        Ok(dst)
    }

    /// Emit a type check and a length check for an array constructor.
    ///
    /// Creates three blocks — type check, length check, result merge — and
    /// returns a register holding the boolean result in the merge block.
    fn emit_type_and_length_test(
        &mut self,
        value_reg: Reg,
        n: usize,
        is_tuple: bool,
        len_cmp: CmpOp,
    ) -> Result<Reg, String> {
        // Store value to temp slot so we can reload after block boundaries.
        let val_slot = self.fresh_local();
        self.emit(InstrRef::StoreLocal {
            slot: val_slot,
            src: value_reg,
        });

        // Reload for type check (auto-pop consumed value_reg)
        let reloaded_for_type = self.fresh_reg();
        self.emit(InstrRef::LoadLocal {
            dst: reloaded_for_type,
            slot: val_slot,
        });

        let type_check_reg = self.fresh_reg();
        if is_tuple {
            self.emit(InstrRef::IsArray {
                dst: type_check_reg,
                src: reloaded_for_type,
            });
        } else {
            self.emit(InstrRef::IsArrayMut {
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
        self.open_block(len_check_label);
        let reloaded = self.fresh_reg();
        self.emit(InstrRef::LoadLocal {
            dst: reloaded,
            slot: val_slot,
        });
        let len_reg = self.fresh_reg();
        self.emit(InstrRef::ArrayMutLen {
            dst: len_reg,
            src: reloaded,
        });
        let expected_reg = self.emit_const(ConstRef::Int(n as i64))?;
        let len_ok = self.fresh_reg();
        self.emit(InstrRef::compare(len_ok, len_cmp, len_reg, expected_reg));
        self.terminate(Terminator::Branch {
            cond: len_ok,
            then_label: pass_label,
            else_label: fail_label,
        });
        self.finish_block();

        // Use a local slot to merge the boolean result across blocks
        let merge_slot = self.fresh_local();

        // Fail block: result = false
        self.open_block(fail_label);
        let false_reg = self.emit_const(ConstRef::Bool(false))?;
        let result_label = self.fresh_label();
        self.emit(InstrRef::StoreLocal {
            slot: merge_slot,
            src: false_reg,
        });
        self.terminate(Terminator::Jump(result_label));
        self.finish_block();

        // Pass block: result = true
        self.open_block(pass_label);
        let true_reg = self.emit_const(ConstRef::Bool(true))?;
        self.emit(InstrRef::StoreLocal {
            slot: merge_slot,
            src: true_reg,
        });
        self.terminate(Terminator::Jump(result_label));
        self.finish_block();

        // Result block: load the boolean
        self.open_block(result_label);
        let dst = self.fresh_reg();
        self.emit(InstrRef::LoadLocal {
            dst,
            slot: merge_slot,
        });
        Ok(dst)
    }
}
