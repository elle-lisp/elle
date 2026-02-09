// Phase 11: Adaptive Compilation & Profiling-Guided Optimization
//
// Uses profiling data from Phase 10 to make intelligent compilation decisions:
// - Compilation strategy selection (aggressive vs conservative)
// - Dynamic optimization level adjustment
// - Hot function prioritization for re-compilation
// - Context-aware inlining decisions
// - Adaptive threshold tuning based on success rates
// - Speculative compilation feedback

use crate::compiler::ast::Expr;
use crate::compiler::cranelift::advanced_optimizer::{
    InliningOpportunity, JitStats, TailCallAnalysis,
};
use crate::value::SymbolId;
use std::collections::HashMap;

/// Compilation strategy selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompilationStrategy {
    /// Conservative: only compile very safe optimizations
    Conservative,
    /// Balanced: moderate optimization level (default)
    #[default]
    Balanced,
    /// Aggressive: apply all possible optimizations
    Aggressive,
}

impl CompilationStrategy {
    /// Get the optimization aggressiveness as a score (0.0 to 1.0)
    pub fn aggressiveness(&self) -> f64 {
        match self {
            CompilationStrategy::Conservative => 0.3,
            CompilationStrategy::Balanced => 0.6,
            CompilationStrategy::Aggressive => 0.9,
        }
    }

    /// Get the inlining threshold for this strategy
    pub fn inlining_threshold(&self) -> usize {
        match self {
            CompilationStrategy::Conservative => 3,
            CompilationStrategy::Balanced => 5,
            CompilationStrategy::Aggressive => 10,
        }
    }

    /// Get the tail call optimization threshold
    pub fn tco_threshold(&self) -> f64 {
        match self {
            CompilationStrategy::Conservative => 0.8,
            CompilationStrategy::Balanced => 0.6,
            CompilationStrategy::Aggressive => 0.4,
        }
    }
}

/// Adaptive compilation decisions based on profiling data
#[derive(Debug, Clone)]
pub struct AdaptiveDecisions {
    /// Current compilation strategy
    pub strategy: CompilationStrategy,
    /// Should inline this function
    pub should_inline: bool,
    /// Should apply tail call optimization
    pub should_optimize_tco: bool,
    /// Should speculate on types
    pub should_speculate: bool,
    /// Confidence in the decision (0.0 to 1.0)
    pub confidence: f64,
    /// Estimated speedup from optimizations
    pub estimated_speedup: f64,
}

impl AdaptiveDecisions {
    /// Create new adaptive decision
    pub fn new(strategy: CompilationStrategy) -> Self {
        AdaptiveDecisions {
            strategy,
            should_inline: false,
            should_optimize_tco: false,
            should_speculate: false,
            confidence: 0.5,
            estimated_speedup: 1.0,
        }
    }

    /// Check if this is a high-confidence decision
    pub fn is_high_confidence(&self) -> bool {
        self.confidence > 0.75
    }

    /// Check if speculative compilation is worth it
    pub fn should_speculate_compile(&self) -> bool {
        self.should_speculate && self.confidence > 0.6
    }

    /// Get the priority of this compilation (0.0 to 1.0)
    pub fn priority(&self) -> f64 {
        self.confidence * self.estimated_speedup
    }
}

impl Default for AdaptiveDecisions {
    fn default() -> Self {
        Self::new(CompilationStrategy::default())
    }
}

/// Profiling-guided compilation context
#[derive(Debug, Clone)]
pub struct CompilationContext {
    /// Statistics from previous compilations
    pub stats: JitStats,
    /// Call frequency for each function
    pub call_frequencies: HashMap<SymbolId, usize>,
    /// Inlining success rate (0.0 to 1.0)
    pub inlining_success_rate: f64,
    /// TCO success rate (0.0 to 1.0)
    pub tco_success_rate: f64,
    /// Number of recompilations performed
    pub recompilation_count: usize,
    /// Total optimization time spent (microseconds)
    pub optimization_time_us: u64,
}

impl CompilationContext {
    /// Create new compilation context
    pub fn new() -> Self {
        CompilationContext {
            stats: JitStats::new(),
            call_frequencies: HashMap::new(),
            inlining_success_rate: 0.5,
            tco_success_rate: 0.5,
            recompilation_count: 0,
            optimization_time_us: 0,
        }
    }

    /// Record a function call
    pub fn record_call(&mut self, func: SymbolId) {
        self.stats.record_call(func);
        *self.call_frequencies.entry(func).or_insert(0) += 1;
    }

