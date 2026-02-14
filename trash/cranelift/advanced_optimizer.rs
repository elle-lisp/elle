// Advanced optimization and JIT infrastructure (Phase 10)
//
// Implements advanced optimizations:
// - Tail call optimization (TCO) detection
// - Function inlining analysis
// - JIT profiling and statistics
// - Adaptive compilation heuristics
// - Hot function detection

use crate::compiler::ast::Expr;
use crate::value::SymbolId;
use std::collections::HashMap;

/// Tail call position analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailPosition {
    /// Expression is in tail position (can be TCO'd)
    Yes,
    /// Expression is not in tail position
    No,
    /// Expression is in a conditional (depends on branches)
    Conditional,
}

impl TailPosition {
    /// Check if this is a guaranteed tail position
    pub fn is_definitely_tail(&self) -> bool {
        matches!(self, TailPosition::Yes)
    }

    /// Check if this might be a tail position
    pub fn might_be_tail(&self) -> bool {
        matches!(self, TailPosition::Yes | TailPosition::Conditional)
    }
}

/// Analysis of tail call opportunities
#[derive(Debug, Clone)]
pub struct TailCallAnalysis {
    /// Whether the expression is in tail position
    pub position: TailPosition,
    /// If it's a function call, the function being called
    pub target_function: Option<SymbolId>,
    /// Whether it's a recursive call
    pub is_recursive: bool,
    /// Number of tail calls detected
    pub tail_call_count: usize,
}

impl TailCallAnalysis {
    /// Create a new tail call analysis
    pub fn new(position: TailPosition) -> Self {
        TailCallAnalysis {
            position,
            target_function: None,
            is_recursive: false,
            tail_call_count: 0,
        }
    }

    /// Check if TCO can be applied
    pub fn can_optimize(&self) -> bool {
        self.position.is_definitely_tail() && self.target_function.is_some()
    }

    /// Get optimization benefit estimate (0.0 to 1.0)
    pub fn optimization_benefit(&self) -> f64 {
        if self.is_recursive {
            0.9 // High benefit for recursive calls
        } else {
            0.6 // Moderate benefit for non-recursive
        }
    }
}

/// Inlining opportunity analysis
#[derive(Debug, Clone)]
pub struct InliningOpportunity {
    /// Whether this function is a candidate for inlining
    pub is_candidate: bool,
    /// Estimated size of the function
    pub estimated_size: usize,
    /// Call frequency in current context
    pub call_frequency: usize,
    /// Whether it's a small, simple function
    pub is_small: bool,
    /// Whether it's called frequently
    pub is_hot: bool,
}

impl InliningOpportunity {
    /// Create a new inlining opportunity
    pub fn new() -> Self {
        InliningOpportunity {
            is_candidate: false,
            estimated_size: 0,
            call_frequency: 0,
            is_small: false,
            is_hot: false,
        }
    }

    /// Check if inlining is worthwhile
    pub fn should_inline(&self) -> bool {
        self.is_small && (self.is_hot || self.call_frequency <= 3)
    }

    /// Get inlining benefit estimate
    pub fn benefit_score(&self) -> f64 {
        let size_factor = if self.estimated_size < 50 { 1.0 } else { 0.5 };
        let frequency_factor = (self.call_frequency as f64).min(10.0) / 10.0;
        size_factor * frequency_factor
    }
}

impl Default for InliningOpportunity {
    fn default() -> Self {
        Self::new()
    }
}

/// JIT compilation statistics and profiling
#[derive(Debug, Clone)]
pub struct JitStats {
    /// Total expressions compiled
    pub expressions_compiled: usize,
    /// Functions compiled
    pub functions_compiled: usize,
    /// Optimization passes run
    pub optimization_passes: usize,
    /// Dead code eliminated
    pub dead_code_eliminated: usize,
    /// Constants propagated
    pub constants_propagated: usize,
    /// Tail calls optimized
    pub tail_calls_optimized: usize,
    /// Functions inlined
    pub functions_inlined: usize,
    /// Bytes of code generated
    pub code_size: usize,
    /// Call frequency tracking
    pub call_frequencies: HashMap<SymbolId, usize>,
}

impl JitStats {
    /// Create new statistics tracker
    pub fn new() -> Self {
        JitStats {
            expressions_compiled: 0,
            functions_compiled: 0,
            optimization_passes: 0,
            dead_code_eliminated: 0,
            constants_propagated: 0,
            tail_calls_optimized: 0,
            functions_inlined: 0,
            code_size: 0,
            call_frequencies: HashMap::new(),
        }
    }

    /// Record a compiled expression
    pub fn record_expression(&mut self) {
        self.expressions_compiled += 1;
    }

    /// Record a compiled function
    pub fn record_function(&mut self) {
        self.functions_compiled += 1;
    }

