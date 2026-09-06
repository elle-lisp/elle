// audited: 2026-09-06
// src/lir/AGENTS.md
//! Emitting the intrinsic opcodes: type predicates, data access, mutability
//! and identity.
//!
//! The chain tail of `emit_instr_ops`. Each arm brings its operands to the top
//! of the simulated stack, emits one opcode, and records the stack effect.

use super::*;

impl Emitter {
    pub(super) fn emit_instr_intrinsics(&mut self, instr: &LirInstr) {
        match instr {
            // === New type predicates ===
            LirInstr::IsEmpty { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsEmptyList);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsBool { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsBool);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsInt { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsInt);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsFloat { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsFloat);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsString { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsString);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsKeyword { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsKeyword);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsSymbolCheck { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsSymbol);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsBytes { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsBytes);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsBox { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsBox);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsClosure { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsClosure);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsFiber { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsFiber);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::TypeOf { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::TypeOf);
                self.pop();
                self.push_reg(*dst);
            }

            // === Data access ===
            LirInstr::Length { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::Length);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::Get { dst, obj, key } => {
                self.ensure_binary_on_top(*obj, *key);
                self.bytecode.emit(Instruction::IntrGet);
                self.pop(); // key
                self.pop(); // obj
                self.push_reg(*dst);
            }
            LirInstr::Put { dst, obj, key, val } => {
                self.ensure_on_top(*obj);
                self.ensure_on_top(*key);
                self.ensure_on_top(*val);
                self.bytecode.emit(Instruction::IntrPut);
                self.pop(); // val
                self.pop(); // key
                self.pop(); // obj
                self.push_reg(*dst);
            }
            LirInstr::Del { dst, obj, key } => {
                self.ensure_binary_on_top(*obj, *key);
                self.bytecode.emit(Instruction::IntrDel);
                self.pop(); // key
                self.pop(); // obj
                self.push_reg(*dst);
            }
            LirInstr::Has { dst, obj, key } => {
                self.ensure_binary_on_top(*obj, *key);
                self.bytecode.emit(Instruction::IntrHas);
                self.pop(); // key
                self.pop(); // obj
                self.push_reg(*dst);
            }
            LirInstr::IntrPush { dst, array, value } => {
                self.ensure_binary_on_top(*array, *value);
                self.bytecode.emit(Instruction::IntrPush);
                self.pop(); // value
                self.pop(); // array
                self.push_reg(*dst);
            }
            LirInstr::IntrStringPush { dst, string, value } => {
                self.ensure_binary_on_top(*string, *value);
                self.bytecode.emit(Instruction::IntrStringPush);
                self.pop(); // value
                self.pop(); // string
                self.push_reg(*dst);
            }
            LirInstr::IntrBytesPush { dst, bytes, value } => {
                self.ensure_binary_on_top(*bytes, *value);
                self.bytecode.emit(Instruction::IntrBytesPush);
                self.pop(); // value
                self.pop(); // bytes
                self.push_reg(*dst);
            }
            LirInstr::Pop { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IntrPop);
                self.pop();
                self.push_reg(*dst);
            }

            // === Mutability ===
            LirInstr::Freeze { dst, src, region } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IntrFreeze);
                self.bytecode.emit_u32(region.get());
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::Thaw { dst, src, region } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IntrThaw);
                self.bytecode.emit_u32(region.get());
                self.pop();
                self.push_reg(*dst);
            }

            // === Identity ===
            LirInstr::Identical { dst, lhs, rhs } => {
                self.ensure_binary_on_top(*lhs, *rhs);
                self.bytecode.emit(Instruction::Identical);
                self.pop(); // rhs
                self.pop(); // lhs
                self.push_reg(*dst);
            }

            LirInstr::CheckSignalBound { src, allowed_bits } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::CheckSignalBound);
                self.bytecode.emit_signal_bits(*allowed_bits);
                // Value consumed by the check
                self.pop();
            }
            _ => unreachable!("emit_instr_intrinsics: instruction handled earlier"),
        }
    }
}