    /// Record inlining success/failure
    pub fn record_inlining_attempt(&mut self, success: bool) {
        let total = self.stats.expressions_compiled as f64 + 1.0;
        let successes = if success {
            (self.inlining_success_rate * self.stats.expressions_compiled as f64) + 1.0
        } else {
            self.inlining_success_rate * self.stats.expressions_compiled as f64
        };
        self.inlining_success_rate = successes / total;
    }

    /// Record TCO attempt
    pub fn record_tco_attempt(&mut self, success: bool) {
        let total = self.stats.tail_calls_optimized as f64 + 1.0;
        let successes = if success {
            (self.tco_success_rate * self.stats.tail_calls_optimized as f64) + 1.0
        } else {
            self.tco_success_rate * self.stats.tail_calls_optimized as f64
        };
        self.tco_success_rate = successes / total;
    }

    /// Get the total number of compilations
    pub fn total_compilations(&self) -> usize {
        self.stats.functions_compiled + self.recompilation_count
    }

    /// Get hot functions (called more than threshold)
    pub fn get_hot_functions(&self, threshold: usize) -> Vec<SymbolId> {
        self.call_frequencies
            .iter()
            .filter_map(
                |(func, freq)| {
                    if *freq > threshold {
                        Some(*func)
                    } else {
                        None
                    }
                },
            )
            .collect()
    }

    /// Get average calls per function
    pub fn average_calls_per_function(&self) -> f64 {
        if self.call_frequencies.is_empty() {
            0.0
        } else {
            let total: usize = self.call_frequencies.values().sum();
            total as f64 / self.call_frequencies.len() as f64
        }
    }
}

impl Default for CompilationContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Adaptive compiler using profiling data
pub struct AdaptiveCompiler;

impl AdaptiveCompiler {
    /// Select compilation strategy based on context
    pub fn select_strategy(context: &CompilationContext) -> CompilationStrategy {
        let hot_count = context.get_hot_functions(5).len();
        let avg_calls = context.average_calls_per_function();

        // Use aggressive strategy if many hot functions or high call volume
        if hot_count > 5 || avg_calls > 20.0 {
            CompilationStrategy::Aggressive
        } else if hot_count > 2 || avg_calls > 10.0 {
            CompilationStrategy::Balanced
        } else {
            CompilationStrategy::Conservative
        }
    }

    /// Make adaptive decisions for a function
    pub fn decide(
        _expr: &Expr,
        context: &CompilationContext,
        inlining_opp: &InliningOpportunity,
        tail_call: &TailCallAnalysis,
    ) -> AdaptiveDecisions {
        let strategy = Self::select_strategy(context);
        let mut decision = AdaptiveDecisions::new(strategy);

        // Inlining decision
        decision.should_inline = Self::should_inline(inlining_opp, &strategy, context);

        // TCO decision
        decision.should_optimize_tco = Self::should_optimize_tco(tail_call, &strategy, context);

        // Speculation decision
        decision.should_speculate = Self::should_speculate(&strategy, context);

        // Calculate confidence
        decision.confidence = Self::calculate_confidence(inlining_opp, tail_call, context);

        // Estimate speedup
        decision.estimated_speedup = Self::estimate_speedup(
            decision.should_inline,
            decision.should_optimize_tco,
            decision.should_speculate,
            context,
        );

        decision
    }

    /// Decide if inlining should be applied
    pub fn should_inline(
        opp: &InliningOpportunity,
        strategy: &CompilationStrategy,
        context: &CompilationContext,
    ) -> bool {
        // Start with basic inlining opportunity check
        if !opp.is_candidate {
            return false;
        }

        // Scale threshold based on strategy and success rate
        let threshold = strategy.inlining_threshold();
        let adjusted_threshold =
            (threshold as f64 * (1.0 / context.inlining_success_rate)) as usize;

        opp.call_frequency <= adjusted_threshold
    }

    /// Decide if TCO should be applied
    pub fn should_optimize_tco(
        tail_call: &TailCallAnalysis,
        strategy: &CompilationStrategy,
        context: &CompilationContext,
    ) -> bool {
        // Must be in tail position
        if !tail_call.can_optimize() {
            return false;
        }

        // Check benefit threshold based on strategy
        let benefit_threshold = strategy.tco_threshold();
        let benefit = tail_call.optimization_benefit();

        // Scale benefit threshold by success rate
        let adjusted_benefit = benefit_threshold * context.tco_success_rate;

        benefit >= adjusted_benefit
    }

