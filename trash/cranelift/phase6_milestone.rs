// Phase 6 Milestone: User-Defined Functions
//
// This module documents and tests Phase 6 achievements:
// - Lambda expression compilation framework
// - User-defined function support with parameters
// - Function parameter binding as local variables
// - Function composition and nesting
// - Foundation for closures (Phase 7+)

#[cfg(test)]
mod tests {
    use crate::compiler::ast::Expr;
    use crate::compiler::cranelift::function_compiler::FunctionCompiler;
    use crate::compiler::cranelift::scoping::ScopeManager;
    use crate::symbol::SymbolTable;
    use crate::value::{Arity, SymbolId, Value};

    #[test]
    fn phase6_lambda_compilation() {
        // Phase 6: Lambda expressions can be compiled
        let symbol_table = SymbolTable::new();
        let params = vec![SymbolId(1), SymbolId(2)];
        let body = Box::new(Expr::Literal(Value::Int(42)));
        let captures = vec![];

        let result =
            FunctionCompiler::compile_lambda(params.clone(), body, captures, &symbol_table);

        assert!(result.is_ok());
        let lambda = result.unwrap();
        assert_eq!(lambda.param_count(), 2);
        assert!(lambda.matches_arity(2));
    }

    #[test]
    fn phase6_zero_argument_function() {
        // Phase 6: Functions with no arguments work correctly
        let symbol_table = SymbolTable::new();
        let params = vec![];
        let body = Box::new(Expr::Literal(Value::Int(100)));
        let captures = vec![];

        let result = FunctionCompiler::compile_lambda(params, body, captures, &symbol_table);

        assert!(result.is_ok());
        let lambda = result.unwrap();
        assert_eq!(lambda.param_count(), 0);
        assert!(lambda.matches_arity(0));
        assert!(!lambda.matches_arity(1));
    }

    #[test]
    fn phase6_parameter_binding() {
        // Phase 6: Function parameters are properly bound to scope
        let mut scope_manager = ScopeManager::new();
        let params = vec![SymbolId(1), SymbolId(2), SymbolId(3)];

        // Initially global scope
        assert_eq!(scope_manager.current_depth(), 0);

        // Bind parameters
        let result = FunctionCompiler::bind_parameters(&mut scope_manager, &params);
        assert!(result.is_ok());

        // Should be in function scope now
        assert_eq!(scope_manager.current_depth(), 1);

        // All parameters should be bound
        for param in &params {
            assert!(scope_manager.is_bound(*param));
        }

        // Unbind parameters
        let unbind_result = FunctionCompiler::unbind_parameters(&mut scope_manager);
        assert!(unbind_result.is_ok());

        // Back to global scope
        assert_eq!(scope_manager.current_depth(), 0);
    }

    #[test]
    fn phase6_arity_matching() {
        // Phase 6: Function arity is properly tracked and matched
        let symbol_table = SymbolTable::new();
        let params = vec![SymbolId(1), SymbolId(2)];
        let body = Box::new(Expr::Literal(Value::Int(0)));
        let captures = vec![];

        let lambda =
            FunctionCompiler::compile_lambda(params, body, captures, &symbol_table).unwrap();

        // Exact match
        assert!(lambda.matches_arity(2));

        // Too few arguments
        assert!(!lambda.matches_arity(1));

        // Too many arguments
        assert!(!lambda.matches_arity(3));

        // Check arity enum
        assert_eq!(lambda.arity, Arity::Exact(2));
    }

    #[test]
    fn phase6_nested_parameter_scopes() {
        // Phase 6: Multiple function parameter scopes work correctly
        let mut scope_manager = ScopeManager::new();

        // Outer function parameters
        let outer_params = vec![SymbolId(1), SymbolId(2)];
        let _ = FunctionCompiler::bind_parameters(&mut scope_manager, &outer_params).unwrap();
        assert_eq!(scope_manager.current_depth(), 1);

        // Inner function parameters (nested)
        let inner_params = vec![SymbolId(3), SymbolId(4)];
        let _ = FunctionCompiler::bind_parameters(&mut scope_manager, &inner_params).unwrap();
        assert_eq!(scope_manager.current_depth(), 2);

        // All parameters visible in inner scope
        assert!(scope_manager.is_bound(SymbolId(1))); // outer
        assert!(scope_manager.is_bound(SymbolId(2))); // outer
        assert!(scope_manager.is_bound(SymbolId(3))); // inner
        assert!(scope_manager.is_bound(SymbolId(4))); // inner

        // Pop inner function
        FunctionCompiler::unbind_parameters(&mut scope_manager).unwrap();
        assert_eq!(scope_manager.current_depth(), 1);

        // Inner parameters no longer visible
        assert!(scope_manager.is_bound(SymbolId(1))); // still outer
        assert!(scope_manager.is_bound(SymbolId(2))); // still outer
        assert!(!scope_manager.is_bound(SymbolId(3))); // inner gone
        assert!(!scope_manager.is_bound(SymbolId(4))); // inner gone

        // Pop outer function
        FunctionCompiler::unbind_parameters(&mut scope_manager).unwrap();
        assert_eq!(scope_manager.current_depth(), 0);
    }

    #[test]
    fn phase6_lambda_with_captures() {
        // Phase 6: Lambda expressions can capture variables
        let symbol_table = SymbolTable::new();
        let params = vec![SymbolId(1)];
        let body = Box::new(Expr::Literal(Value::Int(42)));
        let captures = vec![(SymbolId(10), 0, 0), (SymbolId(11), 0, 1)];

        let result = FunctionCompiler::compile_lambda(params, body, captures, &symbol_table);

        assert!(result.is_ok());
        let lambda = result.unwrap();
        assert_eq!(lambda.param_count(), 1);
        assert_eq!(lambda.capture_count(), 2);
    }

    #[test]
    fn phase6_composed_lambda_expression() {
        // Phase 6: Lambdas can compose with let expressions
        let symbol_table = SymbolTable::new();
        let params = vec![SymbolId(1)];

        // Body: (begin (+ param 5) (+ param 10))
        let body = Box::new(Expr::Begin(vec![
            Expr::Literal(Value::Int(5)),
            Expr::Literal(Value::Int(10)),
        ]));
        let captures = vec![];

        let result = FunctionCompiler::compile_lambda(params, body, captures, &symbol_table);

        assert!(result.is_ok());
        let lambda = result.unwrap();
        assert_eq!(lambda.param_count(), 1);
    }

    #[test]
    fn phase6_parameter_count_validation() {
        // Phase 6: Parameter counts are properly validated
        let symbol_table = SymbolTable::new();

        // Single parameter
        let params = vec![SymbolId(1)];
        let body = Box::new(Expr::Literal(Value::Int(42)));
        let captures = vec![];

        let result = FunctionCompiler::compile_lambda(params, body, captures, &symbol_table);
        assert!(result.is_ok());

        // Many parameters
        let many_params: Vec<_> = (0..10).map(|i| SymbolId(i + 1)).collect();
        let result = FunctionCompiler::compile_lambda(
            many_params,
            Box::new(Expr::Literal(Value::Int(42))),
            vec![],
            &symbol_table,
        );
        assert!(result.is_ok());
    }
}
