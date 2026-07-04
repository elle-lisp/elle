use super::*;

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
                self.emit(LirInstr::IsKeyword {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsSymbol => {
                self.emit(LirInstr::IsSymbolCheck {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsPair => {
                self.emit(LirInstr::IsPair {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsArray => {
                // %array? checks both immutable and mutable arrays.
                // Spill the source to a local so both checks can read it
                // (the stack-based emitter consumes the value on first use).
                let src_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: src_slot,
                    src: arg_regs[0],
                });
                let src1 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: src1,
                    slot: src_slot,
                });
                let imm = self.fresh_reg();
                self.emit(LirInstr::IsArray {
                    dst: imm,
                    src: src1,
                });
                let result_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                let then_label = self.fresh_label();
                let else_label = self.fresh_label();
                let merge_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: imm,
                    then_label,
                    else_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(then_label);
                let true_reg = self.emit_const(LirConst::Bool(true))?;
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: true_reg,
                });
                self.terminate(Terminator::Jump(merge_label));
                self.finish_block();
                self.current_block = BasicBlock::new(else_label);
                let src2 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: src2,
                    slot: src_slot,
                });
                let mut_r = self.fresh_reg();
                self.emit(LirInstr::IsArrayMut {
                    dst: mut_r,
                    src: src2,
                });
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: mut_r,
                });
                self.terminate(Terminator::Jump(merge_label));
                self.finish_block();
                self.current_block = BasicBlock::new(merge_label);
                self.emit(LirInstr::LoadLocal {
                    dst,
                    slot: result_slot,
                });
            }
            IntrinsicOp::IsStruct => {
                let src_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: src_slot,
                    src: arg_regs[0],
                });
                let src1 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: src1,
                    slot: src_slot,
                });
                let imm = self.fresh_reg();
                self.emit(LirInstr::IsStruct {
                    dst: imm,
                    src: src1,
                });
                let result_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                let then_label = self.fresh_label();
                let else_label = self.fresh_label();
                let merge_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: imm,
                    then_label,
                    else_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(then_label);
                let true_reg = self.emit_const(LirConst::Bool(true))?;
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: true_reg,
                });
                self.terminate(Terminator::Jump(merge_label));
                self.finish_block();
                self.current_block = BasicBlock::new(else_label);
                let src2 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: src2,
                    slot: src_slot,
                });
                let mut_r = self.fresh_reg();
                self.emit(LirInstr::IsStructMut {
                    dst: mut_r,
                    src: src2,
                });
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: mut_r,
                });
                self.terminate(Terminator::Jump(merge_label));
                self.finish_block();
                self.current_block = BasicBlock::new(merge_label);
                self.emit(LirInstr::LoadLocal {
                    dst,
                    slot: result_slot,
                });
            }
            IntrinsicOp::IsSet => {
                let src_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: src_slot,
                    src: arg_regs[0],
                });
                let src1 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: src1,
                    slot: src_slot,
                });
                let imm = self.fresh_reg();
                self.emit(LirInstr::IsSet {
                    dst: imm,
                    src: src1,
                });
                let result_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                let then_label = self.fresh_label();
                let else_label = self.fresh_label();
                let merge_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: imm,
                    then_label,
                    else_label,
                });
                self.finish_block();
                self.current_block = BasicBlock::new(then_label);
                let true_reg = self.emit_const(LirConst::Bool(true))?;
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: true_reg,
                });
                self.terminate(Terminator::Jump(merge_label));
                self.finish_block();
                self.current_block = BasicBlock::new(else_label);
                let src2 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: src2,
                    slot: src_slot,
                });
                let mut_r = self.fresh_reg();
                self.emit(LirInstr::IsSetMut {
                    dst: mut_r,
                    src: src2,
                });
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: mut_r,
                });
                self.terminate(Terminator::Jump(merge_label));
                self.finish_block();
                self.current_block = BasicBlock::new(merge_label);
                self.emit(LirInstr::LoadLocal {
                    dst,
                    slot: result_slot,
                });
            }
            IntrinsicOp::IsBytes => {
                self.emit(LirInstr::IsBytes {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsBox => {
                self.emit(LirInstr::IsBox {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsClosure => {
                self.emit(LirInstr::IsClosure {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsFiber => {
                self.emit(LirInstr::IsFiber {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::TypeOf => {
                self.emit(LirInstr::TypeOf {
                    dst,
                    src: arg_regs[0],
                });
            }
            // Data access
            IntrinsicOp::Length => {
                self.emit(LirInstr::Length {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::Get => {
                self.emit(LirInstr::Get {
                    dst,
                    obj: arg_regs[0],
                    key: arg_regs[1],
                });
            }
            // Monomorphic put variants reuse the existing Put opcode (intrinsics-mono.md
            // Impl note 2): the runtime dispatches on the actual container type, so the
            // variants differ only in static effect/RetType, not lowering.
            IntrinsicOp::Put
            | IntrinsicOp::PutStruct
            | IntrinsicOp::PutArray
            | IntrinsicOp::PutStructMut
            | IntrinsicOp::PutArrayMut => {
                self.emit(LirInstr::Put {
                    dst,
                    obj: arg_regs[0],
                    key: arg_regs[1],
                    val: arg_regs[2],
                });
            }
            IntrinsicOp::Del => {
                self.emit(LirInstr::Del {
                    dst,
                    obj: arg_regs[0],
                    key: arg_regs[1],
                });
            }
            IntrinsicOp::Has => {
                self.emit(LirInstr::Has {
                    dst,
                    obj: arg_regs[0],
                    key: arg_regs[1],
                });
            }
            // %array-push mutates @array in place, returns new array for immutable.
            // Distinct from ArrayMutPush which is splice infrastructure. The
            // monomorphic %push-array / %push-array-mut reuse the same runtime opcode
            // (intrinsics-mono.md Impl note 2): IntrPush already dispatches on the
            // runtime type, so the variants differ only in their static effect/RetType,
            // not their lowering — no new VM/jit/wasm/mlir opcode needed for the
            // region/type win.
            IntrinsicOp::Push | IntrinsicOp::PushArray | IntrinsicOp::PushArrayMut => {
                self.emit(LirInstr::IntrPush {
                    dst,
                    array: arg_regs[0],
                    value: arg_regs[1],
                });
            }
            IntrinsicOp::StringPush => {
                self.emit(LirInstr::IntrStringPush {
                    dst,
                    string: arg_regs[0],
                    value: arg_regs[1],
                });
            }
            IntrinsicOp::BytesPush => {
                self.emit(LirInstr::IntrBytesPush {
                    dst,
                    bytes: arg_regs[0],
                    value: arg_regs[1],
                });
            }
            IntrinsicOp::Pop => {
                self.emit(LirInstr::Pop {
                    dst,
                    src: arg_regs[0],
                });
            }
            // Mutability
            IntrinsicOp::Freeze => {
                self.emit_alloc(|region| LirInstr::Freeze {
                    region,
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::Thaw => {
                self.emit_alloc(|region| LirInstr::Thaw {
                    region,
                    dst,
                    src: arg_regs[0],
                });
            }
            // Identity
            IntrinsicOp::Identical => {
                self.emit(LirInstr::Identical {
                    dst,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            _ => unreachable!("lower_intrinsic_rest: intrinsic handled in lower_intrinsic"),
        }
        Ok(dst)
    }
}