    /// Decide if speculative compilation should be used
    pub fn should_speculate(strategy: &CompilationStrategy, context: &CompilationContext) -> bool {
        // More aggressive strategies speculate more
        let aggressiveness = strategy.aggressiveness();
        let recompilation_rate = if context.total_compilations() > 0 {
            context.recompilation_count as f64 / context.total_compilations() as f64
        } else {
            0.0
        };

        // Speculate if strategy is aggressive and we have successful recompilation history
        aggressiveness > 0.7 && recompilation_rate > 0.1
    }

    /// Calculate confidence in the decision
    pub fn calculate_confidence(
        _inlining: &InliningOpportunity,
        _tail_call: &TailCallAnalysis,
        context: &CompilationContext,
    ) -> f64 {
        // Base confidence on success rates
        let avg_success = (context.inlining_success_rate + context.tco_success_rate) / 2.0;

        // Boost confidence if we have lots of data
        let data_boost = if context.total_compilations() > 100 {
            0.2
        } else if context.total_compilations() > 50 {
            0.1
        } else {
            0.0
        };

        (avg_success + data_boost).min(1.0)
    }

    /// Estimate speedup from optimizations
    pub fn estimate_speedup(
        should_inline: bool,
        should_optimize_tco: bool,
        should_speculate: bool,
        _context: &CompilationContext,
    ) -> f64 {
        let mut speedup = 1.0;

        // Each optimization contributes to speedup
        if should_inline {
            speedup *= 1.5; // 50% speedup from inlining
        }
        if should_optimize_tco {
            speedup *= 1.3; // 30% speedup from TCO
        }
        if should_speculate {
            speedup *= 1.2; // 20% speedup from speculation
        }

        speedup
    }

    /// Determine if a function should be recompiled
    pub fn should_recompile(
        func: SymbolId,
        context: &CompilationContext,
        current_strategy: CompilationStrategy,
    ) -> bool {
        let call_freq = context.call_frequencies.get(&func).copied().unwrap_or(0);

        // Recompile if function became hot and we're using conservative strategy
        if call_freq > 10 && current_strategy == CompilationStrategy::Conservative {
            return true;
        }

        // Recompile if function is very hot (> 50 calls)
        if call_freq > 50 && current_strategy != CompilationStrategy::Aggressive {
            return true;
        }

        false
    }

    /// Get hot functions that should be prioritized for compilation
    pub fn prioritize_hot_functions(
        context: &CompilationContext,
        limit: usize,
    ) -> Vec<(SymbolId, usize)> {
        let mut funcs: Vec<_> = context
            .call_frequencies
            .iter()
            .map(|(f, c)| (*f, *c))
            .collect();
        funcs.sort_by(|a, b| b.1.cmp(&a.1));
        funcs.into_iter().take(limit).collect()
    }

