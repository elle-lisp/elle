// Phase 7 Milestone: Closures and Captured Variables
//
// This module documents and tests Phase 7 achievements:
// - Closure compilation with captured variables
// - Environment packing and unpacking
// - Free variable detection in lambda bodies
// - Nested closure support
// - Higher-order function capabilities

#[cfg(test)]
mod tests {
    use crate::compiler::ast::Expr;
    use crate::compiler::cranelift::closure_compiler::{
        CapturedVar, ClosureCompiler, CompiledClosure, Environment,
    };
    use crate::compiler::cranelift::scoping::ScopeManager;
    use crate::symbol::SymbolTable;
    use crate::value::{SymbolId, Value};

    #[test]
    fn phase7_simple_closure() {
        // Phase 7: Closures can capture variables from outer scope
        let symbol_table = SymbolTable::new();
        let mut scope_manager = ScopeManager::new();

        // Outer scope has a variable
        scope_manager.bind(SymbolId(1));

        // Lambda that captures outer variable
        let params = vec![SymbolId(2)];
        let body = Box::new(Expr::Var(SymbolId(1), 0, 0)); // Reference outer var

        let result =
            ClosureCompiler::compile_with_captures(params, body, &scope_manager, &symbol_table);

        assert!(result.is_ok());
        let closure = result.unwrap();
        assert_eq!(closure.lambda.param_count(), 1);
        assert!(closure.has_captures());
        assert_eq!(closure.environment.capture_count(), 1);
    }

    #[test]
    fn phase7_closure_no_captures() {
        // Phase 7: Lambdas without free variables have no captures
        let symbol_table = SymbolTable::new();
        let scope_manager = ScopeManager::new();

        let params = vec![SymbolId(1)];
        let body = Box::new(Expr::Literal(Value::Int(42))); // No free vars

        let result =
            ClosureCompiler::compile_with_captures(params, body, &scope_manager, &symbol_table);

        assert!(result.is_ok());
        let closure = result.unwrap();
        assert!(!closure.has_captures());
        assert_eq!(closure.environment.capture_count(), 0);
    }

    #[test]
    fn phase7_closure_multiple_captures() {
        // Phase 7: Closures can capture multiple variables
        let symbol_table = SymbolTable::new();
        let mut scope_manager = ScopeManager::new();

        // Outer scope has multiple variables
        scope_manager.bind(SymbolId(1));
        scope_manager.bind(SymbolId(2));
        scope_manager.bind(SymbolId(3));

        let params = vec![SymbolId(4)];
        // Body references multiple outer variables
        let body = Box::new(Expr::Begin(vec![
            Expr::Var(SymbolId(1), 0, 0),
            Expr::Var(SymbolId(3), 0, 2),
        ]));

        let result =
            ClosureCompiler::compile_with_captures(params, body, &scope_manager, &symbol_table);

        assert!(result.is_ok());
        let closure = result.unwrap();
        assert!(closure.has_captures());
        assert!(closure.environment.capture_count() > 0);
    }

    #[test]
    fn phase7_closure_parameter_not_captured() {
        // Phase 7: Function parameters are not captured
        let symbol_table = SymbolTable::new();
        let scope_manager = ScopeManager::new();

        let params = vec![SymbolId(1), SymbolId(2)];
        // Body references only its own parameters
        let body = Box::new(Expr::Var(SymbolId(1), 1, 0)); // Self-reference

        let result =
            ClosureCompiler::compile_with_captures(params, body, &scope_manager, &symbol_table);

        assert!(result.is_ok());
        let closure = result.unwrap();
        // Should have no captures since we're only using own parameters
        assert!(!closure.has_captures());
    }

    #[test]
    fn phase7_environment_management() {
        // Phase 7: Environments properly manage captured values
        let mut env = Environment::new();

        // Add some captures
        let cap1 = CapturedVar {
            sym_id: SymbolId(1),
            depth: 0,
            index: 0,
        };
        let cap2 = CapturedVar {
            sym_id: SymbolId(2),
            depth: 0,
            index: 1,
        };

        env.add_capture(cap1);
        env.add_capture(cap2);

        assert_eq!(env.capture_count(), 2);

        // Pack values
        let values = vec![Value::Int(10), Value::Int(20)];
        let result = env.pack_values(values);
        assert!(result.is_ok());

        assert_eq!(env.values().len(), 2);
    }

