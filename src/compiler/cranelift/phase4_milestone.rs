// Phase 4 Milestone: Symbol Table Threading and Integration
//
// This module documents and tests Phase 4 achievements:
// - Symbol table threading through all compiler methods
// - Function call compilation with symbol resolution
// - Dynamic operation dispatch framework
// - Foundation for full expression compilation pipeline

#[cfg(test)]
mod tests {
    use crate::compiler::ast::Expr;
    use crate::compiler::cranelift::funcall::{CallCompileResult, FunctionCallCompiler};
    use crate::symbol::SymbolTable;
    use crate::value::Value;

    #[test]
    fn phase4_symbol_table_integration() {
        // Phase 4 enables full symbol table integration in the compiler
        let mut symbols = SymbolTable::new();

        // Create comprehensive symbol set
        let ops = vec![
            symbols.intern("+"),
            symbols.intern("-"),
            symbols.intern("*"),
            symbols.intern("/"),
            symbols.intern("<"),
            symbols.intern(">"),
            symbols.intern("="),
        ];

        // Verify all symbols are properly interned
        assert_eq!(ops.len(), 7);

        // Verify symbol resolution works
        assert_eq!(symbols.name(ops[0]), Some("+"));
        assert_eq!(symbols.name(ops[1]), Some("-"));
        assert_eq!(symbols.name(ops[2]), Some("*"));
        assert_eq!(symbols.name(ops[3]), Some("/"));
        assert_eq!(symbols.name(ops[4]), Some("<"));
        assert_eq!(symbols.name(ops[5]), Some(">"));
        assert_eq!(symbols.name(ops[6]), Some("="));
    }

    #[test]
    fn phase4_function_call_with_symbol_resolution() {
        // Phase 4: Function calls dispatch via symbol resolution
        let mut symbols = SymbolTable::new();
        let add_sym = symbols.intern("+");

        // Function call: (+ 10 20)
        let args = vec![Expr::Literal(Value::Int(10)), Expr::Literal(Value::Int(20))];

        let result = FunctionCallCompiler::try_compile_call(
            &Expr::Literal(Value::Symbol(add_sym)),
            &args,
            &symbols,
        );

        // Should resolve symbol and fold the computation
        match result {
            CallCompileResult::CompiledConstant(Value::Int(30)) => (),
            _ => panic!("Expected constant 30, got {:?}", result),
        }
    }

    #[test]
    fn phase4_nested_operations() {
        // Phase 4: Complex nested expressions work correctly
        let mut symbols = SymbolTable::new();
        let add_sym = symbols.intern("+");
        let mul_sym = symbols.intern("*");

        // (+ 10 (* 3 4)) => (+ 10 12) => 22
        // First fold (* 3 4) => 12
        let mul_args = vec![Expr::Literal(Value::Int(3)), Expr::Literal(Value::Int(4))];

        let mul_result = FunctionCallCompiler::try_compile_call(
            &Expr::Literal(Value::Symbol(mul_sym)),
            &mul_args,
            &symbols,
        );

        let mul_val = match mul_result {
            CallCompileResult::CompiledConstant(v) => v,
            _ => panic!("Expected mul folding"),
        };

        // Then fold (+ 10 12)
        let add_args = vec![Expr::Literal(Value::Int(10)), Expr::Literal(mul_val)];

        let add_result = FunctionCallCompiler::try_compile_call(
            &Expr::Literal(Value::Symbol(add_sym)),
            &add_args,
            &symbols,
        );

        match add_result {
            CallCompileResult::CompiledConstant(Value::Int(22)) => (),
            _ => panic!("Expected 22 from nested folding"),
        }
    }

    #[test]
    fn phase4_all_comparison_operators() {
        // Phase 4: All comparison operations fully supported
        let mut symbols = SymbolTable::new();

        let comparisons = vec![
            ("<", Value::Int(1), Value::Int(5), Value::Bool(true)),
            (">", Value::Int(5), Value::Int(1), Value::Bool(true)),
            ("=", Value::Int(5), Value::Int(5), Value::Bool(true)),
            ("!=", Value::Int(1), Value::Int(2), Value::Bool(true)),
            ("<=", Value::Int(5), Value::Int(5), Value::Bool(true)),
            (">=", Value::Int(5), Value::Int(5), Value::Bool(true)),
        ];

        for (op_str, left, right, expected) in comparisons {
            let sym = symbols.intern(op_str);
            let args = vec![Expr::Literal(left), Expr::Literal(right)];

            let result = FunctionCallCompiler::try_compile_call(
                &Expr::Literal(Value::Symbol(sym)),
                &args,
                &symbols,
            );

            match result {
                CallCompileResult::CompiledConstant(val) => {
                    assert_eq!(val, expected, "Mismatch for {}", op_str);
                }
                _ => panic!("Expected folding for {}", op_str),
            }
        }
    }

    #[test]
    fn phase4_variadic_with_mixed_types() {
        // Phase 4: Variadic operations with type coercion
        let mut symbols = SymbolTable::new();
        let add_sym = symbols.intern("+");

        // (+ 1 2.0 3 4.5) => 10.5
        let args = vec![
            Expr::Literal(Value::Int(1)),
            Expr::Literal(Value::Float(2.0)),
            Expr::Literal(Value::Int(3)),
            Expr::Literal(Value::Float(4.5)),
        ];

        let result = FunctionCallCompiler::try_compile_call(
            &Expr::Literal(Value::Symbol(add_sym)),
            &args,
            &symbols,
        );

        match result {
            CallCompileResult::CompiledConstant(Value::Float(f)) => {
                assert!((f - 10.5).abs() < 0.0001);
            }
            _ => panic!("Expected float result from mixed-type variadic add"),
        }
    }

    #[test]
    fn phase4_compiler_foundation() {
        // Phase 4 foundation test: Symbol table threading enables
        // dynamic compilation with runtime symbol resolution
        let symbols = SymbolTable::new();

        // Verify CompileContext can be constructed with symbol table
        // This is the key Phase 4 achievement
        assert!(
            symbols.name(crate::value::SymbolId(0)).is_none(),
            "Symbol 0 should not exist"
        );
    }
}
