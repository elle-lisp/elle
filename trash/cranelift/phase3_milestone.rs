// Phase 3 Milestone: Symbol Table Integration
//
// This module documents and tests the Phase 3 achievements:
// - Symbol table integration for function call compilation
// - Constant folding for all primitive operations
// - Foundation for runtime primitive operation compilation

#[cfg(test)]
mod tests {
    use crate::compiler::ast::Expr;
    use crate::compiler::cranelift::funcall::{CallCompileResult, FunctionCallCompiler};
    use crate::symbol::SymbolTable;
    use crate::value::Value;

    #[test]
    fn phase3_milestone_constant_folding() {
        // Phase 3 enables constant folding via symbol table integration
        let mut symbols = SymbolTable::new();

        // Create symbols for primitive operations
        let add_sym = symbols.intern("+");
        let mul_sym = symbols.intern("*");
        let lt_sym = symbols.intern("<");
        let eq_sym = symbols.intern("=");

        // Test constant folding for various operations
        let tests = vec![
            // (symbol_id, args, expected_result)
            (
                add_sym,
                vec![Value::Int(10), Value::Int(20)],
                Value::Int(30),
            ),
            (mul_sym, vec![Value::Int(5), Value::Int(6)], Value::Int(30)),
            (
                lt_sym,
                vec![Value::Int(3), Value::Int(10)],
                Value::Bool(true),
            ),
            (
                eq_sym,
                vec![Value::Int(5), Value::Int(5)],
                Value::Bool(true),
            ),
        ];

        for (sym_id, args, expected) in tests {
            let arg_exprs: Vec<Expr> = args.iter().map(|v| Expr::Literal(v.clone())).collect();

            let result = FunctionCallCompiler::try_compile_call(
                &Expr::Literal(Value::Symbol(sym_id)),
                &arg_exprs,
                &symbols,
            );

            match result {
                CallCompileResult::CompiledConstant(val) => {
                    assert_eq!(val, expected, "Constant folding mismatch for {:?}", sym_id);
                }
                CallCompileResult::NotCompilable => {
                    panic!("Expected constant folding for {:?}", sym_id);
                }
            }
        }
    }

    #[test]
    fn phase3_milestone_variadic_operations() {
        // Phase 3 supports variadic operations (multiple arguments)
        let mut symbols = SymbolTable::new();
        let add_sym = symbols.intern("+");

        // Test variadic addition: (+ 1 2 3 4 5) => 15
        let args = vec![
            Expr::Literal(Value::Int(1)),
            Expr::Literal(Value::Int(2)),
            Expr::Literal(Value::Int(3)),
            Expr::Literal(Value::Int(4)),
            Expr::Literal(Value::Int(5)),
        ];

        let result = FunctionCallCompiler::try_compile_call(
            &Expr::Literal(Value::Symbol(add_sym)),
            &args,
            &symbols,
        );

        match result {
            CallCompileResult::CompiledConstant(Value::Int(15)) => (),
            _ => panic!("Expected constant 15 from variadic add, got {:?}", result),
        }
    }

    #[test]
    fn phase3_milestone_mixed_types() {
        // Phase 3 supports mixed int/float arithmetic
        let mut symbols = SymbolTable::new();
        let add_sym = symbols.intern("+");

        // Test mixed type: (+ 10 3.14) => 13.14
        let args = vec![
            Expr::Literal(Value::Int(10)),
            Expr::Literal(Value::Float(3.14)),
        ];

        let result = FunctionCallCompiler::try_compile_call(
            &Expr::Literal(Value::Symbol(add_sym)),
            &args,
            &symbols,
        );

        match result {
            CallCompileResult::CompiledConstant(Value::Float(f)) => {
                // Account for floating point imprecision
                assert!((f - 13.14).abs() < 0.0001);
            }
            _ => panic!("Expected float result from mixed arithmetic"),
        }
    }

    #[test]
    fn phase3_milestone_all_primitives() {
        // Phase 3 milestone: Support for all 10 primitive operations
        let mut symbols = SymbolTable::new();

        let ops = vec![
            ("+", "addition", Value::Int(1), Value::Int(2), Value::Int(3)),
            (
                "-",
                "subtraction",
                Value::Int(10),
                Value::Int(3),
                Value::Int(7),
            ),
            (
                "*",
                "multiplication",
                Value::Int(4),
                Value::Int(5),
                Value::Int(20),
            ),
            (
                "/",
                "division",
                Value::Int(20),
                Value::Int(4),
                Value::Int(5),
            ),
            (
                "<",
                "less-than",
                Value::Int(1),
                Value::Int(5),
                Value::Bool(true),
            ),
            (
                ">",
                "greater-than",
                Value::Int(5),
                Value::Int(1),
                Value::Bool(true),
            ),
            (
                "=",
                "equality",
                Value::Int(5),
                Value::Int(5),
                Value::Bool(true),
            ),
            (
                "<=",
                "less-or-equal",
                Value::Int(5),
                Value::Int(5),
                Value::Bool(true),
            ),
            (
                ">=",
                "greater-or-equal",
                Value::Int(5),
                Value::Int(5),
                Value::Bool(true),
            ),
            (
                "!=",
                "not-equal",
                Value::Int(3),
                Value::Int(5),
                Value::Bool(true),
            ),
        ];

        for (op_str, name, arg1, arg2, expected) in ops {
            let sym = symbols.intern(op_str);
            let args = vec![Expr::Literal(arg1), Expr::Literal(arg2)];

            let result = FunctionCallCompiler::try_compile_call(
                &Expr::Literal(Value::Symbol(sym)),
                &args,
                &symbols,
            );

            match result {
                CallCompileResult::CompiledConstant(val) => {
                    assert_eq!(val, expected, "Mismatch for {}", name);
                }
                CallCompileResult::NotCompilable => {
                    panic!("Expected constant folding for {}", name);
                }
            }
        }
    }
}
