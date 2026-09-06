// audited: 2026-09-06
// src/lir/lower/AGENTS.md
// docs/match.md
//! The constructor tests a decision-tree switch branches on.
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
            Constructor::Literal(lit) => {
                // A STRING literal compares by content but is a HEAP value, so —
                // unlike the immediate literals below — it cannot be a pooled
                // constant. Materialize it FRESH into a transient per-activation
                // region, compare, then free that region immediately: a heap
                // literal is an ordinary, reclaimable allocation
                // (docs/impl/region/model.md), never a process-pinned constant.
                // The string is dead the instant the comparison reads it, so the
                // region's whole life is these three instructions.
                if let PatternLiteral::String(s) = lit {
                    let str_reg = self.fresh_reg();
                    let region = self.fresh_managed_region();
                    self.emit(LirInstr::MaterializeConst {
                        dst: str_reg,
                        template: crate::value::ConstTemplate::String(s.clone()),
                        region,
                    });
                    let dst = self.fresh_reg();
                    self.emit(LirInstr::compare(dst, CmpOp::Eq, value_reg, str_reg));
                    self.emit(LirInstr::DecrefRegion { region_id: region });
                    return Ok(dst);
                }
                let lit_reg = match lit {
                    PatternLiteral::Bool(b) => self.emit_const(LirConst::Bool(*b))?,
                    PatternLiteral::Int(n) => self.emit_const(LirConst::Int(*n))?,
                    PatternLiteral::Float(f) => self.emit_const(LirConst::Float(*f))?,
                    PatternLiteral::Keyword(k) => {
                        self.emit_const(LirConst::Keyword(crate::value::keyword::keyword_hash(k)))?
                    }
                    PatternLiteral::String(_) => unreachable!("string handled above"),
                };
                let dst = self.fresh_reg();
                self.emit(LirInstr::compare(dst, CmpOp::Eq, value_reg, lit_reg));
                Ok(dst)
            }
            Constructor::Pair => {
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
                self.emit(LirInstr::compare(dst, CmpOp::Eq, value_reg, empty_reg));
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
        self.emit(LirInstr::compare(len_ok, len_cmp, len_reg, expected_reg));
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
