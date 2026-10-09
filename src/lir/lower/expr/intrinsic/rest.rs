// audited: 2026-10-06
//! The tail of `%`-intrinsic lowering: the remaining type checks, collection access, freeze and thaw, and identity.
//!
//! src/lir/lower/AGENTS.md
//! docs/intrinsics.md

use super::*;

/// A type check of `src` into `dst`, one of the pair a two-variant predicate
/// reads.
type Check = fn(Reg, Reg) -> InstrRef<'static>;

impl<'a> Lowerer<'a> {
    /// Type-check, collection, freeze/thaw, and misc intrinsics (chain tail
    /// of `lower_intrinsic`; args already lowered into `arg_regs`, result in `dst`).
    pub(super) fn lower_intrinsic_rest(
        &mut self,
        op: crate::hir::IntrinsicOp,
        arg_regs: &[Reg],
        dst: Reg,
    ) -> Result<Reg, String> {
        use crate::hir::IntrinsicOp;
        match op {
            IntrinsicOp::IsKeyword => {
                self.emit(InstrRef::IsKeyword {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsSymbol => {
                self.emit(InstrRef::IsSymbolCheck {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsPair => {
                self.emit(InstrRef::IsPair {
                    dst,
                    src: arg_regs[0],
                });
            }
            // Each of these checks the immutable and the mutable variant.
            IntrinsicOp::IsArray => self.lower_either_check(
                arg_regs[0],
                dst,
                |dst, src| InstrRef::IsArray { dst, src },
                |dst, src| InstrRef::IsArrayMut { dst, src },
            )?,
            IntrinsicOp::IsStruct => self.lower_either_check(
                arg_regs[0],
                dst,
                |dst, src| InstrRef::IsStruct { dst, src },
                |dst, src| InstrRef::IsStructMut { dst, src },
            )?,
            IntrinsicOp::IsSet => self.lower_either_check(
                arg_regs[0],
                dst,
                |dst, src| InstrRef::IsSet { dst, src },
                |dst, src| InstrRef::IsSetMut { dst, src },
            )?,
            IntrinsicOp::IsBytes => {
                self.emit(InstrRef::IsBytes {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsBox => {
                self.emit(InstrRef::IsBox {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsClosure => {
                self.emit(InstrRef::IsClosure {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsFiber => {
                self.emit(InstrRef::IsFiber {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::TypeOf => {
                self.emit(InstrRef::TypeOf {
                    dst,
                    src: arg_regs[0],
                });
            }
            // Data access
            IntrinsicOp::Length => {
                self.emit(InstrRef::Length {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::Get => {
                self.emit(InstrRef::Get {
                    dst,
                    obj: arg_regs[0],
                    key: arg_regs[1],
                });
            }
            // Monomorphic put variants reuse the existing Put opcode: the
            // runtime dispatches on the actual container type, so the variants
            // differ only in static effect/RetType, not lowering.
            IntrinsicOp::Put
            | IntrinsicOp::PutStruct
            | IntrinsicOp::PutArray
            | IntrinsicOp::PutStructMut
            | IntrinsicOp::PutArrayMut => {
                self.emit(InstrRef::Put {
                    dst,
                    obj: arg_regs[0],
                    key: arg_regs[1],
                    val: arg_regs[2],
                });
            }
            IntrinsicOp::Del => {
                self.emit(InstrRef::Del {
                    dst,
                    obj: arg_regs[0],
                    key: arg_regs[1],
                });
            }
            IntrinsicOp::Has => {
                self.emit(InstrRef::Has {
                    dst,
                    obj: arg_regs[0],
                    key: arg_regs[1],
                });
            }
            // %array-push mutates @array in place, returns new array for immutable.
            // Distinct from ArrayMutPush which is splice infrastructure. The
            // monomorphic %push-array / %push-array-mut reuse the same runtime
            // opcode: IntrPush already dispatches on the runtime type, so the
            // variants differ only in their static effect/RetType, not their
            // lowering — no new VM/jit/wasm/mlir opcode needed for the
            // region/type win.
            IntrinsicOp::Push | IntrinsicOp::PushArray | IntrinsicOp::PushArrayMut => {
                self.emit(InstrRef::IntrPush {
                    dst,
                    array: arg_regs[0],
                    value: arg_regs[1],
                });
            }
            IntrinsicOp::StringPush => {
                self.emit(InstrRef::IntrStringPush {
                    dst,
                    string: arg_regs[0],
                    value: arg_regs[1],
                });
            }
            IntrinsicOp::BytesPush => {
                self.emit(InstrRef::IntrBytesPush {
                    dst,
                    bytes: arg_regs[0],
                    value: arg_regs[1],
                });
            }
            IntrinsicOp::Pop => {
                self.emit(InstrRef::Pop {
                    dst,
                    src: arg_regs[0],
                });
            }
            // Mutability
            IntrinsicOp::Freeze => {
                self.emit_alloc(|region| InstrRef::Freeze {
                    region,
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::Thaw => {
                self.emit_alloc(|region| InstrRef::Thaw {
                    region,
                    dst,
                    src: arg_regs[0],
                });
            }
            // Identity
            IntrinsicOp::Identical => {
                self.emit(InstrRef::Identical {
                    dst,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            _ => unreachable!("lower_intrinsic_rest: intrinsic handled in lower_intrinsic"),
        }
        Ok(dst)
    }

    /// `dst` is true when `src` passes `immutable` or, failing that, `mutable`.
    ///
    /// The source is spilled to a local so both checks can read it: the
    /// stack-based emitter consumes a value on its first use. The second check
    /// runs on the branch where the first failed.
    fn lower_either_check(
        &mut self,
        src: Reg,
        dst: Reg,
        immutable: Check,
        mutable: Check,
    ) -> Result<(), String> {
        let src_slot = self.fresh_local();
        self.emit(InstrRef::StoreLocal {
            slot: src_slot,
            src,
        });
        let src1 = self.fresh_reg();
        self.emit(InstrRef::LoadLocal {
            dst: src1,
            slot: src_slot,
        });
        let imm = self.fresh_reg();
        self.emit(immutable(imm, src1));
        let result_slot = self.fresh_local();
        let then_label = self.fresh_label();
        let else_label = self.fresh_label();
        let merge_label = self.fresh_label();
        self.terminate(Terminator::Branch {
            cond: imm,
            then_label,
            else_label,
        });
        self.start_new_block(then_label);
        let true_reg = self.emit_const(ConstRef::Bool(true))?;
        self.emit(InstrRef::StoreLocal {
            slot: result_slot,
            src: true_reg,
        });
        self.terminate(Terminator::Jump(merge_label));
        self.start_new_block(else_label);
        let src2 = self.fresh_reg();
        self.emit(InstrRef::LoadLocal {
            dst: src2,
            slot: src_slot,
        });
        let mut_r = self.fresh_reg();
        self.emit(mutable(mut_r, src2));
        self.emit(InstrRef::StoreLocal {
            slot: result_slot,
            src: mut_r,
        });
        self.terminate(Terminator::Jump(merge_label));
        self.start_new_block(merge_label);
        self.emit(InstrRef::LoadLocal {
            dst,
            slot: result_slot,
        });
        Ok(())
    }
}
