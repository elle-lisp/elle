// Cranelift code generation for Elle Lisp expressions
//
// This module handles the core logic of translating Elle AST expressions
// into Cranelift IR (CLIF) and compiling to native x86_64 code.

use super::branching::BranchManager;
use super::codegen::IrEmitter;
use super::context::JITContext;
use crate::compiler::ast::Expr;
use crate::value::Value;
use cranelift::prelude::*;
use cranelift_module::Module;

/// Represents a compiled expression value in CLIF IR
/// Maps Elle values to Cranelift SSA values
#[derive(Debug, Clone, Copy)]
pub enum IrValue {
    /// An i64 SSA value (nil, bool, int, or encoded float)
    I64(cranelift::prelude::Value),
    /// An f64 SSA value (unboxed float)
    F64(cranelift::prelude::Value),
}

/// Expression compiler
pub struct ExprCompiler;

impl ExprCompiler {
    /// Compile a single expression to a function
    pub fn compile_expr(
        ctx: &mut JITContext,
        name: &str,
        expr: &Expr,
    ) -> Result<*const u8, String> {
        // Create function signature: fn(args_ptr: i64, args_len: i64) -> i64
        let mut sig = ctx.make_signature();
        sig.params.push(AbiParam::new(types::I64)); // args pointer
        sig.params.push(AbiParam::new(types::I64)); // args length
        sig.returns.push(AbiParam::new(types::I64)); // return value

        let func_id = ctx.declare_function(name, sig)?;

        // Set the signature before building
        ctx.ctx.func.signature = ctx.module.make_signature();
        ctx.ctx
            .func
            .signature
            .params
            .push(AbiParam::new(types::I64));
        ctx.ctx
            .func
            .signature
            .params
            .push(AbiParam::new(types::I64));
        ctx.ctx
            .func
            .signature
            .returns
            .push(AbiParam::new(types::I64));

        let mut builder = FunctionBuilder::new(&mut ctx.ctx.func, &mut ctx.builder_ctx);
        let entry_block = builder.create_block();
        builder.append_block_param(entry_block, types::I64); // args pointer
        builder.append_block_param(entry_block, types::I64); // args length
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // Compile the expression
        let result = Self::compile_expr_block(&mut builder, expr)?;

        // Convert the compiled value to i64 for return
        let return_val = match result {
            IrValue::I64(v) => v,
            IrValue::F64(_v) => {
                // TODO: Encode float as its bit representation (i64)
                // For now, return 0
                builder.ins().iconst(types::I64, 0)
            }
        };
        builder.ins().return_(&[return_val]);

        builder.finalize();

        ctx.define_function(func_id)?;
        ctx.clear();

        Ok(ctx.get_function(func_id))
    }

    /// Compile an expression within a builder block
    /// Returns an IrValue (Cranelift SSA value)
    pub fn compile_expr_block(
        builder: &mut FunctionBuilder,
        expr: &Expr,
    ) -> Result<IrValue, String> {
        match expr {
            Expr::Literal(val) => Self::compile_literal(builder, val),
            Expr::Begin(exprs) => Self::compile_begin(builder, exprs),
            Expr::If { cond, then, else_ } => Self::compile_if(builder, cond, then, else_),
            _ => Err(format!(
                "Expression type not yet supported in JIT: {:?}",
                expr
            )),
        }
    }

    /// Compile a literal value to CLIF IR
    fn compile_literal(builder: &mut FunctionBuilder, val: &Value) -> Result<IrValue, String> {
        match val {
            Value::Nil => {
                // Nil is encoded as 0i64
                let ir_val = IrEmitter::emit_nil(builder);
                Ok(IrValue::I64(ir_val))
            }
            Value::Bool(b) => {
                // Bool is encoded as 0 (false) or 1 (true)
                let ir_val = IrEmitter::emit_bool(builder, *b);
                Ok(IrValue::I64(ir_val))
            }
            Value::Int(i) => {
                // Int is emitted directly
                let ir_val = IrEmitter::emit_int(builder, *i);
                Ok(IrValue::I64(ir_val))
            }
            Value::Float(f) => {
                // Float is emitted as f64
                let ir_val = IrEmitter::emit_float(builder, *f);
                Ok(IrValue::F64(ir_val))
            }
            _ => Err(format!(
                "Cannot compile non-primitive literal in JIT: {:?}",
                val
            )),
        }
    }

