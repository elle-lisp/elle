// audited: 2026-09-14
// docs/impl/vm.md
// docs/impl/bytecode.md
//! The stack-only opcode bodies: arithmetic, bitwise, comparisons, type
//! conversions, type checks, and the container intrinsics.
//!
//! Split out of the dispatch match (opcodes.rs) on the one line every opcode
//! here sits above: its whole effect is on the operand stack. None of them
//! reads the bytecode, advances the ip, or asks the code object anything, so
//! none of them can exit the dispatch loop and none needs the harness's
//! arguments. What is left in opcodes.rs is the opcodes that do.
//!
//! The outer match names every instruction this one handles rather than
//! routing a wildcard here, so a new opcode still fails to compile until some
//! arm claims it. The arm below is that list's other half.

use super::*;

impl VM {
    /// Run one stack-only opcode. `instr` is already decoded, and the dispatch
    /// match named it in the arm that calls this — the `unreachable!` is what
    /// a disagreement between the two lists reports.
    ///
    /// `#[inline]` for the same reason `dispatch_instruction` carries it: the
    /// loop is hot, and folding this back into the caller keeps one function
    /// body from opcode decode to handler.
    #[inline]
    pub(super) fn dispatch_scalar(&mut self, instr: Instruction) {
        match instr {
            // Arithmetic (integer)
            Instruction::AddInt => arithmetic::handle_add_int(self),
            Instruction::SubInt => arithmetic::handle_sub_int(self),
            Instruction::MulInt => arithmetic::handle_mul_int(self),
            Instruction::DivInt => arithmetic::handle_div_int(self),

            // Arithmetic (polymorphic)
            Instruction::Add => arithmetic::handle_add(self),
            Instruction::Sub => arithmetic::handle_sub(self),
            Instruction::Mul => arithmetic::handle_mul(self),
            Instruction::Div => arithmetic::handle_div(self),
            Instruction::Rem => arithmetic::handle_rem(self),

            // Bitwise operations
            Instruction::BitAnd => arithmetic::handle_bit_and(self),
            Instruction::BitOr => arithmetic::handle_bit_or(self),
            Instruction::BitXor => arithmetic::handle_bit_xor(self),
            Instruction::BitNot => arithmetic::handle_bit_not(self),
            Instruction::BitNotIntr => types::handle_bit_not_intr(self),
            Instruction::Shl => arithmetic::handle_shl(self),
            Instruction::Shr => arithmetic::handle_shr(self),

            // Type conversions
            Instruction::IntToFloat => arithmetic::handle_int_to_float(self),
            Instruction::FloatToInt => arithmetic::handle_float_to_int(self),

            // Comparisons
            Instruction::Eq => comparison::handle_eq(self),
            Instruction::Ne => types::handle_ne(self),
            Instruction::Lt => comparison::handle_lt(self),
            Instruction::Gt => comparison::handle_gt(self),
            Instruction::Le => comparison::handle_le(self),
            Instruction::Ge => comparison::handle_ge(self),
            Instruction::Identical => types::handle_identical(self),

            // Type checks
            Instruction::IsNil => types::handle_is_nil(self),
            Instruction::IsEmptyList => types::handle_is_empty_list(self),
            Instruction::IsPair => types::handle_is_pair(self),
            Instruction::IsArray => types::handle_is_array(self),
            Instruction::IsArrayMut => types::handle_is_array_mut(self),
            Instruction::IsStruct => types::handle_is_struct(self),
            Instruction::IsStructMut => types::handle_is_struct_mut(self),
            Instruction::IsSet => types::handle_is_set(self),
            Instruction::IsSetMut => types::handle_is_set_mut(self),
            Instruction::IsBool => types::handle_is_bool(self),
            Instruction::IsInt => types::handle_is_int(self),
            Instruction::IsFloat => types::handle_is_float(self),
            Instruction::IsString => types::handle_is_string(self),
            Instruction::IsKeyword => types::handle_is_keyword(self),
            Instruction::IsBytes => types::handle_is_bytes(self),
            Instruction::IsBox => types::handle_is_box(self),
            Instruction::IsClosure => types::handle_is_closure(self),
            Instruction::IsFiber => types::handle_is_fiber(self),
            Instruction::IsNumber => types::handle_is_number(self),
            Instruction::IsSymbol => types::handle_is_symbol(self),
            Instruction::Not => types::handle_not(self),
            Instruction::TypeOf => types::handle_type_of(self),

            // Container intrinsics. `IntrFreeze` and `IntrThaw` are not here:
            // each allocates a fresh container and needs the lowerer's region
            // slot out of the bytecode, so both stay in the dispatch match.
            Instruction::ArrayMutLen => types::handle_array_len(self),
            Instruction::Length => types::handle_length(self),
            Instruction::IntrGet => types::handle_intr_get(self),
            Instruction::IntrPut => types::handle_intr_put(self),
            Instruction::IntrDel => types::handle_intr_del(self),
            Instruction::IntrHas => types::handle_intr_has(self),
            Instruction::IntrPush => types::handle_intr_push(self),
            Instruction::IntrStringPush => types::handle_intr_string_push(self),
            Instruction::IntrBytesPush => types::handle_intr_bytes_push(self),
            Instruction::IntrPop => types::handle_intr_pop(self),

            other => unreachable!("{other:?} is not a stack-only opcode"),
        }
    }
}
