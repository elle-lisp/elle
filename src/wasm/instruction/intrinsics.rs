// audited: 2026-09-06
// src/wasm/AGENTS.md
//! Emitting the intrinsic opcodes as WASM: type predicates, data access, and
//! the collection operations that cross to the host.
//!
//! The chain tail of `emit_instr`. A predicate becomes a tag comparison inline;
//! everything else becomes an `rt_data_op` call.

use super::*;

impl WasmEmitter {
    pub(in crate::wasm) fn emit_instr_intrinsics(&mut self, f: &mut Function, instr: &LirInstr) {
        match instr {
            // New type predicates — use tag checks or data ops
            LirInstr::IsEmpty { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_EMPTY_LIST),
            LirInstr::IsBool { dst, src } => {
                // bool = tag is TRUE or FALSE
                f.instruction(&Instruction::LocalGet(self.tag_local(*src)));
                f.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::LocalGet(self.tag_local(*src)));
                f.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::I32Or);
                self.emit_bool_from_i32(f, *dst);
            }
            LirInstr::IsInt { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_INT),
            LirInstr::IsFloat { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_FLOAT),
            LirInstr::IsString { dst, src } => {
                // string = TAG_STRING or TAG_STRING_MUT
                f.instruction(&Instruction::LocalGet(self.tag_local(*src)));
                f.instruction(&Instruction::I64Const(TAG_STRING as i64));
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::LocalGet(self.tag_local(*src)));
                f.instruction(&Instruction::I64Const(TAG_STRING_MUT as i64));
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::I32Or);
                self.emit_bool_from_i32(f, *dst);
            }
            LirInstr::IsKeyword { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_KEYWORD),
            LirInstr::IsSymbolCheck { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_SYMBOL),
            LirInstr::IsBytes { dst, src } => {
                f.instruction(&Instruction::LocalGet(self.tag_local(*src)));
                f.instruction(&Instruction::I64Const(TAG_BYTES as i64));
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::LocalGet(self.tag_local(*src)));
                f.instruction(&Instruction::I64Const(TAG_BYTES_MUT as i64));
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::I32Or);
                self.emit_bool_from_i32(f, *dst);
            }
            LirInstr::IsBox { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_LBOX),
            LirInstr::IsClosure { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_CLOSURE),
            LirInstr::IsFiber { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_FIBER),

            // Data ops via runtime
            LirInstr::TypeOf { dst, src } => {
                self.emit_data_op1(f, *dst, OP_TYPE_OF, *src);
            }
            LirInstr::Length { dst, src } => {
                self.emit_data_op1(f, *dst, OP_LENGTH, *src);
            }
            LirInstr::Get { dst, obj, key } => {
                self.emit_data_op2(f, *dst, OP_INTR_GET, *obj, *key);
            }
            LirInstr::Put { dst, obj, key, val } => {
                self.write_val_to_mem(f, *obj, 0);
                self.write_val_to_mem(f, *key, 1);
                self.write_val_to_mem(f, *val, 2);
                f.instruction(&wasm_encoder::Instruction::I32Const(OP_INTR_PUT));
                f.instruction(&wasm_encoder::Instruction::I32Const(ARGS_BASE));
                f.instruction(&wasm_encoder::Instruction::I32Const(3));
                f.instruction(&wasm_encoder::Instruction::Call(FN_RT_DATA_OP));
                self.store_result_with_signal(f, *dst);
            }
            LirInstr::Del { dst, obj, key } => {
                self.emit_data_op2(f, *dst, OP_INTR_DEL, *obj, *key);
            }
            LirInstr::Has { dst, obj, key } => {
                self.emit_data_op2(f, *dst, OP_INTR_HAS, *obj, *key);
            }
            LirInstr::IntrPush { dst, array, value } => {
                self.emit_data_op2(f, *dst, OP_INTR_PUSH, *array, *value);
            }
            LirInstr::IntrStringPush { dst, string, value } => {
                self.emit_data_op2(f, *dst, OP_INTR_STRING_PUSH, *string, *value);
            }
            LirInstr::IntrBytesPush { dst, bytes, value } => {
                self.emit_data_op2(f, *dst, OP_INTR_BYTES_PUSH, *bytes, *value);
            }
            LirInstr::Pop { dst, src } => {
                self.emit_data_op1(f, *dst, OP_INTR_POP, *src);
            }
            LirInstr::Freeze { dst, src, .. } => {
                self.emit_data_op1(f, *dst, OP_INTR_FREEZE, *src);
            }
            LirInstr::Thaw { dst, src, .. } => {
                self.emit_data_op1(f, *dst, OP_INTR_THAW, *src);
            }
            LirInstr::Identical { dst, lhs, rhs } => {
                self.emit_data_op2(f, *dst, OP_INTR_IDENTICAL, *lhs, *rhs);
            }
            LirInstr::StructRest {
                dst,
                src,
                exclude_keys,
            } => {
                self.write_val_to_mem(f, *src, 0);
                for (i, key) in exclude_keys.iter().enumerate() {
                    match key {
                        LirConst::Keyword(hash) => {
                            self.emit_const_pool_load(f, *dst, Value::keyword_from_hash(*hash));
                        }
                        LirConst::Symbol(id) => {
                            self.emit_const_pool_load(f, *dst, Value::symbol(*id));
                        }
                        _ => {
                            f.instruction(&Instruction::I64Const(TAG_NIL as i64));
                            f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                            f.instruction(&Instruction::I64Const(0));
                            f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                        }
                    }
                    self.write_val_to_mem(f, *dst, i + 1);
                }
                f.instruction(&Instruction::I32Const(OP_STRUCT_REST));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(1 + exclude_keys.len() as i32));
                f.instruction(&Instruction::Call(FN_RT_DATA_OP));
                self.store_result_with_signal(f, *dst);
            }
            _ => unreachable!("emit_instr_intrinsics: instruction handled earlier"),
        }
    }
}
