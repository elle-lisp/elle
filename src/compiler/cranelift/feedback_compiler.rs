// Phase 13: Feedback-Based Recompilation & Integration
//
// Integrates profiling feedback with adaptive compilation:
// - Recompilation feedback tracking
// - Profiling-to-compilation decision pipeline
// - Recompilation triggers and management
// - Performance improvement tracking
// - Compilation history and statistics
// - Feedback loop closure

use crate::compiler::cranelift::adaptive_compiler::{CompilationContext, CompilationStrategy};
use crate::compiler::cranelift::profiler::RuntimeProfiler;
use crate::value::SymbolId;
use std::collections::HashMap;

/// Compilation feedback from profiling data
#[derive(Debug, Clone)]
pub struct CompilationFeedback {
    /// Function being compiled
    pub func: SymbolId,
    /// Call count for this function
    pub call_count: usize,
    /// Previous compilation strategy used
    pub previous_strategy: CompilationStrategy,
    /// Recommended new strategy based on profiling
    pub recommended_strategy: CompilationStrategy,
    /// Performance improvement potential (0.0 to 1.0)
    pub improvement_potential: f64,
    /// Whether recompilation is recommended
    pub should_recompile: bool,
    /// Confidence in recommendation (0.0 to 1.0)
    pub confidence: f64,
}

impl CompilationFeedback {
    /// Create new feedback
    pub fn new(func: SymbolId, call_count: usize, previous_strategy: CompilationStrategy) -> Self {
        let recommended_strategy = if call_count > 50 {
            CompilationStrategy::Aggressive
        } else if call_count > 10 {
            CompilationStrategy::Balanced
        } else {
            CompilationStrategy::Conservative
        };

        let should_recompile = recommended_strategy != previous_strategy && call_count > 5;
        let improvement_potential =
            Self::estimate_improvement(previous_strategy, recommended_strategy);

        CompilationFeedback {
            func,
            call_count,
            previous_strategy,
            recommended_strategy,
            improvement_potential,
            should_recompile,
            confidence: 0.5,
        }
    }

    /// Estimate performance improvement from strategy change
    pub fn estimate_improvement(from: CompilationStrategy, to: CompilationStrategy) -> f64 {
        match (from, to) {
            (CompilationStrategy::Conservative, CompilationStrategy::Balanced) => 0.15,
            (CompilationStrategy::Conservative, CompilationStrategy::Aggressive) => 0.35,
            (CompilationStrategy::Balanced, CompilationStrategy::Aggressive) => 0.20,
            (CompilationStrategy::Aggressive, CompilationStrategy::Balanced) => -0.05,
            (CompilationStrategy::Aggressive, CompilationStrategy::Conservative) => -0.15,
            (CompilationStrategy::Balanced, CompilationStrategy::Conservative) => -0.10,
            _ => 0.0,
        }
    }

    /// Check if feedback is high-quality (high confidence)
    pub fn is_high_quality(&self) -> bool {
        self.confidence > 0.75 && self.improvement_potential.abs() > 0.1
    }
}

/// Recompilation decision with full context
#[derive(Debug, Clone)]
pub struct RecompilationDecision {
    /// Function to recompile
    pub func: SymbolId,
    /// New compilation strategy
    pub new_strategy: CompilationStrategy,
    /// Priority (0.0 to 1.0, higher = more important)
    pub priority: f64,
    /// Reason for recompilation
    pub reason: RecompilationReason,
    /// Expected performance improvement
    pub expected_improvement: f64,
    /// Whether this was executed
    pub executed: bool,
}

/// Reasons for recompilation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecompilationReason {
    /// Function became hot (call count increased significantly)
    BecameHot,
    /// Function is very hot (call count > 50)
    VeryHot,
    /// Call pattern changed
    CallPatternChanged,
    /// Strategy mismatch (current strategy too conservative)
    StrategyMismatch,
    /// Optimization opportunity detected
    OptimizationOpportunity,
    /// Performance regression suspected
    PerformanceRegression,
}

/// Feedback-based compilation pipeline
pub struct FeedbackCompiler {
    /// Profiler instance
    profiler: RuntimeProfiler,
    /// Compilation context
    context: CompilationContext,
    /// Recompilation history
    recompilation_history: Vec<RecompilationDecision>,
    /// Function compilation strategies (func -> strategy)
    function_strategies: HashMap<SymbolId, CompilationStrategy>,
    /// Recompilation count per function
    recompilation_counts: HashMap<SymbolId, usize>,
}

