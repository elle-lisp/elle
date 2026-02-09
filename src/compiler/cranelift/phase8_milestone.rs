// Phase 8 Milestone: Expression Optimization Passes
//
// This module documents and tests Phase 8 achievements:
// - Expression simplification
// - Constant propagation
// - Dead code elimination (empty blocks)
// - Algebraic identity elimination
// - Optimization pipeline

#[cfg(test)]
mod tests {
    use crate::compiler::ast::Expr;
    use crate::compiler::cranelift::optimizer::{
        ConstantPropagator, ConstantValue, ExprSimplifier, OptimizationResult, Optimizer,
    };
    use crate::value::{SymbolId, Value};

    #[test]
    fn phase8_constant_value_types() {
        // Phase 8: Various constant types are properly recognized
        let const_int = ConstantValue::Int(42);
        assert!(const_int.is_constant());

        let const_float = ConstantValue::Float(3.14);
        assert!(const_float.is_constant());

        let const_bool = ConstantValue::Bool(true);
        assert!(const_bool.is_constant());

        let const_nil = ConstantValue::Nil;
        assert!(const_nil.is_constant());

        let const_unknown = ConstantValue::Unknown;
        assert!(!const_unknown.is_constant());
    }

    #[test]
    fn phase8_simplify_empty_begin() {
        // Phase 8: Empty begin blocks are simplified to nil
        let expr = Expr::Begin(vec![]);
        let (simplified, result) = ExprSimplifier::simplify(&expr);

        assert_eq!(result, OptimizationResult::Optimized);
        assert!(matches!(simplified, Expr::Literal(Value::Nil)));
    }

    #[test]
    fn phase8_simplify_single_element_begin() {
        // Phase 8: Single-element begin blocks are unwrapped
        let expr = Expr::Begin(vec![Expr::Literal(Value::Int(42))]);
        let (simplified, result) = ExprSimplifier::simplify(&expr);

        assert_eq!(result, OptimizationResult::Optimized);
        assert!(matches!(simplified, Expr::Literal(Value::Int(42))));
    }

    #[test]
    fn phase8_branch_prediction_true() {
        // Phase 8: If with constant true condition takes then branch
        let expr = Expr::If {
            cond: Box::new(Expr::Literal(Value::Bool(true))),
            then: Box::new(Expr::Literal(Value::Int(1))),
            else_: Box::new(Expr::Literal(Value::Int(2))),
        };

        let (simplified, result) = ExprSimplifier::simplify(&expr);

        assert_eq!(result, OptimizationResult::Optimized);
        assert!(matches!(simplified, Expr::Literal(Value::Int(1))));
    }

    #[test]
    fn phase8_branch_prediction_false() {
        // Phase 8: If with constant false condition takes else branch
        let expr = Expr::If {
            cond: Box::new(Expr::Literal(Value::Bool(false))),
            then: Box::new(Expr::Literal(Value::Int(1))),
            else_: Box::new(Expr::Literal(Value::Int(2))),
        };

        let (simplified, result) = ExprSimplifier::simplify(&expr);

        assert_eq!(result, OptimizationResult::Optimized);
        assert!(matches!(simplified, Expr::Literal(Value::Int(2))));
    }

    #[test]
    fn phase8_constant_propagation() {
        // Phase 8: Constants are propagated from let bindings
        let expr = Expr::Let {
            bindings: vec![(SymbolId(1), Expr::Literal(Value::Int(42)))],
            body: Box::new(Expr::Var(SymbolId(1), 1, 0)),
        };

        let mut propagator = ConstantPropagator::new();
        let (optimized, result) = propagator.propagate(&expr);

        assert_eq!(result, OptimizationResult::Optimized);
        // Body should be replaced with constant
        if let Expr::Let { body, .. } = &optimized {
            assert!(matches!(**body, Expr::Literal(Value::Int(42))));
        }
    }

    #[test]
    fn phase8_constant_propagation_not_applied() {
        // Phase 8: Non-constant bindings don't propagate as constants
        let expr = Expr::Let {
            bindings: vec![(SymbolId(1), Expr::Var(SymbolId(2), 0, 0))],
            body: Box::new(Expr::Var(SymbolId(1), 1, 0)),
        };

        let mut propagator = ConstantPropagator::new();
        let (_optimized, result) = propagator.propagate(&expr);

        // Since the binding isn't a constant, no optimization should occur
        // (The variable reference remains unchanged)
        assert_eq!(result, OptimizationResult::Unchanged);
    }