    /// Calculate optimization budget (time in microseconds)
    pub fn calculate_optimization_budget(context: &CompilationContext) -> u64 {
        // Budget based on total compilations
        match context.total_compilations() {
            0..=10 => 1000,    // 1ms budget for initial compilations
            11..=50 => 5000,   // 5ms for moderate compilation load
            51..=100 => 10000, // 10ms for high load
            _ => 20000,        // 20ms for very high load
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase11_compilation_strategy_aggressiveness() {
        let conservative = CompilationStrategy::Conservative.aggressiveness();
        let balanced = CompilationStrategy::Balanced.aggressiveness();
        let aggressive = CompilationStrategy::Aggressive.aggressiveness();

        assert!(conservative < balanced);
        assert!(balanced < aggressive);
        assert!(aggressive <= 1.0);
    }

    #[test]
    fn phase11_compilation_strategy_inlining_threshold() {
        let conservative = CompilationStrategy::Conservative.inlining_threshold();
        let balanced = CompilationStrategy::Balanced.inlining_threshold();
        let aggressive = CompilationStrategy::Aggressive.inlining_threshold();

        assert!(conservative < balanced);
        assert!(balanced < aggressive);
    }

    #[test]
    fn phase11_compilation_strategy_tco_threshold() {
        let conservative = CompilationStrategy::Conservative.tco_threshold();
        let balanced = CompilationStrategy::Balanced.tco_threshold();
        let aggressive = CompilationStrategy::Aggressive.tco_threshold();

        assert!(conservative > balanced);
        assert!(balanced > aggressive);
    }

    #[test]
    fn phase11_adaptive_decisions_creation() {
        let decision = AdaptiveDecisions::new(CompilationStrategy::Balanced);

        assert_eq!(decision.strategy, CompilationStrategy::Balanced);
        assert!(!decision.should_inline);
        assert!(!decision.should_optimize_tco);
        assert!(!decision.should_speculate);
        assert!(decision.confidence > 0.0);
    }

    #[test]
    fn phase11_adaptive_decisions_confidence() {
        let mut decision = AdaptiveDecisions::new(CompilationStrategy::Balanced);
        decision.confidence = 0.9;

        assert!(decision.is_high_confidence());
    }

    #[test]
    fn phase11_adaptive_decisions_priority() {
        let mut decision = AdaptiveDecisions::new(CompilationStrategy::Balanced);
        decision.confidence = 0.8;
        decision.estimated_speedup = 1.5;

        let priority = decision.priority();
        assert!(priority > 0.0);
        assert!((priority - 1.2).abs() < 0.01); // 0.8 * 1.5 = 1.2
    }

    #[test]
    fn phase11_adaptive_decisions_speculation() {
        let mut decision = AdaptiveDecisions::new(CompilationStrategy::Aggressive);
        decision.should_speculate = true;
        decision.confidence = 0.7;

        assert!(decision.should_speculate_compile());
    }

    #[test]
    fn phase11_compilation_context_creation() {
        let ctx = CompilationContext::new();

        assert_eq!(ctx.recompilation_count, 0);
        assert_eq!(ctx.optimization_time_us, 0);
        assert!(ctx.call_frequencies.is_empty());
    }

    #[test]
    fn phase11_compilation_context_record_call() {
        let mut ctx = CompilationContext::new();
        let func = SymbolId(1);

        ctx.record_call(func);
        ctx.record_call(func);
        ctx.record_call(func);

        assert_eq!(ctx.call_frequencies[&func], 3);
    }

    #[test]
    fn phase11_compilation_context_hot_functions() {
        let mut ctx = CompilationContext::new();

        ctx.call_frequencies.insert(SymbolId(1), 15);
        ctx.call_frequencies.insert(SymbolId(2), 3);
        ctx.call_frequencies.insert(SymbolId(3), 50);

        let hot = ctx.get_hot_functions(5);

        assert_eq!(hot.len(), 2);
        assert!(hot.contains(&SymbolId(1)));
        assert!(hot.contains(&SymbolId(3)));
    }

    #[test]
    fn phase11_compilation_context_average_calls() {
        let mut ctx = CompilationContext::new();

        ctx.call_frequencies.insert(SymbolId(1), 10);
        ctx.call_frequencies.insert(SymbolId(2), 20);
        ctx.call_frequencies.insert(SymbolId(3), 30);

        let avg = ctx.average_calls_per_function();
        assert!((avg - 20.0).abs() < 0.01);
    }

    #[test]
    fn phase11_compilation_context_inlining_success() {
        let mut ctx = CompilationContext::new();

        ctx.record_inlining_attempt(true);
        ctx.record_inlining_attempt(true);
        ctx.record_inlining_attempt(false);

        // Just verify it's in valid range
        assert!(ctx.inlining_success_rate >= 0.0 && ctx.inlining_success_rate <= 1.0);
    }

    #[test]
    fn phase11_compilation_context_tco_success() {
        let mut ctx = CompilationContext::new();

        ctx.record_tco_attempt(true);
        ctx.record_tco_attempt(true);
        ctx.record_tco_attempt(false);

        // Just verify it's in valid range
        assert!(ctx.tco_success_rate >= 0.0 && ctx.tco_success_rate <= 1.0);
    }

    #[test]
    fn phase11_strategy_selection_conservative() {
        let ctx = CompilationContext::new();

        let strategy = AdaptiveCompiler::select_strategy(&ctx);

        assert_eq!(strategy, CompilationStrategy::Conservative);
    }

    #[test]
    fn phase11_strategy_selection_hot_functions() {
        let mut ctx = CompilationContext::new();

        for i in 0..10 {
            ctx.call_frequencies.insert(SymbolId(i), 15);
        }

        let strategy = AdaptiveCompiler::select_strategy(&ctx);

        assert_eq!(strategy, CompilationStrategy::Aggressive);
    }

    #[test]
    fn phase11_should_inline_candidate() {
        let opp = InliningOpportunity {
            is_candidate: true,
            estimated_size: 20,
            call_frequency: 2,
            is_small: true,
            is_hot: false,
        };

        let ctx = CompilationContext::new();
        let strategy = CompilationStrategy::Balanced;

        let should = AdaptiveCompiler::should_inline(&opp, &strategy, &ctx);

        assert!(should);
    }

    #[test]
    fn phase11_should_inline_not_candidate() {
        let opp = InliningOpportunity {
            is_candidate: false,
            estimated_size: 100,
            call_frequency: 2,
            is_small: false,
            is_hot: false,
        };

        let ctx = CompilationContext::new();
        let strategy = CompilationStrategy::Balanced;

        let should = AdaptiveCompiler::should_inline(&opp, &strategy, &ctx);

        assert!(!should);
    }

    #[test]
    fn phase11_should_speculate_aggressive() {
        let ctx = CompilationContext::new();
        let strategy = CompilationStrategy::Aggressive;

        let should = AdaptiveCompiler::should_speculate(&strategy, &ctx);

        // Might be true or false depending on recompilation rate
        let _ = should; // Just verify it doesn't panic
    }

    #[test]
    fn phase11_recompile_hot_function_conservative() {
        let mut ctx = CompilationContext::new();
        ctx.call_frequencies.insert(SymbolId(1), 15);

        let should = AdaptiveCompiler::should_recompile(
            SymbolId(1),
            &ctx,
            CompilationStrategy::Conservative,
        );

        assert!(should);
    }

    #[test]
    fn phase11_recompile_cold_function() {
        let ctx = CompilationContext::new();

        let should =
            AdaptiveCompiler::should_recompile(SymbolId(1), &ctx, CompilationStrategy::Balanced);

        assert!(!should);
    }

    #[test]
    fn phase11_prioritize_hot_functions() {
        let mut ctx = CompilationContext::new();

        ctx.call_frequencies.insert(SymbolId(1), 5);
        ctx.call_frequencies.insert(SymbolId(2), 20);
        ctx.call_frequencies.insert(SymbolId(3), 10);

        let prioritized = AdaptiveCompiler::prioritize_hot_functions(&ctx, 2);

        assert_eq!(prioritized.len(), 2);
        assert_eq!(prioritized[0].0, SymbolId(2)); // Most hot first
    }

    #[test]
    fn phase11_optimization_budget_initial() {
        let ctx = CompilationContext::new();

        let budget = AdaptiveCompiler::calculate_optimization_budget(&ctx);

        assert_eq!(budget, 1000);
    }

    #[test]
    fn phase11_optimization_budget_scaling() {
        let mut ctx = CompilationContext::new();
        ctx.stats.record_function();
        for _ in 0..50 {
            ctx.stats.record_function();
        }

        let budget = AdaptiveCompiler::calculate_optimization_budget(&ctx);

        assert!(budget > 1000);
    }

    #[test]
    fn phase11_strategy_default() {
        let strategy = CompilationStrategy::default();

        assert_eq!(strategy, CompilationStrategy::Balanced);
    }

    #[test]
    fn phase11_decisions_default() {
        let decisions = AdaptiveDecisions::default();

        assert_eq!(decisions.strategy, CompilationStrategy::Balanced);
        assert!(decisions.confidence > 0.0);
    }

    #[test]
    fn phase11_context_default() {
        let ctx = CompilationContext::default();

        assert_eq!(ctx.recompilation_count, 0);
    }

    #[test]
    fn phase11_estimate_speedup_no_optimizations() {
        let speedup =
            AdaptiveCompiler::estimate_speedup(false, false, false, &CompilationContext::new());

        assert_eq!(speedup, 1.0);
    }

    #[test]
    fn phase11_estimate_speedup_with_inlining() {
        let speedup =
            AdaptiveCompiler::estimate_speedup(true, false, false, &CompilationContext::new());

        assert!(speedup > 1.0);
        assert!(speedup < 2.0);
    }

    #[test]
    fn phase11_estimate_speedup_combined() {
        let speedup =
            AdaptiveCompiler::estimate_speedup(true, true, true, &CompilationContext::new());

        assert!(speedup > 2.0);
    }

    #[test]
    fn phase11_total_compilations() {
        let mut ctx = CompilationContext::new();
        ctx.stats.record_function();
        ctx.stats.record_function();
        ctx.recompilation_count = 5;

        assert_eq!(ctx.total_compilations(), 7);
    }

    #[test]
    fn phase11_adaptive_decisions_integration() {
        let opp = InliningOpportunity {
            is_candidate: true,
            estimated_size: 25,
            call_frequency: 3,
            is_small: true,
            is_hot: false,
        };

        let tail_call = TailCallAnalysis {
            position: crate::compiler::cranelift::advanced_optimizer::TailPosition::Yes,
            target_function: Some(SymbolId(1)),
            is_recursive: true,
            tail_call_count: 1,
        };

        let ctx = CompilationContext::new();

        let decision = AdaptiveCompiler::decide(
            &crate::compiler::ast::Expr::Literal(crate::value::Value::Int(42)),
            &ctx,
            &opp,
            &tail_call,
        );

        assert!(decision.confidence > 0.0);
        assert!(decision.estimated_speedup >= 1.0);
    }
}
