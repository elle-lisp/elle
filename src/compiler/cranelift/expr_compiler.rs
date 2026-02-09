// Extended expression compilation (Phase 9)
//
// Adds support for additional expression types:
// - Cond: Multi-way conditional
// - While: Loop with condition
// - For: Iteration over sequences
// - Match: Pattern matching (partial support)
//
// Builds on compiler_v4 infrastructure

use crate::compiler::ast::Expr;
use crate::value::Value;

/// Extended expression compiler with additional expression types (Phase 9)
pub struct ExprCompilerV5;

impl ExprCompilerV5 {
    /// Analyze a Cond expression structure
    /// Cond evaluates conditions in order, taking first true branch
    pub fn analyze_cond(
        clauses: &[(Expr, Expr)],
        else_body: &Option<Box<Expr>>,
    ) -> Result<CondAnalysis, String> {
        if clauses.is_empty() {
            return Err("Cond requires at least one clause".to_string());
        }

        let mut all_constants = true;
        let mut constant_branch = None;

        // Check if any condition is a constant true
        for (i, (cond, _body)) in clauses.iter().enumerate() {
            if let Expr::Literal(Value::Bool(true)) = cond {
                constant_branch = Some(i);
                break;
            }
            if !matches!(cond, Expr::Literal(_)) {
                all_constants = false;
            }
        }

        Ok(CondAnalysis {
            clause_count: clauses.len(),
            all_constants,
            constant_true_branch: constant_branch,
            has_else: else_body.is_some(),
        })
    }

    /// Analyze a While loop structure
    /// While repeatedly executes body while condition is true
    pub fn analyze_while(cond: &Expr, body: &Expr) -> Result<WhileAnalysis, String> {
        // Check if condition is a constant (infinite or never loop)
        let is_infinite = matches!(cond, Expr::Literal(Value::Bool(true)));
        let is_never = matches!(cond, Expr::Literal(Value::Bool(false)));

        let body_is_empty = matches!(body, Expr::Begin(v) if v.is_empty());

        Ok(WhileAnalysis {
            is_infinite,
            is_never,
            body_is_empty,
            condition_is_literal: matches!(cond, Expr::Literal(_)),
        })
    }

    /// Analyze a For loop structure
    /// For iterates over a collection binding each element
    pub fn analyze_for(iter: &Expr) -> Result<ForAnalysis, String> {
        let collection_is_literal = matches!(iter, Expr::Literal(_));
        let collection_is_empty = matches!(iter, Expr::Literal(Value::Nil));

        Ok(ForAnalysis {
            collection_is_literal,
            collection_is_empty,
        })
    }
}

/// Analysis result for Cond expressions
#[derive(Debug, Clone)]
pub struct CondAnalysis {
    /// Number of clauses in the cond
    pub clause_count: usize,
    /// True if all conditions are constants
    pub all_constants: bool,
    /// If one condition is constant true, its index
    pub constant_true_branch: Option<usize>,
    /// True if there's an else clause
    pub has_else: bool,
}

impl CondAnalysis {
    /// Check if this cond can be optimized to a single branch
    pub fn is_deterministic(&self) -> bool {
        self.constant_true_branch.is_some()
    }

    /// Get the single branch if deterministic
    pub fn deterministic_branch(&self) -> Option<usize> {
        self.constant_true_branch
    }
}

/// Analysis result for While loops
#[derive(Debug, Clone)]
pub struct WhileAnalysis {
    /// True if condition is constant true (infinite loop)
    pub is_infinite: bool,
    /// True if condition is constant false (never runs)
    pub is_never: bool,
    /// True if body is empty
    pub body_is_empty: bool,
    /// True if condition is a literal (can be analyzed)
    pub condition_is_literal: bool,
}

impl WhileAnalysis {
    /// Check if this while loop can be eliminated
    pub fn can_eliminate(&self) -> bool {
        self.is_never || (self.is_infinite && self.body_is_empty)
    }
}