    #[test]
    fn phase7_closure_total_vars() {
        // Phase 7: Total variables = parameters + captures
        let closure_no_cap = CompiledClosure::new(
            crate::compiler::cranelift::function_compiler::CompiledLambda::from_expr(
                vec![SymbolId(1), SymbolId(2)],
                Box::new(Expr::Literal(Value::Int(42))),
                vec![],
            ),
            Environment::new(),
        );

        assert_eq!(closure_no_cap.total_vars(), 2);

        // Closure with captures
        let mut env_with_caps = Environment::new();
        env_with_caps.add_capture(CapturedVar {
            sym_id: SymbolId(3),
            depth: 0,
            index: 0,
        });
        env_with_caps.add_capture(CapturedVar {
            sym_id: SymbolId(4),
            depth: 0,
            index: 1,
        });

        let closure_with_cap = CompiledClosure::new(
            crate::compiler::cranelift::function_compiler::CompiledLambda::from_expr(
                vec![SymbolId(1)],
                Box::new(Expr::Literal(Value::Int(42))),
                vec![],
            ),
            env_with_caps,
        );

        assert_eq!(closure_with_cap.total_vars(), 3); // 1 param + 2 captures
    }

    #[test]
    fn phase7_nested_closure() {
        // Phase 7: Nested closures properly track captures
        let symbol_table = SymbolTable::new();
        let mut scope_manager = ScopeManager::new();

        // Outer scope variable
        scope_manager.bind(SymbolId(1));

        // Outer lambda
        scope_manager.push_scope();
        scope_manager.bind(SymbolId(2));

        // Inner lambda references both outer variable and outer param
        let inner_params = vec![SymbolId(3)];
        let inner_body = Box::new(Expr::Begin(vec![
            Expr::Var(SymbolId(1), 0, 0), // Outer variable
            Expr::Var(SymbolId(2), 1, 0), // Outer parameter
        ]));

        let result = ClosureCompiler::compile_with_captures(
            inner_params,
            inner_body,
            &scope_manager,
            &symbol_table,
        );

        assert!(result.is_ok());
        let closure = result.unwrap();
        assert!(closure.has_captures());

        scope_manager.pop_scope().unwrap();
    }

    #[test]
    fn phase7_closure_with_let_binding() {
        // Phase 7: Closures handle let bindings correctly
        let symbol_table = SymbolTable::new();
        let mut scope_manager = ScopeManager::new();

        scope_manager.bind(SymbolId(1)); // outer var

        let params = vec![SymbolId(2)];
        // Body has let binding that uses outer variable
        let body = Box::new(Expr::Let {
            bindings: vec![(SymbolId(3), Expr::Var(SymbolId(1), 0, 0))],
            body: Box::new(Expr::Var(SymbolId(3), 1, 0)),
        });

        let result =
            ClosureCompiler::compile_with_captures(params, body, &scope_manager, &symbol_table);

        assert!(result.is_ok());
        let closure = result.unwrap();
        assert!(closure.has_captures());
    }

    #[test]
    fn phase7_closure_with_conditional() {
        // Phase 7: Closures handle conditionals with captures
        let symbol_table = SymbolTable::new();
        let mut scope_manager = ScopeManager::new();

        scope_manager.bind(SymbolId(1)); // outer var
        scope_manager.bind(SymbolId(2)); // outer var

        let params = vec![SymbolId(3)];
        let body = Box::new(Expr::If {
            cond: Box::new(Expr::Var(SymbolId(1), 0, 0)),
            then: Box::new(Expr::Var(SymbolId(2), 0, 1)),
            else_: Box::new(Expr::Literal(Value::Int(0))),
        });

        let result =
            ClosureCompiler::compile_with_captures(params, body, &scope_manager, &symbol_table);

        assert!(result.is_ok());
        let closure = result.unwrap();
        assert!(closure.has_captures());
        // Should capture both outer variables
        assert!(closure.environment.capture_count() >= 2);
    }

    #[test]
    fn phase7_closure_unique_captures() {
        // Phase 7: Captures are deduplicated (same var referenced multiple times)
        let symbol_table = SymbolTable::new();
        let mut scope_manager = ScopeManager::new();

        scope_manager.bind(SymbolId(1)); // outer var

        let params = vec![SymbolId(2)];
        // Body references same outer variable multiple times
        let body = Box::new(Expr::Begin(vec![
            Expr::Var(SymbolId(1), 0, 0),
            Expr::Var(SymbolId(1), 0, 0), // Same reference again
            Expr::Var(SymbolId(1), 0, 0), // And again
        ]));

        let result =
            ClosureCompiler::compile_with_captures(params, body, &scope_manager, &symbol_table);

        assert!(result.is_ok());
        let closure = result.unwrap();
        // Should only capture once
        assert_eq!(closure.environment.capture_count(), 1);
    }
}