    /// Record a function call
    pub fn record_call(&mut self, func: SymbolId) {
        *self.call_frequencies.entry(func).or_insert(0) += 1;
    }

    /// Get call frequency for a function
    pub fn get_call_frequency(&self, func: SymbolId) -> usize {
        self.call_frequencies.get(&func).copied().unwrap_or(0)
    }

    /// Get total optimization statistics
    pub fn total_optimizations(&self) -> usize {
        self.dead_code_eliminated
            + self.constants_propagated
            + self.tail_calls_optimized
            + self.functions_inlined
    }

    /// Get optimization ratio
    pub fn optimization_ratio(&self) -> f64 {
        if self.expressions_compiled == 0 {
            0.0
        } else {
            self.total_optimizations() as f64 / self.expressions_compiled as f64
        }
    }
}

impl Default for JitStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Advanced optimizer combining multiple strategies
pub struct AdvancedOptimizer;

impl AdvancedOptimizer {
    /// Analyze an expression for tail call opportunities
    pub fn analyze_tail_calls(expr: &Expr, current_function: Option<SymbolId>) -> TailCallAnalysis {
        let mut analysis = TailCallAnalysis::new(Self::check_tail_position(expr));

        // Check if it's a function call
        if let Expr::Call { func, .. } = expr {
            if let Expr::Literal(crate::value::Value::Symbol(sym)) = **func {
                analysis.target_function = Some(sym);
                analysis.is_recursive = current_function == Some(sym);

                // Count all tail calls in the expression
                analysis.tail_call_count = Self::count_tail_calls(expr, current_function);
            }
        }

        analysis
    }

    /// Check if an expression is in tail position
    fn check_tail_position(expr: &Expr) -> TailPosition {
        match expr {
            // Function calls at the end of a block are in tail position
            Expr::Call { .. } => TailPosition::Yes,

            // The last expression in a begin is in tail position
            Expr::Begin(exprs) if !exprs.is_empty() => {
                Self::check_tail_position(&exprs[exprs.len() - 1])
            }

            // Both branches of if/then/else are in tail position
            Expr::If { then, else_, .. } => {
                let then_tail = Self::check_tail_position(then);
                let else_tail = Self::check_tail_position(else_);
                if then_tail == TailPosition::Yes && else_tail == TailPosition::Yes {
                    TailPosition::Yes
                } else if then_tail.might_be_tail() && else_tail.might_be_tail() {
                    TailPosition::Conditional
                } else {
                    TailPosition::No
                }
            }

            // Lambda body is in tail position
            Expr::Lambda { body, .. } => Self::check_tail_position(body),

            // Let body is in tail position
            Expr::Let { body, .. } => Self::check_tail_position(body),

            // Everything else is not in tail position
            _ => TailPosition::No,
        }
    }

    /// Count tail calls in an expression
    fn count_tail_calls(expr: &Expr, current_func: Option<SymbolId>) -> usize {
        match expr {
            Expr::Call { func, .. } => {
                if let Expr::Literal(crate::value::Value::Symbol(sym)) = **func {
                    if current_func == Some(sym) {
                        return 1;
                    }
                }
                0
            }
            Expr::Begin(exprs) => {
                if let Some(last) = exprs.last() {
                    Self::count_tail_calls(last, current_func)
                } else {
                    0
                }
            }
            Expr::If { then, else_, .. } => {
                Self::count_tail_calls(then, current_func)
                    + Self::count_tail_calls(else_, current_func)
            }
            Expr::Lambda { body, .. } => Self::count_tail_calls(body, current_func),
            Expr::Let { body, .. } => Self::count_tail_calls(body, current_func),
            _ => 0,
        }
    }

    /// Analyze a function for inlining opportunities
    pub fn analyze_inlining(expr: &Expr, call_frequency: usize) -> InliningOpportunity {
        let estimated_size = Self::estimate_size(expr);
        let is_small = estimated_size < 50;

        InliningOpportunity {
            is_candidate: is_small,
            estimated_size,
            call_frequency,
            is_small,
            is_hot: call_frequency > 5,
        }
    }

