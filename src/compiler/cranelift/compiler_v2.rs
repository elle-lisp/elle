// Cranelift compiler with symbol table integration (Phase 4+)
//
// This version of the compiler threads symbol table through all methods,
// enabling dynamic primitive operation compilation and function call support.

use super::branching::BranchManager;
use super::codegen::IrEmitter;
use super::funcall::FunctionCallCompiler;
use crate::compiler::ast::Expr;
use crate::symbol::SymbolTable;
use crate::value::Value;
use cranelift::prelude::*;

/// Compilation context that includes the symbol table
pub struct CompileContext<'a> {
    pub builder: &'a mut FunctionBuilder<'a>,
    pub symbol_table: &'a SymbolTable,
}

/// Represents a compiled expression value in CLIF IR (Phase 2+)
#[derive(Debug, Clone, Copy)]
pub enum IrValue {
    /// An i64 SSA value (nil, bool, int, or encoded float)
    I64(cranelift::prelude::Value),
    /// An f64 SSA value (unboxed float)
    F64(cranelift::prelude::Value),
}

/// Expression compiler with full symbol table integration (Phase 4+)
pub struct ExprCompilerV2;

impl ExprCompilerV2 {
    /// Compile an expression within a builder block with symbol table
    ///
    /// This version supports:
    /// - Literal compilation
    /// - Sequence expressions (begin)
    /// - Conditional expressions (if)
    /// - Function calls with constant folding
    pub fn compile_expr_block(ctx: &mut CompileContext, expr: &Expr) -> Result<IrValue, String> {
        match expr {
            Expr::Literal(val) => Self::compile_literal(ctx, val),
            Expr::Begin(exprs) => Self::compile_begin(ctx, exprs),
            Expr::If { cond, then, else_ } => Self::compile_if(ctx, cond, then, else_),
            Expr::Call { func, args, .. } => Self::compile_call(ctx, func, args),
            _ => Err(format!(
                "Expression type not yet supported in JIT: {:?}",
                expr
            )),
        }
    }

    /// Compile a literal value to CLIF IR
    fn compile_literal(ctx: &mut CompileContext, val: &Value) -> Result<IrValue, String> {
        match val {
            Value::Nil => {
                let ir_val = IrEmitter::emit_nil(ctx.builder);
                Ok(IrValue::I64(ir_val))
            }
            Value::Bool(b) => {
                let ir_val = IrEmitter::emit_bool(ctx.builder, *b);
                Ok(IrValue::I64(ir_val))
            }
            Value::Int(i) => {
                let ir_val = IrEmitter::emit_int(ctx.builder, *i);
                Ok(IrValue::I64(ir_val))
            }
            Value::Float(f) => {
                let ir_val = IrEmitter::emit_float(ctx.builder, *f);
                Ok(IrValue::F64(ir_val))
            }
            _ => Err(format!(
                "Cannot compile non-primitive literal in JIT: {:?}",
                val
            )),
        }
    }

    /// Compile a begin (sequence) expression
    fn compile_begin(ctx: &mut CompileContext, exprs: &[Expr]) -> Result<IrValue, String> {
        let mut result = IrValue::I64(IrEmitter::emit_nil(ctx.builder));
        for expr in exprs {
            result = Self::compile_expr_block(ctx, expr)?;
        }
        Ok(result)
    }

    /// Compile an if expression with proper branching
    fn compile_if(
        ctx: &mut CompileContext,
        cond: &Expr,
        then_expr: &Expr,
        else_expr: &Expr,
    ) -> Result<IrValue, String> {
        // Compile condition
        let cond_val = Self::compile_expr_block(ctx, cond)?;

        // Extract i64 value from condition
        let cond_i64 = match cond_val {
            IrValue::I64(v) => v,
            IrValue::F64(_v) => {
                return Err("Float conditions not yet supported".to_string());
            }
        };

        // Create branch blocks
        let (then_block, else_block, join_block) = BranchManager::create_if_blocks(ctx.builder);

        // Emit conditional branch
        BranchManager::emit_if_cond(ctx.builder, cond_i64, then_block, else_block);

        // Compile then branch
        ctx.builder.switch_to_block(then_block);
        ctx.builder.seal_block(then_block);
        let then_val = Self::compile_expr_block(ctx, then_expr)?;
        let then_i64 = Self::ir_value_to_i64(ctx.builder, then_val)?;
        BranchManager::jump_to_join(ctx.builder, join_block, then_i64);

        // Compile else branch
        ctx.builder.switch_to_block(else_block);
        ctx.builder.seal_block(else_block);
        let else_val = Self::compile_expr_block(ctx, else_expr)?;
        let else_i64 = Self::ir_value_to_i64(ctx.builder, else_val)?;
        BranchManager::jump_to_join(ctx.builder, join_block, else_i64);

        // Set up join block and get result
        BranchManager::setup_join_block_for_value(join_block, ctx.builder);
        ctx.builder.switch_to_block(join_block);
        ctx.builder.seal_block(join_block);
        let result_i64 = BranchManager::get_join_value(ctx.builder, join_block);

        Ok(IrValue::I64(result_i64))
    }

    /// Compile a function call
    /// Phase 4: Supports constant folding + framework for future features
    fn compile_call(
        ctx: &mut CompileContext,
        func: &Expr,
        args: &[Expr],
    ) -> Result<IrValue, String> {
        // Try constant folding first (Phase 3 capability)
        match FunctionCallCompiler::try_compile_call(func, args, ctx.symbol_table) {
            super::funcall::CallCompileResult::CompiledConstant(val) => {
                // Convert folded constant to IR value
                Self::compile_literal(ctx, &val)
            }
            super::funcall::CallCompileResult::NotCompilable => {
                // Dynamic call compilation would go here (Phase 4+)
                Err("Dynamic function calls not yet implemented".to_string())
            }
        }
    }

    /// Convert IrValue to i64 for control flow
    fn ir_value_to_i64(
        builder: &mut FunctionBuilder,
        val: IrValue,
    ) -> Result<cranelift::prelude::Value, String> {
        match val {
            IrValue::I64(v) => Ok(v),
            IrValue::F64(_v) => {
                // For now, return a placeholder
                Ok(builder.ins().iconst(types::I64, 0))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_compiler_v2_available() {
        // Phase 4 compiler with symbol table integration is available
        // This verifies the module structure compiles
    }
}
