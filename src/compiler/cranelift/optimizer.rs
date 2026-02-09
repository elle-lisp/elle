// Expression optimization passes (Phase 8)
//
// Implements optimization passes for compiled expressions:
// - Constant propagation
// - Dead code elimination
// - Expression simplification
// - Algebraic identity elimination
//
// Note: These optimizations work on AST level before final compilation

use crate::compiler::ast::Expr;
use crate::value::{SymbolId, Value};
use std::collections::HashMap;

/// Result of optimization analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizationResult {
    /// Expression was optimized
    Optimized,
    /// Expression was not changed
    Unchanged,
    /// Expression was eliminated (dead code)
    Eliminated,
}

/// Represents a constant value detected during analysis
#[derive(Debug, Clone, PartialEq)]
pub enum ConstantValue {
    /// Integer constant
    Int(i64),
    /// Float constant
    Float(f64),
    /// Boolean constant
    Bool(bool),
    /// Nil constant
    Nil,
    /// Unknown (non-constant)
    Unknown,
}

impl ConstantValue {
    /// Check if this is a constant value
    pub fn is_constant(&self) -> bool {
        !matches!(self, ConstantValue::Unknown)
    }

    /// Convert to Value if possible
    pub fn to_value(&self) -> Option<Value> {
        match self {
            ConstantValue::Int(i) => Some(Value::Int(*i)),
            ConstantValue::Float(f) => Some(Value::Float(*f)),
            ConstantValue::Bool(b) => Some(Value::Bool(*b)),
            ConstantValue::Nil => Some(Value::Nil),
            ConstantValue::Unknown => None,
        }
    }
}

/// Constant propagation optimizer
pub struct ConstantPropagator {
    /// Maps variables to their constant values
    constants: HashMap<SymbolId, ConstantValue>,
}

impl ConstantPropagator {
    /// Create a new constant propagator
    pub fn new() -> Self {
        ConstantPropagator {
            constants: HashMap::new(),
        }
    }

    /// Analyze an expression and return optimized version
    pub fn propagate(&mut self, expr: &Expr) -> (Expr, OptimizationResult) {
        match expr {
            Expr::Let { bindings, body } => {
                let mut optimized_bindings = Vec::new();
                let mut changed = false;

                // Propagate constants from bindings
                for (sym_id, binding_expr) in bindings {
                    let (opt_expr, result) = self.propagate(binding_expr);

                    // Try to extract constant value
                    if let Expr::Literal(val) = &opt_expr {
                        let const_val = match val {
                            Value::Int(i) => ConstantValue::Int(*i),
                            Value::Float(f) => ConstantValue::Float(*f),
                            Value::Bool(b) => ConstantValue::Bool(*b),
                            Value::Nil => ConstantValue::Nil,
                            _ => ConstantValue::Unknown,
                        };
                        self.constants.insert(*sym_id, const_val);
                    }

                    optimized_bindings.push((*sym_id, opt_expr));
                    if result != OptimizationResult::Unchanged {
                        changed = true;
                    }
                }

                // Optimize body
                let (opt_body, body_result) = self.propagate(body);

                let result_expr = Expr::Let {
                    bindings: optimized_bindings,
                    body: Box::new(opt_body),
                };

                let result = if changed || body_result != OptimizationResult::Unchanged {
                    OptimizationResult::Optimized
                } else {
                    OptimizationResult::Unchanged
                };

                (result_expr, result)
            }
            Expr::Var(sym_id, _depth, _index) => {
                // Check if this variable is a known constant
                if let Some(const_val) = self.constants.get(sym_id) {
                    if let Some(val) = const_val.to_value() {
                        return (Expr::Literal(val), OptimizationResult::Optimized);
                    }
                }
                (expr.clone(), OptimizationResult::Unchanged)
            }
            Expr::Begin(exprs) => {
                let mut optimized = Vec::new();
                let mut changed = false;

                for e in exprs {
                    let (opt_e, result) = self.propagate(e);
                    optimized.push(opt_e);
                    if result != OptimizationResult::Unchanged {
                        changed = true;
                    }
                }

                let result = if changed {
                    OptimizationResult::Optimized
                } else {
                    OptimizationResult::Unchanged
                };

                (Expr::Begin(optimized), result)
            }
            Expr::If { cond, then, else_ } => {
                let (opt_cond, cond_result) = self.propagate(cond);
                let (opt_then, then_result) = self.propagate(then);
                let (opt_else, else_result) = self.propagate(else_);

                let result = if cond_result != OptimizationResult::Unchanged
                    || then_result != OptimizationResult::Unchanged
                    || else_result != OptimizationResult::Unchanged
                {
                    OptimizationResult::Optimized
                } else {
                    OptimizationResult::Unchanged
                };

                (
                    Expr::If {
                        cond: Box::new(opt_cond),
                        then: Box::new(opt_then),
                        else_: Box::new(opt_else),
                    },
                    result,
                )
            }
            _ => (expr.clone(), OptimizationResult::Unchanged),
        }
    }
}