    /// Estimate the size of an expression
    fn estimate_size(expr: &Expr) -> usize {
        match expr {
            Expr::Literal(_) => 1,
            Expr::Var(_, _, _) => 1,
            Expr::Begin(exprs) => exprs.iter().map(Self::estimate_size).sum::<usize>() + 1,
            Expr::If { cond, then, else_ } => {
                Self::estimate_size(cond)
                    + Self::estimate_size(then)
                    + Self::estimate_size(else_)
                    + 2
            }
            Expr::Let { bindings, body } => {
                bindings
                    .iter()
                    .map(|(_, e)| Self::estimate_size(e))
                    .sum::<usize>()
                    + Self::estimate_size(body)
                    + 1
            }
            Expr::Call { args, .. } => args.iter().map(Self::estimate_size).sum::<usize>() + 2,
            Expr::Lambda { body, .. } => Self::estimate_size(body) + 1,
            _ => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    #[test]
    fn test_tail_position_simple_call() {
        let expr = Expr::Call {
            func: Box::new(Expr::Literal(Value::Symbol(SymbolId(1)))),
            args: vec![],
            tail: false,
        };

        let result = AdvancedOptimizer::analyze_tail_calls(&expr, None);
        assert_eq!(result.position, TailPosition::Yes);
    }

    #[test]
    fn test_tail_position_begin() {
        let expr = Expr::Begin(vec![
            Expr::Literal(Value::Int(1)),
            Expr::Call {
                func: Box::new(Expr::Literal(Value::Symbol(SymbolId(1)))),
                args: vec![],
                tail: false,
            },
        ]);

        let result = AdvancedOptimizer::analyze_tail_calls(&expr, None);
        assert_eq!(result.position, TailPosition::Yes);
    }

    #[test]
    fn test_tail_position_not_tail() {
        let expr = Expr::Literal(Value::Int(42));

        let result = AdvancedOptimizer::analyze_tail_calls(&expr, None);
        assert_eq!(result.position, TailPosition::No);
    }

    #[test]
    fn test_recursive_tail_call() {
        let func_id = SymbolId(1);
        let expr = Expr::Call {
            func: Box::new(Expr::Literal(Value::Symbol(func_id))),
            args: vec![],
            tail: false,
        };

        let result = AdvancedOptimizer::analyze_tail_calls(&expr, Some(func_id));
        assert!(result.is_recursive);
        assert!(result.can_optimize());
    }

    #[test]
    fn test_inlining_small_function() {
        let expr = Expr::Literal(Value::Int(42));

        let opportunity = AdvancedOptimizer::analyze_inlining(&expr, 10);
        assert!(opportunity.is_small);
        assert!(opportunity.should_inline());
    }

    #[test]
    fn test_inlining_large_function() {
        let mut exprs = vec![];
        for _ in 0..100 {
            exprs.push(Expr::Literal(Value::Int(1)));
        }
        let expr = Expr::Begin(exprs);

        let opportunity = AdvancedOptimizer::analyze_inlining(&expr, 1);
        assert!(!opportunity.is_small);
        assert!(!opportunity.should_inline());
    }

    #[test]
    fn test_jit_stats_creation() {
        let stats = JitStats::new();
        assert_eq!(stats.expressions_compiled, 0);
        assert_eq!(stats.functions_compiled, 0);
    }

    #[test]
    fn test_jit_stats_recording() {
        let mut stats = JitStats::new();
        stats.record_expression();
        stats.record_function();
        stats.record_call(SymbolId(1));
        stats.record_call(SymbolId(1));

        assert_eq!(stats.expressions_compiled, 1);
        assert_eq!(stats.functions_compiled, 1);
        assert_eq!(stats.get_call_frequency(SymbolId(1)), 2);
    }

    #[test]
    fn test_optimization_ratio() {
        let mut stats = JitStats::new();
        stats.record_expression();
        stats.record_expression();
        stats.record_expression();
        stats.dead_code_eliminated = 1;
        stats.constants_propagated = 1;

        let ratio = stats.optimization_ratio();
        assert!(ratio > 0.0);
        assert!(ratio <= 1.0);
    }

    #[test]
    fn test_estimate_size() {
        let simple = Expr::Literal(Value::Int(42));
        assert_eq!(AdvancedOptimizer::estimate_size(&simple), 1);

        let call = Expr::Call {
            func: Box::new(Expr::Literal(Value::Symbol(SymbolId(1)))),
            args: vec![Expr::Literal(Value::Int(1)), Expr::Literal(Value::Int(2))],
            tail: false,
        };
        assert!(AdvancedOptimizer::estimate_size(&call) > 2);
    }

    #[test]
    fn test_tail_call_if_expression() {
        let expr = Expr::If {
            cond: Box::new(Expr::Literal(Value::Bool(true))),
            then: Box::new(Expr::Call {
                func: Box::new(Expr::Literal(Value::Symbol(SymbolId(1)))),
                args: vec![],
                tail: false,
            }),
            else_: Box::new(Expr::Call {
                func: Box::new(Expr::Literal(Value::Symbol(SymbolId(2)))),
                args: vec![],
                tail: false,
            }),
        };

        let result = AdvancedOptimizer::analyze_tail_calls(&expr, None);
        assert!(result.position.might_be_tail());
    }

    #[test]
    fn test_inlining_benefit_score() {
        let small_hot = InliningOpportunity {
            is_candidate: true,
            estimated_size: 20,
            call_frequency: 10,
            is_small: true,
            is_hot: true,
        };

        let score = small_hot.benefit_score();
        assert!(score > 0.5);
    }
}