impl FeedbackCompiler {
    /// Create new feedback compiler
    pub fn new(profiler: RuntimeProfiler) -> Self {
        FeedbackCompiler {
            profiler,
            context: CompilationContext::new(),
            recompilation_history: Vec::new(),
            function_strategies: HashMap::new(),
            recompilation_counts: HashMap::new(),
        }
    }

    /// Register initial compilation of a function
    pub fn register_compilation(&mut self, func: SymbolId, strategy: CompilationStrategy) {
        self.function_strategies.insert(func, strategy);
        self.recompilation_counts.insert(func, 0);
    }

    /// Analyze profiling data and generate feedback
    pub fn analyze_feedback(&self) -> Vec<CompilationFeedback> {
        let hot_functions = self.profiler.get_hot_functions(5);
        let mut feedbacks = Vec::new();

        for (func, call_count) in hot_functions {
            let previous_strategy = self
                .function_strategies
                .get(&func)
                .copied()
                .unwrap_or(CompilationStrategy::Conservative);

            let feedback = CompilationFeedback::new(func, call_count, previous_strategy);
            feedbacks.push(feedback);
        }

        feedbacks
    }

    /// Make recompilation decision based on feedback
    pub fn make_recompilation_decision(
        feedback: &CompilationFeedback,
        recompile_count: usize,
    ) -> RecompilationDecision {
        let reason = if feedback.call_count > 50 {
            RecompilationReason::VeryHot
        } else if feedback.call_count > 20 {
            RecompilationReason::BecameHot
        } else {
            RecompilationReason::StrategyMismatch
        };

        let priority = if recompile_count >= 2 {
            0.3 // Diminishing returns on multiple recompilations
        } else if feedback.call_count > 50 {
            0.95
        } else if feedback.call_count > 20 {
            0.75
        } else {
            0.5
        };

        RecompilationDecision {
            func: feedback.func,
            new_strategy: feedback.recommended_strategy,
            priority,
            reason,
            expected_improvement: feedback.improvement_potential,
            executed: false,
        }
    }

    /// Execute a recompilation decision
    pub fn execute_recompilation(&mut self, mut decision: RecompilationDecision) -> bool {
        let recompile_count = self.recompilation_counts.entry(decision.func).or_insert(0);

        // Limit recompilations to 3 per function
        if *recompile_count >= 3 {
            return false;
        }

        decision.executed = true;
        self.function_strategies
            .insert(decision.func, decision.new_strategy);
        *recompile_count += 1;

        self.context.recompilation_count += 1;
        self.recompilation_history.push(decision);

        true
    }

