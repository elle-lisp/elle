// Phase 9 Milestone: Additional Expression Types
//
// This module documents and tests Phase 9 achievements:
// - Cond expression support (multi-way conditionals)
// - While loop support
// - For loop support
// - Expression type analysis framework

#[cfg(test)]
mod tests {
    use crate::compiler::ast::Expr;
    use crate::compiler::cranelift::expr_compiler::ExprCompilerV5;
    use crate::value::{SymbolId, Value};

    #[test]
    fn phase9_cond_simple() {
        // Phase 9: Cond with multiple clauses works correctly
        let clauses = vec![
            (
                Expr::Literal(Value::Bool(false)),
                Expr::Literal(Value::Int(1)),
            ),
            (
                Expr::Literal(Value::Bool(true)),
                Expr::Literal(Value::Int(2)),
            ),
        ];
        let else_body = None;

        let result = ExprCompilerV5::analyze_cond(&clauses, &else_body);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert_eq!(analysis.clause_count, 2);
        assert!(analysis.is_deterministic());
    }

    #[test]
    fn phase9_cond_with_else() {
        // Phase 9: Cond with else clause
        let clauses = vec![
            (
                Expr::Literal(Value::Bool(false)),
                Expr::Literal(Value::Int(1)),
            ),
            (
                Expr::Literal(Value::Bool(false)),
                Expr::Literal(Value::Int(2)),
            ),
        ];
        let else_body = Some(Box::new(Expr::Literal(Value::Int(3))));

        let result = ExprCompilerV5::analyze_cond(&clauses, &else_body);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(analysis.has_else);
    }

    #[test]
    fn phase9_cond_first_true() {
        // Phase 9: Cond takes first true branch
        let clauses = vec![
            (
                Expr::Literal(Value::Bool(false)),
                Expr::Literal(Value::Int(1)),
            ),
            (
                Expr::Literal(Value::Bool(true)),
                Expr::Literal(Value::Int(2)),
            ),
            (
                Expr::Literal(Value::Bool(true)),
                Expr::Literal(Value::Int(3)),
            ),
        ];
        let else_body = None;

        let result = ExprCompilerV5::analyze_cond(&clauses, &else_body);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert_eq!(analysis.deterministic_branch(), Some(1)); // Second clause
    }

    #[test]
    fn phase9_cond_dynamic_conditions() {
        // Phase 9: Cond with dynamic conditions
        let clauses = vec![
            (Expr::Var(SymbolId(1), 0, 0), Expr::Literal(Value::Int(1))),
            (Expr::Var(SymbolId(2), 0, 1), Expr::Literal(Value::Int(2))),
        ];
        let else_body = None;

        let result = ExprCompilerV5::analyze_cond(&clauses, &else_body);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(!analysis.all_constants);
    }

    #[test]
    fn phase9_while_infinite() {
        // Phase 9: While loop with true condition is infinite
        let cond = Expr::Literal(Value::Bool(true));
        let body = Expr::Literal(Value::Int(1));

        let result = ExprCompilerV5::analyze_while(&cond, &body);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(analysis.is_infinite);
    }

    #[test]
    fn phase9_while_never_executes() {
        // Phase 9: While loop with false condition never executes
        let cond = Expr::Literal(Value::Bool(false));
        let body = Expr::Literal(Value::Int(1));

        let result = ExprCompilerV5::analyze_while(&cond, &body);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(analysis.is_never);
        assert!(analysis.can_eliminate());
    }

    #[test]
    fn phase9_while_dynamic_condition() {
        // Phase 9: While loop with dynamic condition
        let cond = Expr::Var(SymbolId(1), 0, 0);
        let body = Expr::Literal(Value::Int(1));

        let result = ExprCompilerV5::analyze_while(&cond, &body);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(!analysis.condition_is_literal);
        assert!(!analysis.can_eliminate());
    }

    #[test]
    fn phase9_while_empty_body() {
        // Phase 9: While loop with empty body can be optimized
        let cond = Expr::Literal(Value::Bool(true));
        let body = Expr::Begin(vec![]);

        let result = ExprCompilerV5::analyze_while(&cond, &body);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(analysis.body_is_empty);
        assert!(analysis.can_eliminate()); // Dead infinite loop
    }

    #[test]
    fn phase9_for_empty_collection() {
        // Phase 9: For loop with empty collection never executes
        let iter = Expr::Literal(Value::Nil);

        let result = ExprCompilerV5::analyze_for(&iter);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(analysis.collection_is_empty);
        assert!(analysis.can_eliminate());
    }

    #[test]
    fn phase9_for_literal_collection() {
        // Phase 9: For loop with literal collection is analyzable
        let iter = Expr::Literal(Value::Int(10));

        let result = ExprCompilerV5::analyze_for(&iter);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(analysis.collection_is_literal);
    }

    #[test]
    fn phase9_for_dynamic_collection() {
        // Phase 9: For loop with dynamic collection
        let iter = Expr::Var(SymbolId(1), 0, 0);

        let result = ExprCompilerV5::analyze_for(&iter);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(!analysis.collection_is_literal);
        assert!(!analysis.can_eliminate());
    }

    #[test]
    fn phase9_cond_analysis_structure() {
        // Phase 9: CondAnalysis provides useful information
        let clauses = vec![
            (
                Expr::Literal(Value::Bool(false)),
                Expr::Literal(Value::Int(1)),
            ),
            (
                Expr::Literal(Value::Bool(false)),
                Expr::Literal(Value::Int(2)),
            ),
            (
                Expr::Literal(Value::Bool(true)),
                Expr::Literal(Value::Int(3)),
            ),
        ];
        let else_body = None;

        let result = ExprCompilerV5::analyze_cond(&clauses, &else_body);
        let analysis = result.unwrap();

        assert_eq!(analysis.clause_count, 3);
        assert!(analysis.all_constants);
        assert!(analysis.is_deterministic());
        assert_eq!(analysis.deterministic_branch(), Some(2));
    }

    #[test]
    fn phase9_while_analysis_structure() {
        // Phase 9: WhileAnalysis provides complete information
        let cond = Expr::Var(SymbolId(1), 0, 0);
        let body = Expr::Literal(Value::Int(1));

        let result = ExprCompilerV5::analyze_while(&cond, &body);
        let analysis = result.unwrap();

        assert!(!analysis.is_infinite);
        assert!(!analysis.is_never);
        assert!(!analysis.body_is_empty);
        assert!(!analysis.condition_is_literal);
    }

    #[test]
    fn phase9_for_analysis_structure() {
        // Phase 9: ForAnalysis provides iteration information
        let iter = Expr::Var(SymbolId(1), 0, 0);

        let result = ExprCompilerV5::analyze_for(&iter);
        let analysis = result.unwrap();

        assert!(!analysis.collection_is_literal);
        assert!(!analysis.collection_is_empty);
        assert!(!analysis.can_eliminate());
    }

    #[test]
    fn phase9_cond_no_clauses() {
        // Phase 9: Cond requires at least one clause
        let clauses = vec![];
        let else_body = None;

        let result = ExprCompilerV5::analyze_cond(&clauses, &else_body);
        assert!(result.is_err());
    }
}