    /// Compile a begin (sequence) expression
    fn compile_begin(builder: &mut FunctionBuilder, exprs: &[Expr]) -> Result<IrValue, String> {
        let mut result = IrValue::I64(IrEmitter::emit_nil(builder));
        for expr in exprs {
            result = Self::compile_expr_block(builder, expr)?;
        }
        Ok(result)
    }

    /// Compile an if expression with proper conditional branching
    fn compile_if(
        builder: &mut FunctionBuilder,
        cond: &Expr,
        then_expr: &Expr,
        else_expr: &Expr,
    ) -> Result<IrValue, String> {
        // Compile the condition expression
        let cond_val = Self::compile_expr_block(builder, cond)?;

        // Extract i64 value from condition (floats would need conversion)
        let cond_i64 = match cond_val {
            IrValue::I64(v) => v,
            IrValue::F64(_v) => {
                // For now, treat any f64 as truthy (non-zero)
                // TODO: Proper float-to-int conversion
                return Err("Float conditions not yet supported".to_string());
            }
        };

        // Create branch blocks
        let (then_block, else_block, join_block) = BranchManager::create_if_blocks(builder);

        // Emit the conditional branch
        BranchManager::emit_if_cond(builder, cond_i64, then_block, else_block);

        // Compile then branch
        builder.switch_to_block(then_block);
        builder.seal_block(then_block);
        let then_val = Self::compile_expr_block(builder, then_expr)?;
        let then_i64 = Self::ir_value_to_i64(builder, then_val)?;
        BranchManager::jump_to_join(builder, join_block, then_i64);

        // Compile else branch
        builder.switch_to_block(else_block);
        builder.seal_block(else_block);
        let else_val = Self::compile_expr_block(builder, else_expr)?;
        let else_i64 = Self::ir_value_to_i64(builder, else_val)?;
        BranchManager::jump_to_join(builder, join_block, else_i64);

        // Set up join block and get the result value
        BranchManager::setup_join_block_for_value(join_block, builder);
        builder.switch_to_block(join_block);
        builder.seal_block(join_block);
        let result_i64 = BranchManager::get_join_value(builder, join_block);

        Ok(IrValue::I64(result_i64))
    }

    /// Convert IrValue to i64 for control flow operations
    fn ir_value_to_i64(
        builder: &mut FunctionBuilder,
        val: IrValue,
    ) -> Result<cranelift::prelude::Value, String> {
        match val {
            IrValue::I64(v) => Ok(v),
            IrValue::F64(_v) => {
                // For now, return a placeholder
                // TODO: Proper float-to-i64 encoding
                Ok(builder.ins().iconst(types::I64, 0))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift::codegen::ir;

    #[test]
    fn test_compile_expr_block_literal() {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let result = ExprCompiler::compile_expr_block(&mut builder, &Expr::Literal(Value::Int(42)));
        assert!(
            result.is_ok(),
            "Failed to compile integer literal: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compile_expr_block_bool() {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let result =
            ExprCompiler::compile_expr_block(&mut builder, &Expr::Literal(Value::Bool(true)));
        assert!(
            result.is_ok(),
            "Failed to compile boolean literal: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compile_expr_block_begin() {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let result = ExprCompiler::compile_expr_block(
            &mut builder,
            &Expr::Begin(vec![
                Expr::Literal(Value::Int(1)),
                Expr::Literal(Value::Int(2)),
            ]),
        );
        assert!(
            result.is_ok(),
            "Failed to compile begin expression: {:?}",
            result.err()
        );
    }
}