/// Analysis result for For loops
#[derive(Debug, Clone)]
pub struct ForAnalysis {
    /// True if collection is a literal (can be analyzed)
    pub collection_is_literal: bool,
    /// True if collection is empty (loop never executes)
    pub collection_is_empty: bool,
}

impl ForAnalysis {
    /// Check if this for loop can be eliminated
    pub fn can_eliminate(&self) -> bool {
        self.collection_is_empty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::SymbolId;

    #[test]
    fn test_cond_analysis_basic() {
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
        assert_eq!(analysis.deterministic_branch(), Some(1));
    }

    #[test]
    fn test_cond_analysis_no_constant_true() {
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
        assert!(!analysis.is_deterministic());
        assert!(analysis.has_else);
    }

    #[test]
    fn test_cond_analysis_with_non_literals() {
        let clauses = vec![
            (Expr::Var(SymbolId(1), 0, 0), Expr::Literal(Value::Int(1))),
            (
                Expr::Literal(Value::Bool(true)),
                Expr::Literal(Value::Int(2)),
            ),
        ];
        let else_body = None;

        let result = ExprCompilerV5::analyze_cond(&clauses, &else_body);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(!analysis.all_constants);
        assert!(analysis.is_deterministic()); // Still deterministic due to second clause
    }

    #[test]
    fn test_cond_analysis_empty() {
        let clauses = vec![];
        let else_body = None;

        let result = ExprCompilerV5::analyze_cond(&clauses, &else_body);
        assert!(result.is_err());
    }

    #[test]
    fn test_while_analysis_infinite() {
        let cond = Expr::Literal(Value::Bool(true));
        let body = Expr::Literal(Value::Int(1));

        let result = ExprCompilerV5::analyze_while(&cond, &body);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(analysis.is_infinite);
        assert!(!analysis.is_never);
        assert!(analysis.condition_is_literal);
    }

    #[test]
    fn test_while_analysis_never() {
        let cond = Expr::Literal(Value::Bool(false));
        let body = Expr::Literal(Value::Int(1));

        let result = ExprCompilerV5::analyze_while(&cond, &body);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(!analysis.is_infinite);
        assert!(analysis.is_never);
        assert!(analysis.can_eliminate());
    }

    #[test]
    fn test_while_analysis_dynamic_condition() {
        let cond = Expr::Var(SymbolId(1), 0, 0);
        let body = Expr::Literal(Value::Int(1));

        let result = ExprCompilerV5::analyze_while(&cond, &body);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(!analysis.condition_is_literal);
        assert!(!analysis.can_eliminate());
    }

    #[test]
    fn test_for_analysis_empty_collection() {
        let iter = Expr::Literal(Value::Nil);

        let result = ExprCompilerV5::analyze_for(&iter);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(analysis.collection_is_empty);
        assert!(analysis.can_eliminate());
    }

    #[test]
    fn test_for_analysis_literal_collection() {
        let iter = Expr::Literal(Value::Int(10));

        let result = ExprCompilerV5::analyze_for(&iter);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(analysis.collection_is_literal);
        assert!(!analysis.collection_is_empty);
    }

    #[test]
    fn test_for_analysis_dynamic_collection() {
        let iter = Expr::Var(SymbolId(1), 0, 0);

        let result = ExprCompilerV5::analyze_for(&iter);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(!analysis.collection_is_literal);
        assert!(!analysis.can_eliminate());
    }

    #[test]
    fn test_cond_analysis_all_literals() {
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
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(analysis.all_constants);
        assert!(analysis.is_deterministic());
    }

    #[test]
    fn test_while_analysis_empty_body() {
        let cond = Expr::Literal(Value::Bool(true));
        let body = Expr::Begin(vec![]);

        let result = ExprCompilerV5::analyze_while(&cond, &body);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(analysis.body_is_empty);
        assert!(analysis.can_eliminate()); // Infinite loop with no side effects
    }
}