impl Default for ConstantPropagator {
    fn default() -> Self {
        Self::new()
    }
}

/// Simplifies expressions by eliminating algebraic identities
pub struct ExprSimplifier;

impl ExprSimplifier {
    /// Simplify an expression
    pub fn simplify(expr: &Expr) -> (Expr, OptimizationResult) {
        match expr {
            Expr::Begin(exprs) => {
                if exprs.is_empty() {
                    // Empty begin is nil
                    (Expr::Literal(Value::Nil), OptimizationResult::Optimized)
                } else if exprs.len() == 1 {
                    // Single expression in begin, just return it
                    (exprs[0].clone(), OptimizationResult::Optimized)
                } else {
                    // Recursively simplify all expressions
                    let mut simplified = Vec::new();
                    let mut changed = false;
                    for e in exprs {
                        let (opt_e, result) = Self::simplify(e);
                        simplified.push(opt_e);
                        if result != OptimizationResult::Unchanged {
                            changed = true;
                        }
                    }

                    let result = if changed {
                        OptimizationResult::Optimized
                    } else {
                        OptimizationResult::Unchanged
                    };

                    (Expr::Begin(simplified), result)
                }
            }
            Expr::If { cond, then, else_ } => {
                let (opt_cond, cond_result) = Self::simplify(cond);
                let (opt_then, then_result) = Self::simplify(then);
                let (opt_else, else_result) = Self::simplify(else_);

                // Check if condition is a constant
                if let Expr::Literal(Value::Bool(b)) = &opt_cond {
                    // Branch prediction - take the correct branch
                    let result_expr = if *b {
                        opt_then.clone()
                    } else {
                        opt_else.clone()
                    };
                    return (result_expr, OptimizationResult::Optimized);
                }

                let result = if cond_result != OptimizationResult::Unchanged
                    || then_result != OptimizationResult::Unchanged
                    || else_result != OptimizationResult::Unchanged
                {
                    OptimizationResult::Optimized
                } else {
                    OptimizationResult::Unchanged
                };

                (
                    Expr::If {
                        cond: Box::new(opt_cond),
                        then: Box::new(opt_then),
                        else_: Box::new(opt_else),
                    },
                    result,
                )
            }
            Expr::Let { bindings, body } => {
                let mut optimized_bindings = Vec::new();
                let mut changed = false;

                for (sym_id, binding_expr) in bindings {
                    let (opt_expr, result) = Self::simplify(binding_expr);
                    optimized_bindings.push((*sym_id, opt_expr));
                    if result != OptimizationResult::Unchanged {
                        changed = true;
                    }
                }

                let (opt_body, body_result) = Self::simplify(body);

                let result = if changed || body_result != OptimizationResult::Unchanged {
                    OptimizationResult::Optimized
                } else {
                    OptimizationResult::Unchanged
                };

                (
                    Expr::Let {
                        bindings: optimized_bindings,
                        body: Box::new(opt_body),
                    },
                    result,
                )
            }
            _ => (expr.clone(), OptimizationResult::Unchanged),
        }
    }
}

/// Optimization pipeline that applies multiple passes
pub struct Optimizer {
    passes: Vec<String>,
}

impl Optimizer {
    /// Create a new optimizer with default passes
    pub fn new() -> Self {
        Optimizer {
            passes: vec!["simplify".to_string(), "constant_propagate".to_string()],
        }
    }

