// audited: 2026-09-06
// src/wasm/AGENTS.md
//! Emitting one LIR instruction as WASM.
//!
//! Each arm either emits the instructions inline or calls one of the concrete
//! emitters beside it. `intrinsics` takes the chain tail.

use super::*;

impl WasmEmitter {
    /// Emit a single LIR instruction as WASM code.
    pub(in crate::wasm) fn emit_instr(&mut self, f: &mut Function, instr: &LirInstr) {
        match instr {
            LirInstr::Const { dst, value } => {
                self.emit_const(f, *dst, value);
                match value {
                    LirConst::Int(_) => {
                        self.known_int.insert(*dst);
                    }
                    _ => {
                        self.known_int.remove(dst);
                    }
                }
            }
            LirInstr::ValueConst { dst, value } => {
                self.emit_value_const(f, *dst, *value);
                self.known_int.remove(dst);
            }
            LirInstr::MaterializeConst { dst, template, .. } => {
                // The wasm backend has its own linear-memory model and no region
                // RC, so the region slot is irrelevant here. A flat string takes
                // the fast path (`emit_const` handles `LirConst::String`);
                // a compound literal is reconstructed as a host Value and baked as
                // a constant, materialized into a freshly minted region held for
                // the module's lifetime (never freed — region reclamation is a
                // VM/JIT property; wasm literals live for the module's lifetime).
                match template {
                    crate::value::ConstTemplate::String(s) => {
                        self.emit_const(f, *dst, &LirConst::String(s.clone()));
                    }
                    other => {
                        let heap_ptr = self.heap_ptr;
                        let region = unsafe { (*heap_ptr).new_runtime_region() };
                        // Re-intern quoted *symbol* leaves into the driving
                        // instance's table so their ids are valid at runtime.
                        // The full-module path threads `self.symbols`; the
                        // standalone/tiered path leaves it null (no instance
                        // table in scope), where a compound-symbol literal is
                        // held off by `standalone_emittable` and `materialize`
                        // would panic — see the `symbols` field on WasmEmitter.
                        let symbols = if self.symbols.is_null() {
                            None
                        } else {
                            Some(unsafe { &mut *self.symbols })
                        };
                        let value = other.materialize(unsafe { &mut *heap_ptr }, region, symbols);
                        self.emit_value_const(f, *dst, value);
                    }
                }
                self.known_int.remove(dst);
            }
            LirInstr::BinOp {
                dst,
                op,
                lhs,
                rhs,
                proof,
            } => {
                // Two sources say the operands are integers, and either is
                // enough. `known_int` follows integers this emitter produced
                // itself; the proof carries what the front end decided, which
                // reaches a parameter the walk cannot type (docs/impl/lir.md).
                let both_int = proof.is_int()
                    || (self.known_int.contains(lhs) && self.known_int.contains(rhs));
                self.emit_binop(f, *dst, *op, *lhs, *rhs, both_int);
                let is_bitwise = matches!(
                    op,
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
                );
                if both_int || is_bitwise {
                    self.known_int.insert(*dst);
                } else {
                    self.known_int.remove(dst);
                }
            }
            LirInstr::Compare {
                dst,
                op,
                lhs,
                rhs,
                proof: _,
            } => {
                self.emit_compare(f, *dst, *op, *lhs, *rhs);
            }
            LirInstr::UnaryOp {
                dst,
                op,
                src,
                proof: _,
            } => {
                self.emit_unary(f, *dst, *op, *src);
            }
            LirInstr::IsNil { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_NIL),
            LirInstr::IsPair { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_CONS),
            LirInstr::IsArray { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_ARRAY),
            LirInstr::IsArrayMut { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_ARRAY_MUT),
            LirInstr::IsStruct { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_STRUCT),
            LirInstr::IsStructMut { dst, src } => {
                self.emit_tag_check(f, *dst, *src, TAG_STRUCT_MUT)
            }
            LirInstr::IsSet { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_SET),
            LirInstr::IsSetMut { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_SET_MUT),
            LirInstr::LoadLocal { dst, slot } => {
                if self.is_closure {
                    f.instruction(&Instruction::LocalGet(self.local_slot_tag(*slot)));
                    f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                    f.instruction(&Instruction::LocalGet(self.local_slot_pay(*slot)));
                    f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                } else {
                    let src = Reg(*slot as u32);
                    self.copy_reg(f, src, *dst);
                }
            }
            LirInstr::StoreLocal { slot, src } => {
                if self.is_closure {
                    f.instruction(&Instruction::LocalGet(self.tag_local(*src)));
                    f.instruction(&Instruction::LocalSet(self.local_slot_tag(*slot)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*src)));
                    f.instruction(&Instruction::LocalSet(self.local_slot_pay(*slot)));
                } else {
                    let dst = Reg(*slot as u32);
                    self.copy_reg(f, *src, dst);
                }
            }
            LirInstr::LoadCapture { dst, index } => {
                let offset = (*index as u64) * 16;
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset: offset + 8,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                // Auto-unwrap CaptureCell
                f.instruction(&Instruction::LocalGet(self.tag_local(*dst)));
                f.instruction(&Instruction::I64Const(TAG_CAPTURE_CELL as i64));
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::If(BlockType::Empty));
                self.emit_data_op1(f, *dst, OP_LOAD_CAPTURE, *dst);
                f.instruction(&Instruction::End);
            }
            LirInstr::LoadCaptureRaw { dst, index } => {
                let offset = (*index as u64) * 16;
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset: offset + 8,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
            }
            LirInstr::StoreCapture { index, src } => {
                let offset = (*index as u64) * 16;
                // Write cell to args[0]
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::I64Store(MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset: offset + 8,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::I64Store(MemArg {
                    offset: 8,
                    align: 3,
                    memory_index: 0,
                }));
                // Write new value to args[1]
                self.write_val_to_mem(f, *src, 1);
                // Call OP_STORE_CAPTURE
                f.instruction(&Instruction::I32Const(OP_STORE_CAPTURE));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(2));
                f.instruction(&Instruction::Call(FN_RT_DATA_OP));
                f.instruction(&Instruction::Drop);
                f.instruction(&Instruction::Drop);
                f.instruction(&Instruction::Drop);
            }
            LirInstr::Call {
                dst, func, args, ..
            } => {
                self.emit_call(f, *dst, *func, args);
            }
            LirInstr::SuspendingCall {
                dst, func, args, ..
            } => {
                self.emit_call(f, *dst, *func, args);
            }
            LirInstr::TailCall { func, args, .. } => {
                if !self.is_closure {
                    let dst = Reg(0);
                    self.emit_call(f, dst, *func, args);
                    f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
                    f.instruction(&Instruction::I64Const(0));
                    f.instruction(&Instruction::Return);
                } else {
                    for (i, arg) in args.iter().enumerate() {
                        self.write_val_to_mem(f, *arg, i);
                    }
                    f.instruction(&Instruction::LocalGet(self.tag_local(*func)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*func)));
                    f.instruction(&Instruction::I32Const(ARGS_BASE));
                    f.instruction(&Instruction::I32Const(args.len() as i32));
                    f.instruction(&Instruction::LocalGet(self.env_local()));
                    f.instruction(&Instruction::Call(FN_RT_PREPARE_TAIL_CALL));
                    self.emit_tail_call_dispatch(f);
                }
            }
            LirInstr::IncrefRegion { .. } | LirInstr::DecrefRegion { .. } => {}
            // The ownership-forest ops (adopt, activation adopt, co-owned-group
            // free) are region-RC ops realized on the VM/JIT tiers; handled
            // structurally here — a no-op arm, like every other region op in
            // this backend (the arena boundary reclaims).
            LirInstr::AdoptRegion { .. }
            | LirInstr::AdoptCellRegion { .. }
            | LirInstr::AdoptIntoActivation { .. }
            | LirInstr::FreeRegionGroup { .. } => {}
            // Region/refcount support is VM-only in this backend.
            LirInstr::StoreLocalRefcounted { slot, src } => {
                // Treat as a plain StoreLocal — wasm doesn't track refcounts.
                if self.is_closure {
                    f.instruction(&Instruction::LocalGet(self.tag_local(*src)));
                    f.instruction(&Instruction::LocalSet(self.local_slot_tag(*slot)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*src)));
                    f.instruction(&Instruction::LocalSet(self.local_slot_pay(*slot)));
                } else {
                    let dst = Reg(*slot as u32);
                    self.copy_reg(f, *src, dst);
                }
            }
            LirInstr::DecrefValueRegion { .. } => {}
            LirInstr::DecrefCellRegion { .. } => {}
            LirInstr::IncrefValueRegion { .. } => {}
            // The coalescing oracle is a VM-interp-only debug instrument.
            LirInstr::AssertRegionMatches { .. } => {}
            LirInstr::List {
                dst, head, tail, ..
            } => {
                self.emit_data_op2(f, *dst, OP_CONS, *head, *tail);
            }
            LirInstr::First { dst, pair } => {
                self.emit_data_op1(f, *dst, OP_CAR, *pair);
            }
            LirInstr::Rest { dst, pair } => {
                self.emit_data_op1(f, *dst, OP_CDR, *pair);
            }
            LirInstr::MatchFail { dst, src } => {
                self.emit_data_op1(f, *dst, OP_MATCH_FAIL, *src);
            }
            LirInstr::FirstDestructure { dst, src } => {
                self.emit_data_op1(f, *dst, OP_CAR_DESTRUCTURE, *src);
            }
            LirInstr::RestDestructure { dst, src } => {
                self.emit_data_op1(f, *dst, OP_CDR_DESTRUCTURE, *src);
            }
            LirInstr::FirstOrNil { dst, src } => {
                self.emit_data_op1(f, *dst, OP_CAR_OR_NIL, *src);
            }
            LirInstr::RestOrNil { dst, src } => {
                self.emit_data_op1(f, *dst, OP_CDR_OR_NIL, *src);
            }
            LirInstr::MakeArrayMut { dst, elements, .. } => {
                self.emit_data_op_n(f, *dst, OP_MAKE_ARRAY, elements);
            }
            LirInstr::ArrayMutLen { dst, src } => {
                self.emit_data_op1(f, *dst, OP_ARRAY_LEN, *src);
            }
            LirInstr::ArrayMutRefDestructure { dst, src, index } => {
                self.emit_data_op1_imm(f, *dst, OP_ARRAY_REF_DESTRUCTURE, *src, *index as i64);
            }
            LirInstr::ArrayMutSliceFrom { dst, src, index } => {
                self.emit_data_op1_imm(f, *dst, OP_ARRAY_SLICE_FROM, *src, *index as i64);
            }
            LirInstr::ArrayMutRefOrNil { dst, src, index } => {
                self.emit_data_op1_imm(f, *dst, OP_ARRAY_REF_OR_NIL, *src, *index as i64);
            }
            LirInstr::StructGetOrNil { dst, src, key } => {
                self.emit_struct_get(f, *dst, OP_STRUCT_GET_OR_NIL, *src, key);
            }
            LirInstr::StructGetDestructure { dst, src, key } => {
                self.emit_struct_get(f, *dst, OP_STRUCT_GET_DESTRUCTURE, *src, key);
            }
            LirInstr::ArrayMutExtend { dst, array, source } => {
                self.emit_data_op2(f, *dst, OP_ARRAY_EXTEND, *array, *source);
            }
            LirInstr::ArrayMutPush { dst, array, value } => {
                self.emit_data_op2(f, *dst, OP_ARRAY_PUSH, *array, *value);
            }
            LirInstr::MakeCaptureCell { dst, value, .. } => {
                self.emit_data_op1(f, *dst, OP_MAKE_CAPTURE, *value);
            }
            LirInstr::LoadCaptureCell { dst, cell } => {
                self.emit_data_op1(f, *dst, OP_LOAD_CAPTURE, *cell);
            }
            LirInstr::StoreCaptureCell { cell, value } => {
                self.emit_data_op2(f, *cell, OP_STORE_CAPTURE, *cell, *value);
            }
            LirInstr::CallArrayMut {
                dst, func, args, ..
            } => {
                self.emit_call_array(f, *dst, *func, *args);
            }
            LirInstr::TailCallArrayMut { func, args, .. } => {
                if !self.is_closure {
                    let dst = Reg(0);
                    self.emit_call_array(f, dst, *func, *args);
                    f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
                    f.instruction(&Instruction::I64Const(0));
                    f.instruction(&Instruction::Return);
                } else {
                    self.write_val_to_mem(f, *args, 1);
                    f.instruction(&Instruction::LocalGet(self.tag_local(*func)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*func)));
                    f.instruction(&Instruction::I32Const(ARGS_BASE));
                    f.instruction(&Instruction::I32Const(-1));
                    f.instruction(&Instruction::LocalGet(self.env_local()));
                    f.instruction(&Instruction::Call(FN_RT_PREPARE_TAIL_CALL));
                    self.emit_tail_call_dispatch(f);
                }
            }
            LirInstr::Eval { dst, expr, env } => {
                let _ = (dst, expr, env);
                f.instruction(&Instruction::Unreachable);
            }
            LirInstr::LoadResumeValue { dst } => {
                if self.may_suspend {
                    f.instruction(&Instruction::LocalGet(self.resume_tag_local));
                    f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                    f.instruction(&Instruction::LocalGet(self.resume_pay_local));
                    f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                } else {
                    f.instruction(&Instruction::Unreachable);
                }
            }
            LirInstr::LoadSelf { dst } => {
                // The executing closure lives in the reserved `SELF_SLOT` in linear
                // memory (tag at `SELF_SLOT`, payload at `SELF_SLOT + 8`), written by
                // the host at every closure entry and carried across suspend/resume
                // (src/wasm/store/call.rs, src/wasm/linker/create.rs). Read it into
                // `dst` — the value path materializes the closure; a call-position
                // self-reference calls it (re-entering the same code+env).
                f.instruction(&Instruction::I32Const(SELF_SLOT));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                f.instruction(&Instruction::I32Const(SELF_SLOT));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset: 8,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
            }
            LirInstr::MakeClosure {
                dst,
                closure_id,
                captures,
                ..
            } => {
                self.emit_make_closure(f, *dst, *closure_id, captures);
            }
            LirInstr::PushParamFrame { pairs } => {
                for (i, (param_reg, val_reg)) in pairs.iter().enumerate() {
                    self.write_val_to_mem_offset(f, *param_reg, ARGS_BASE + (i as i32) * 32);
                    self.write_val_to_mem_offset(f, *val_reg, ARGS_BASE + (i as i32) * 32 + 16);
                }
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(pairs.len() as i32));
                f.instruction(&Instruction::Call(FN_RT_PUSH_PARAM));
            }
            LirInstr::PopParamFrame => {
                f.instruction(&Instruction::Call(FN_RT_POP_PARAM));
            }
            LirInstr::Convert { dst, op, src } => {
                let op_code = match op {
                    crate::lir::ConvOp::IntToFloat => OP_INT_TO_FLOAT,
                    crate::lir::ConvOp::FloatToInt => OP_FLOAT_TO_INT,
                };
                self.emit_data_op1(f, *dst, op_code, *src);
            }
            LirInstr::CheckSignalBound { .. } => {}
            _ => self.emit_instr_intrinsics(f, instr),
        }
    }
}