    #[test]
    fn phase8_optimization_pipeline() {
        // Phase 8: Multiple optimization passes can be applied
        let expr = Expr::Begin(vec![Expr::Literal(Value::Int(42))]);

        let optimizer = Optimizer::new();
        let (optimized, passes) = optimizer.optimize(expr);

        assert!(passes > 0);
        assert!(matches!(optimized, Expr::Literal(Value::Int(42))));
    }

    #[test]
    fn phase8_nested_optimization() {
        // Phase 8: Nested expressions are optimized recursively
        let expr = Expr::Begin(vec![
            Expr::Begin(vec![Expr::Literal(Value::Int(1))]),
            Expr::Literal(Value::Int(2)),
        ]);

        let (optimized, result) = ExprSimplifier::simplify(&expr);

        assert_eq!(result, OptimizationResult::Optimized);
        // Inner empty begin should be simplified away
        if let Expr::Begin(exprs) = &optimized {
            // Check that inner simplification occurred
            assert!(matches!(exprs[0], Expr::Literal(_)));
        }
    }

    #[test]
    fn phase8_if_simplification_no_constant() {
        // Phase 8: If without constant condition is not simplified
        let expr = Expr::If {
            cond: Box::new(Expr::Var(SymbolId(1), 0, 0)),
            then: Box::new(Expr::Literal(Value::Int(1))),
            else_: Box::new(Expr::Literal(Value::Int(2))),
        };

        let (simplified, result) = ExprSimplifier::simplify(&expr);

        // Without constant condition, the if should not be simplified
        assert_eq!(result, OptimizationResult::Unchanged);
        assert!(matches!(simplified, Expr::If { .. }));
    }

    #[test]
    fn phase8_optimizer_passes_list() {
        // Phase 8: Optimizer tracks which passes are enabled
        let optimizer = Optimizer::new();
        let passes = optimizer.passes();

        assert!(!passes.is_empty());
        assert!(passes.iter().any(|p| p.contains("simplify")));
        assert!(passes.iter().any(|p| p.contains("constant")));
    }

    #[test]
    fn phase8_constant_value_conversion() {
        // Phase 8: Constant values can be converted to Value
        let const_int = ConstantValue::Int(42);
        let val = const_int.to_value();
        assert!(val.is_some());
        assert!(matches!(val.unwrap(), Value::Int(42)));

        let const_unknown = ConstantValue::Unknown;
        let val = const_unknown.to_value();
        assert!(val.is_none());
    }

    #[test]
    fn phase8_complex_constant_propagation() {
        // Phase 8: Multiple constants in let bindings
        let expr = Expr::Let {
            bindings: vec![
                (SymbolId(1), Expr::Literal(Value::Int(10))),
                (SymbolId(2), Expr::Literal(Value::Int(20))),
            ],
            body: Box::new(Expr::Begin(vec![
                Expr::Var(SymbolId(1), 1, 0),
                Expr::Var(SymbolId(2), 1, 1),
            ])),
        };

        let mut propagator = ConstantPropagator::new();
        let (optimized, result) = propagator.propagate(&expr);

        assert_eq!(result, OptimizationResult::Optimized);
        // Both variable references should be replaced with constants
        if let Expr::Let { body, .. } = &optimized {
            if let Expr::Begin(exprs) = &**body {
                assert!(matches!(exprs[0], Expr::Literal(Value::Int(10))));
                assert!(matches!(exprs[1], Expr::Literal(Value::Int(20))));
            }
        }
    }

    #[test]
    fn phase8_optimization_stability() {
        // Phase 8: Optimizing an already-optimized expression doesn't change it further
        let expr = Expr::Literal(Value::Int(42));

        let optimizer = Optimizer::new();
        let (optimized, passes) = optimizer.optimize(expr.clone());

        // A literal should not require any optimization passes
        assert_eq!(passes, 0);
        assert!(matches!(optimized, Expr::Literal(Value::Int(42))));
    }
}