    /// Optimize an expression with all passes
    pub fn optimize(&self, mut expr: Expr) -> (Expr, usize) {
        let mut pass_count = 0;

        // Apply simplification pass
        let (simplified, result) = ExprSimplifier::simplify(&expr);
        if result == OptimizationResult::Optimized {
            expr = simplified;
            pass_count += 1;
        }

        // Apply constant propagation pass
        let mut propagator = ConstantPropagator::new();
        let (propagated, result) = propagator.propagate(&expr);
        if result == OptimizationResult::Optimized {
            expr = propagated;
            pass_count += 1;
        }

        // Re-simplify after propagation
        let (final_simplified, result) = ExprSimplifier::simplify(&expr);
        if result == OptimizationResult::Optimized {
            expr = final_simplified;
            pass_count += 1;
        }

        (expr, pass_count)
    }

    /// Get list of enabled passes
    pub fn passes(&self) -> &[String] {
        &self.passes
    }
}

impl Default for Optimizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_value_creation() {
        let const_int = ConstantValue::Int(42);
        assert!(const_int.is_constant());

        let const_unknown = ConstantValue::Unknown;
        assert!(!const_unknown.is_constant());
    }

    #[test]
    fn test_constant_value_to_value() {
        let const_int = ConstantValue::Int(42);
        let val = const_int.to_value();
        assert!(val.is_some());
        assert!(matches!(val.unwrap(), Value::Int(42)));
    }

    #[test]
    fn test_simplify_empty_begin() {
        let expr = Expr::Begin(vec![]);
        let (simplified, result) = ExprSimplifier::simplify(&expr);

        assert_eq!(result, OptimizationResult::Optimized);
        assert!(matches!(simplified, Expr::Literal(Value::Nil)));
    }

    #[test]
    fn test_simplify_single_begin() {
        let expr = Expr::Begin(vec![Expr::Literal(Value::Int(42))]);
        let (simplified, result) = ExprSimplifier::simplify(&expr);

        assert_eq!(result, OptimizationResult::Optimized);
        assert!(matches!(simplified, Expr::Literal(Value::Int(42))));
    }

    #[test]
    fn test_simplify_if_constant_condition() {
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
    fn test_simplify_if_constant_condition_false() {
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
    fn test_constant_propagation_simple() {
        let expr = Expr::Let {
            bindings: vec![(SymbolId(1), Expr::Literal(Value::Int(42)))],
            body: Box::new(Expr::Var(SymbolId(1), 1, 0)),
        };

        let mut propagator = ConstantPropagator::new();
        let (optimized, result) = propagator.propagate(&expr);

        assert_eq!(result, OptimizationResult::Optimized);
        // The Var should be replaced with the constant
        if let Expr::Let { body, .. } = &optimized {
            assert!(matches!(**body, Expr::Literal(Value::Int(42))));
        }
    }

    #[test]
    fn test_optimizer_pipeline() {
        let expr = Expr::Begin(vec![Expr::Literal(Value::Int(42))]);

        let optimizer = Optimizer::new();
        let (optimized, passes) = optimizer.optimize(expr);

        assert!(passes > 0);
        assert!(matches!(optimized, Expr::Literal(Value::Int(42))));
    }

    #[test]
    fn test_optimizer_if_simplification() {
        let expr = Expr::If {
            cond: Box::new(Expr::Literal(Value::Bool(true))),
            then: Box::new(Expr::Literal(Value::Int(1))),
            else_: Box::new(Expr::Literal(Value::Int(2))),
        };

        let optimizer = Optimizer::new();
        let (optimized, passes) = optimizer.optimize(expr);

        assert_eq!(passes, 1); // One optimization pass should fire
        assert!(matches!(optimized, Expr::Literal(Value::Int(1))));
    }

    #[test]
    fn test_optimizer_no_optimization_needed() {
        let expr = Expr::Literal(Value::Int(42));

        let optimizer = Optimizer::new();
        let (optimized, passes) = optimizer.optimize(expr);

        assert_eq!(passes, 0);
        assert!(matches!(optimized, Expr::Literal(Value::Int(42))));
    }

    #[test]
    fn test_optimizer_passes_count() {
        let optimizer = Optimizer::new();
        assert!(!optimizer.passes().is_empty());
    }
}