    /// Get all pending recompilation decisions
    pub fn get_pending_recompilations(&self) -> Vec<RecompilationDecision> {
        let feedbacks = self.analyze_feedback();
        let mut decisions = Vec::new();

        for feedback in feedbacks {
            if feedback.should_recompile {
                let recompile_count = self
                    .recompilation_counts
                    .get(&feedback.func)
                    .copied()
                    .unwrap_or(0);

                let decision = Self::make_recompilation_decision(&feedback, recompile_count);
                decisions.push(decision);
            }
        }

        // Sort by priority (highest first)
        decisions.sort_by(|a, b| {
            b.priority
                .partial_cmp(&a.priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        decisions
    }

    /// Get recompilation history
    pub fn get_recompilation_history(&self) -> &[RecompilationDecision] {
        &self.recompilation_history
    }

    /// Get recompilation count for a function
    pub fn get_recompilation_count(&self, func: SymbolId) -> usize {
        self.recompilation_counts.get(&func).copied().unwrap_or(0)
    }

    /// Check if function has been recompiled
    pub fn was_recompiled(&self, func: SymbolId) -> bool {
        self.recompilation_counts
            .get(&func)
            .map(|c| *c > 0)
            .unwrap_or(false)
    }

    /// Get compilation statistics
    pub fn get_stats(&self) -> CompilationStats {
        let profiling_summary = self.profiler.summary();
        let total_recompilations: usize = self.recompilation_counts.values().sum();
        let executed_recompilations = self
            .recompilation_history
            .iter()
            .filter(|d| d.executed)
            .count();

        let total_improvement: f64 = self
            .recompilation_history
            .iter()
            .filter(|d| d.executed)
            .map(|d| d.expected_improvement)
            .sum();

        CompilationStats {
            total_compilations: profiling_summary.total_functions_compiled,
            total_recompilations,
            executed_recompilations,
            expected_total_improvement: total_improvement,
            hot_function_count: profiling_summary.hot_functions.len(),
            code_size: profiling_summary.code_size,
        }
    }

    /// Trigger feedback analysis and recompilation cycle
    pub fn trigger_feedback_cycle(&mut self) -> FeedbackCycleResult {
        let feedbacks = self.analyze_feedback();
        let feedbacks_count = feedbacks.len();
        let mut decisions = Vec::new();
        let mut executed_count = 0;

        for feedback in feedbacks {
            if feedback.should_recompile {
                let recompile_count = self
                    .recompilation_counts
                    .get(&feedback.func)
                    .copied()
                    .unwrap_or(0);

                let decision = Self::make_recompilation_decision(&feedback, recompile_count);

                if self.execute_recompilation(decision.clone()) {
                    executed_count += 1;
                }
                decisions.push(decision);
            }
        }

        FeedbackCycleResult {
            feedbacks_generated: feedbacks_count,
            decisions_made: decisions.len(),
            decisions_executed: executed_count,
            total_expected_improvement: decisions
                .iter()
                .filter(|d| d.executed)
                .map(|d| d.expected_improvement)
                .sum(),
        }
    }

    /// Get profiler reference
    pub fn profiler(&self) -> &RuntimeProfiler {
        &self.profiler
    }

    /// Get mutable profiler reference
    pub fn profiler_mut(&mut self) -> &mut RuntimeProfiler {
        &mut self.profiler
    }

    /// Get compilation context
    pub fn context(&self) -> &CompilationContext {
        &self.context
    }
}

/// Compilation statistics
#[derive(Debug, Clone)]
pub struct CompilationStats {
    /// Total functions compiled
    pub total_compilations: usize,
    /// Total recompilations attempted
    pub total_recompilations: usize,
    /// Recompilations actually executed
    pub executed_recompilations: usize,
    /// Expected total performance improvement
    pub expected_total_improvement: f64,
    /// Number of hot functions
    pub hot_function_count: usize,
    /// Total code size generated
    pub code_size: usize,
}

impl CompilationStats {
    /// Get recompilation success ratio
    pub fn recompilation_success_ratio(&self) -> f64 {
        if self.total_recompilations == 0 {
            0.0
        } else {
            self.executed_recompilations as f64 / self.total_recompilations as f64
        }
    }

    /// Get average improvement per recompilation
    pub fn avg_improvement_per_recompilation(&self) -> f64 {
        if self.executed_recompilations == 0 {
            0.0
        } else {
            self.expected_total_improvement / self.executed_recompilations as f64
        }
    }
}

/// Result of a feedback cycle
#[derive(Debug, Clone)]
pub struct FeedbackCycleResult {
    /// Number of feedbacks generated
    pub feedbacks_generated: usize,
    /// Number of decisions made
    pub decisions_made: usize,
    /// Number of decisions executed
    pub decisions_executed: usize,
    /// Total expected improvement
    pub total_expected_improvement: f64,
}

impl FeedbackCycleResult {
    /// Check if cycle was productive
    pub fn was_productive(&self) -> bool {
        self.decisions_executed > 0 && self.total_expected_improvement > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase13_compilation_feedback_creation() {
        let feedback = CompilationFeedback::new(SymbolId(1), 20, CompilationStrategy::Conservative);

        assert_eq!(feedback.func, SymbolId(1));
        assert_eq!(feedback.call_count, 20);
        assert_eq!(
            feedback.previous_strategy,
            CompilationStrategy::Conservative
        );
    }

    #[test]
    fn phase13_compilation_feedback_recommendation() {
        let feedback_cold =
            CompilationFeedback::new(SymbolId(1), 3, CompilationStrategy::Conservative);

        let feedback_warm =
            CompilationFeedback::new(SymbolId(2), 20, CompilationStrategy::Conservative);

        let feedback_hot =
            CompilationFeedback::new(SymbolId(3), 60, CompilationStrategy::Conservative);

        assert_eq!(
            feedback_cold.recommended_strategy,
            CompilationStrategy::Conservative
        );
        assert_eq!(
            feedback_warm.recommended_strategy,
            CompilationStrategy::Balanced
        );
        assert_eq!(
            feedback_hot.recommended_strategy,
            CompilationStrategy::Aggressive
        );
    }

    #[test]
    fn phase13_compilation_feedback_recompile_decision() {
        let feedback = CompilationFeedback::new(SymbolId(1), 20, CompilationStrategy::Conservative);

        assert!(feedback.should_recompile);
    }

    #[test]
    fn phase13_compilation_feedback_no_recompile_low_calls() {
        let feedback = CompilationFeedback::new(SymbolId(1), 3, CompilationStrategy::Conservative);

        assert!(!feedback.should_recompile);
    }

    #[test]
    fn phase13_improvement_estimation_upward() {
        let improvement = CompilationFeedback::estimate_improvement(
            CompilationStrategy::Conservative,
            CompilationStrategy::Aggressive,
        );

        assert!(improvement > 0.0);
    }

    #[test]
    fn phase13_improvement_estimation_downward() {
        let improvement = CompilationFeedback::estimate_improvement(
            CompilationStrategy::Aggressive,
            CompilationStrategy::Conservative,
        );

        assert!(improvement < 0.0);
    }

    #[test]
    fn phase13_recompilation_decision_creation() {
        let feedback = CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

        let decision = FeedbackCompiler::make_recompilation_decision(&feedback, 0);

        assert_eq!(decision.func, SymbolId(1));
        assert!(!decision.executed);
    }

    #[test]
    fn phase13_recompilation_reason_very_hot() {
        let feedback = CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

        let decision = FeedbackCompiler::make_recompilation_decision(&feedback, 0);

        assert_eq!(decision.reason, RecompilationReason::VeryHot);
    }

    #[test]
    fn phase13_feedback_compiler_creation() {
        let profiler = RuntimeProfiler::new(100);
        let compiler = FeedbackCompiler::new(profiler);

        assert!(compiler.function_strategies.is_empty());
        assert!(compiler.recompilation_counts.is_empty());
    }

    #[test]
    fn phase13_feedback_compiler_register() {
        let profiler = RuntimeProfiler::new(100);
        let mut compiler = FeedbackCompiler::new(profiler);

        compiler.register_compilation(SymbolId(1), CompilationStrategy::Conservative);

        assert_eq!(
            compiler.function_strategies.get(&SymbolId(1)),
            Some(&CompilationStrategy::Conservative)
        );
        assert_eq!(compiler.get_recompilation_count(SymbolId(1)), 0);
    }

    #[test]
    fn phase13_feedback_compiler_execute_recompilation() {
        let profiler = RuntimeProfiler::new(100);
        let mut compiler = FeedbackCompiler::new(profiler);

        compiler.register_compilation(SymbolId(1), CompilationStrategy::Conservative);

        let feedback = CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

        let decision = FeedbackCompiler::make_recompilation_decision(&feedback, 0);
        let executed = compiler.execute_recompilation(decision);

        assert!(executed);
        assert_eq!(compiler.get_recompilation_count(SymbolId(1)), 1);
    }

    #[test]
    fn phase13_feedback_compiler_limit_recompilations() {
        let profiler = RuntimeProfiler::new(100);
        let mut compiler = FeedbackCompiler::new(profiler);

        compiler.register_compilation(SymbolId(1), CompilationStrategy::Conservative);

        for _ in 0..4 {
            let feedback =
                CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

            let decision = FeedbackCompiler::make_recompilation_decision(&feedback, 0);
            compiler.execute_recompilation(decision);
        }

        assert_eq!(compiler.get_recompilation_count(SymbolId(1)), 3);
    }

    #[test]
    fn phase13_compilation_stats_success_ratio() {
        let stats = CompilationStats {
            total_compilations: 10,
            total_recompilations: 4,
            executed_recompilations: 3,
            expected_total_improvement: 0.5,
            hot_function_count: 2,
            code_size: 2048,
        };

        assert!((stats.recompilation_success_ratio() - 0.75).abs() < 0.01);
    }

    #[test]
    fn phase13_compilation_stats_avg_improvement() {
        let stats = CompilationStats {
            total_compilations: 10,
            total_recompilations: 4,
            executed_recompilations: 2,
            expected_total_improvement: 0.4,
            hot_function_count: 2,
            code_size: 2048,
        };

        assert!((stats.avg_improvement_per_recompilation() - 0.2).abs() < 0.01);
    }

    #[test]
    fn phase13_feedback_cycle_result_productive() {
        let result = FeedbackCycleResult {
            feedbacks_generated: 5,
            decisions_made: 3,
            decisions_executed: 2,
            total_expected_improvement: 0.3,
        };

        assert!(result.was_productive());
    }

    #[test]
    fn phase13_feedback_cycle_result_not_productive() {
        let result = FeedbackCycleResult {
            feedbacks_generated: 5,
            decisions_made: 0,
            decisions_executed: 0,
            total_expected_improvement: 0.0,
        };

        assert!(!result.was_productive());
    }
}
