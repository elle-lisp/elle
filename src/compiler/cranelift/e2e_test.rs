// End-to-end tests for Cranelift JIT compilation
//
// These tests verify that expressions compile to valid CLIF IR
// and can be processed by the Cranelift backend without panicking.

#[cfg(test)]
mod tests {
    use crate::compiler::ast::Expr;
    use crate::compiler::cranelift::compiler::ExprCompiler;
    use crate::value::Value;
    use cranelift::prelude::*;

    fn test_compilation_no_panic(expr: &Expr, name: &str) {
        use crate::compiler::cranelift::compiler::CompileContext;
        use crate::compiler::cranelift::context::JITContext;
        use crate::compiler::cranelift::PrimitiveRegistry;
        use crate::symbol::SymbolTable;

        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = cranelift::codegen::ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));

        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let symbols = SymbolTable::new();
        let primitives = PrimitiveRegistry::new();
        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module, &primitives);
        let result = ExprCompiler::compile_expr_block(&mut ctx, expr);

        // Should not panic and should successfully compile
        assert!(
            result.is_ok(),
            "Failed to compile {}: {:?}",
            name,
            result.err()
        );
    }

    #[test]
    fn test_compile_int_literal() {
        test_compilation_no_panic(&Expr::Literal(Value::Int(42)), "int_literal");
    }

    #[test]
    fn test_compile_bool_literal() {
        test_compilation_no_panic(&Expr::Literal(Value::Bool(true)), "bool_literal");
    }

    #[test]
    fn test_compile_float_literal() {
        test_compilation_no_panic(
            &Expr::Literal(Value::Float(std::f64::consts::PI)),
            "float_literal",
        );
    }

    #[test]
    fn test_compile_nil_literal() {
        test_compilation_no_panic(&Expr::Literal(Value::Nil), "nil_literal");
    }

    #[test]
    fn test_compile_begin_sequence() {
        let expr = Expr::Begin(vec![
            Expr::Literal(Value::Int(1)),
            Expr::Literal(Value::Int(2)),
            Expr::Literal(Value::Int(3)),
        ]);
        test_compilation_no_panic(&expr, "begin_sequence");
    }

    #[test]
    fn test_compile_if_expression() {
        let expr = Expr::If {
            cond: Box::new(Expr::Literal(Value::Bool(true))),
            then: Box::new(Expr::Literal(Value::Int(1))),
            else_: Box::new(Expr::Literal(Value::Int(0))),
        };
        test_compilation_no_panic(&expr, "if_expression");
    }

    #[test]
    fn test_compile_nested_if() {
        let expr = Expr::If {
            cond: Box::new(Expr::Literal(Value::Bool(true))),
            then: Box::new(Expr::If {
                cond: Box::new(Expr::Literal(Value::Bool(false))),
                then: Box::new(Expr::Literal(Value::Int(10))),
                else_: Box::new(Expr::Literal(Value::Int(20))),
            }),
            else_: Box::new(Expr::Literal(Value::Int(30))),
        };
        test_compilation_no_panic(&expr, "nested_if");
    }

    #[test]
    fn test_compile_begin_with_if() {
        let expr = Expr::Begin(vec![
            Expr::Literal(Value::Int(42)),
            Expr::If {
                cond: Box::new(Expr::Literal(Value::Bool(true))),
                then: Box::new(Expr::Literal(Value::Int(1))),
                else_: Box::new(Expr::Literal(Value::Int(0))),
            },
        ]);
        test_compilation_no_panic(&expr, "begin_with_if");
    }

    #[test]
    fn test_compile_mixed_primitives() {
        let expr = Expr::Begin(vec![
            Expr::Literal(Value::Int(42)),
            Expr::Literal(Value::Bool(true)),
            Expr::Literal(Value::Float(std::f64::consts::PI)),
            Expr::Literal(Value::Nil),
        ]);
        test_compilation_no_panic(&expr, "mixed_primitives");
    }

    #[test]
    fn test_compile_complex_expression() {
        let expr = Expr::Begin(vec![
            Expr::Literal(Value::Int(100)),
            Expr::If {
                cond: Box::new(Expr::Literal(Value::Bool(true))),
                then: Box::new(Expr::Begin(vec![
                    Expr::Literal(Value::Int(1)),
                    Expr::Literal(Value::Int(2)),
                ])),
                else_: Box::new(Expr::Literal(Value::Int(0))),
            },
        ]);
        test_compilation_no_panic(&expr, "complex_expression");
    }

    #[test]
    fn test_compile_tail_call_flag() {
        // Test that tail call flag is properly handled during compilation
        // We test with a primitive operation to ensure the tail flag doesn't break compilation
        use crate::compiler::cranelift::compiler::CompileContext;
        use crate::compiler::cranelift::context::JITContext;
        use crate::compiler::cranelift::PrimitiveRegistry;
        use crate::symbol::SymbolTable;
        use crate::value::Value as EllValue;
        use cranelift::prelude::*;

        // Create a symbol table with the necessary symbols
        let mut symbols = SymbolTable::new();
        let plus_sym = symbols.intern("+");

        // Create a simple binary operation marked as a tail call
        // (+ 1 2) marked as a tail call
        let expr = Expr::Call {
            func: Box::new(Expr::GlobalVar(plus_sym)),
            args: vec![
                Expr::Literal(EllValue::Int(1)),
                Expr::Literal(EllValue::Int(2)),
            ],
            tail: true, // This is marked as a tail call
        };

        // Now compile it with the proper symbol table
        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = cranelift::codegen::ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));

        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let primitives = PrimitiveRegistry::new();
        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module, &primitives);
        let result = ExprCompiler::compile_expr_block(&mut ctx, &expr);

        // Should not panic and should successfully compile
        assert!(
            result.is_ok(),
            "Failed to compile tail_call_flag: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compile_primitive_tail_call() {
        // Test that primitive tail calls compile correctly
        use crate::compiler::cranelift::compiler::CompileContext;
        use crate::compiler::cranelift::context::JITContext;
        use crate::compiler::cranelift::PrimitiveRegistry;
        use crate::symbol::SymbolTable;
        use crate::value::Value as EllValue;
        use cranelift::prelude::*;

        // Create a symbol table with the necessary symbols
        let mut symbols = SymbolTable::new();
        let plus_sym = symbols.intern("+");

        // Create a primitive call marked as a tail call
        // (+ 1 2) marked as a tail call
        let expr = Expr::Call {
            func: Box::new(Expr::GlobalVar(plus_sym)),
            args: vec![
                Expr::Literal(EllValue::Int(1)),
                Expr::Literal(EllValue::Int(2)),
            ],
            tail: true, // This is marked as a tail call
        };

        // Now compile it with the proper symbol table
        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = cranelift::codegen::ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));

        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let primitives = PrimitiveRegistry::new();
        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module, &primitives);
        let result = ExprCompiler::compile_expr_block(&mut ctx, &expr);

        // Should not panic and should successfully compile
        assert!(
            result.is_ok(),
            "Failed to compile primitive_tail_call: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compile_global_closure_tail_call() {
        // Test that closure tail calls to global variables now compile successfully
        use crate::compiler::cranelift::compiler::CompileContext;
        use crate::compiler::cranelift::context::JITContext;
        use crate::compiler::cranelift::PrimitiveRegistry;
        use crate::symbol::SymbolTable;
        use crate::value::Value as EllValue;
        use cranelift::prelude::*;

        // Create a symbol table with the necessary symbols
        let mut symbols = SymbolTable::new();
        let f_sym = symbols.intern("f");

        // Create a closure call marked as a tail call
        // (f 1 2) marked as a tail call
        let expr = Expr::Call {
            func: Box::new(Expr::GlobalVar(f_sym)),
            args: vec![
                Expr::Literal(EllValue::Int(1)),
                Expr::Literal(EllValue::Int(2)),
            ],
            tail: true, // This is marked as a tail call
        };

        // Now compile it with the proper symbol table
        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = cranelift::codegen::ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));

        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let primitives = PrimitiveRegistry::new();
        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module, &primitives);
        let result = ExprCompiler::compile_expr_block(&mut ctx, &expr);

        // Should now compile successfully (not return an error)
        assert!(
            result.is_ok(),
            "Failed to compile global_closure_tail_call: {:?}",
            result.err()
        );
    }
}
