// Cranelift CLIF IR generation
//
// This module emits actual Cranelift IR instructions that generate
// native machine code. Unlike the BinOpCompiler which operates on
// compile-time constants, this emits runtime code generation.

use cranelift::prelude::*;

/// Emits CLIF IR for primitive value literals
pub struct IrEmitter;

impl IrEmitter {
    /// Emit CLIF IR for an integer literal
    /// Returns a Cranelift Value (SSA value, not Elle Value)
    pub fn emit_int(builder: &mut FunctionBuilder, value: i64) -> cranelift::prelude::Value {
        builder.ins().iconst(types::I64, value)
    }

    /// Emit CLIF IR for a boolean literal
    /// Booleans are encoded as 0 (false) or 1 (true) in i64
    pub fn emit_bool(builder: &mut FunctionBuilder, value: bool) -> cranelift::prelude::Value {
        let encoded = if value { 1 } else { 0 };
        builder.ins().iconst(types::I64, encoded)
    }

    /// Emit CLIF IR for a float literal
    /// Floats are stored as f64 type in CLIF
    pub fn emit_float(builder: &mut FunctionBuilder, value: f64) -> cranelift::prelude::Value {
        builder.ins().f64const(value)
    }

    /// Emit CLIF IR for a nil value
    /// Nil is encoded as 0i64
    pub fn emit_nil(builder: &mut FunctionBuilder) -> cranelift::prelude::Value {
        builder.ins().iconst(types::I64, 0)
    }

    /// Emit CLIF IR for integer addition
    pub fn emit_add_int(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        builder.ins().iadd(left, right)
    }

    /// Emit CLIF IR for integer subtraction
    pub fn emit_sub_int(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        builder.ins().isub(left, right)
    }

    /// Emit CLIF IR for integer multiplication
    pub fn emit_mul_int(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        builder.ins().imul(left, right)
    }

    /// Emit CLIF IR for signed integer division
    pub fn emit_sdiv_int(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        builder.ins().sdiv(left, right)
    }

    /// Emit CLIF IR for float addition
    pub fn emit_add_float(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        builder.ins().fadd(left, right)
    }

    /// Emit CLIF IR for float subtraction
    pub fn emit_sub_float(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        builder.ins().fsub(left, right)
    }

    /// Emit CLIF IR for float multiplication
    pub fn emit_mul_float(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        builder.ins().fmul(left, right)
    }

    /// Emit CLIF IR for float division
    pub fn emit_sdiv_float(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        builder.ins().fdiv(left, right)
    }

    /// Emit CLIF IR for integer less-than comparison
    pub fn emit_lt_int(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        let cond = IntCC::SignedLessThan;
        builder.ins().icmp(cond, left, right)
    }

    /// Emit CLIF IR for integer greater-than comparison
    pub fn emit_gt_int(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        let cond = IntCC::SignedGreaterThan;
        builder.ins().icmp(cond, left, right)
    }

    /// Emit CLIF IR for integer equality comparison
    pub fn emit_eq_int(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        let cond = IntCC::Equal;
        builder.ins().icmp(cond, left, right)
    }

    /// Emit CLIF IR for float less-than comparison
    pub fn emit_lt_float(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        let cond = FloatCC::LessThan;
        builder.ins().fcmp(cond, left, right)
    }

    /// Emit CLIF IR for float greater-than comparison
    pub fn emit_gt_float(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        let cond = FloatCC::GreaterThan;
        builder.ins().fcmp(cond, left, right)
    }

    /// Emit CLIF IR for float equality comparison
    pub fn emit_eq_float(
        builder: &mut FunctionBuilder,
        left: cranelift::prelude::Value,
        right: cranelift::prelude::Value,
    ) -> cranelift::prelude::Value {
        let cond = FloatCC::Equal;
        builder.ins().fcmp(cond, left, right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift::codegen::ir;

    #[test]
    fn test_emit_int() {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let val = IrEmitter::emit_int(&mut builder, 42);
        builder.ins().return_(&[val]);
        builder.finalize();
        // Test passes if no panic occurs
    }

    #[test]
    fn test_emit_bool_true() {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let val = IrEmitter::emit_bool(&mut builder, true);
        builder.ins().return_(&[val]);
        builder.finalize();
    }

    #[test]
    fn test_emit_float() {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.returns.push(AbiParam::new(types::F64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let val = IrEmitter::emit_float(&mut builder, std::f64::consts::PI);
        builder.ins().return_(&[val]);
        builder.finalize();
    }

    #[test]
    fn test_emit_addition() {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.append_block_param(block, types::I64);
        builder.append_block_param(block, types::I64);
        builder.switch_to_block(block);
        builder.seal_block(block);

        let p0 = builder.block_params(block)[0];
        let p1 = builder.block_params(block)[1];
        let result = IrEmitter::emit_add_int(&mut builder, p0, p1);
        builder.ins().return_(&[result]);
        builder.finalize();
    }

    #[test]
    fn test_emit_comparison() {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.append_block_param(block, types::I64);
        builder.append_block_param(block, types::I64);
        builder.switch_to_block(block);
        builder.seal_block(block);

        let p0 = builder.block_params(block)[0];
        let p1 = builder.block_params(block)[1];
        let result = IrEmitter::emit_lt_int(&mut builder, p0, p1);
        builder.ins().return_(&[result]);
        builder.finalize();
    }
}
