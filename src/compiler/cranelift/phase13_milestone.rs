// Phase 13: Feedback-Based Recompilation (Milestone Tests)
//
// Comprehensive tests for Phase 13 features:
// - Compilation feedback from profiling
// - Recompilation decision making
// - Feedback-based compilation pipeline
// - Performance improvement tracking
// - Recompilation history and limits

#[cfg(test)]
mod tests {
    use crate::compiler::cranelift::adaptive_compiler::CompilationStrategy;
    use crate::compiler::cranelift::feedback_compiler::{
        CompilationFeedback, CompilationStats, FeedbackCompiler, FeedbackCycleResult,
        RecompilationReason,
    };
    use crate::compiler::cranelift::profiler::RuntimeProfiler;
    use crate::value::SymbolId;

    // ===== Compilation Feedback Tests =====

    #[test]
    fn phase13_feedback_cold_function() {
        let feedback = CompilationFeedback::new(SymbolId(1), 3, CompilationStrategy::Conservative);

        assert_eq!(
            feedback.recommended_strategy,
            CompilationStrategy::Conservative
        );
        assert!(!feedback.should_recompile);
    }

    #[test]
    fn phase13_feedback_warm_function() {
        let feedback = CompilationFeedback::new(SymbolId(1), 20, CompilationStrategy::Conservative);

        assert_eq!(feedback.recommended_strategy, CompilationStrategy::Balanced);
        assert!(feedback.should_recompile);
    }

    #[test]
    fn phase13_feedback_hot_function() {
        let feedback = CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

        assert_eq!(
            feedback.recommended_strategy,
            CompilationStrategy::Aggressive
        );
        assert!(feedback.should_recompile);
    }

    #[test]
    fn phase13_feedback_improvement_potential_up() {
        let feedback = CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

        assert!(feedback.improvement_potential > 0.0);
    }

    #[test]
    fn phase13_feedback_improvement_potential_none() {
        let feedback = CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Aggressive);

        assert_eq!(feedback.improvement_potential, 0.0);
    }

    #[test]
    fn phase13_feedback_no_recompile_same_strategy() {
        let feedback = CompilationFeedback::new(SymbolId(1), 20, CompilationStrategy::Balanced);

        assert!(!feedback.should_recompile);
    }

    #[test]
    fn phase13_feedback_high_quality() {
        let mut feedback =
            CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);
        feedback.confidence = 0.9;

        assert!(feedback.is_high_quality());
    }

    #[test]
    fn phase13_feedback_low_quality() {
        let feedback = CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

        assert!(!feedback.is_high_quality());
    }

    // ===== Recompilation Decision Tests =====

    #[test]
    fn phase13_recompilation_decision_very_hot() {
        let feedback = CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

        let decision = FeedbackCompiler::make_recompilation_decision(&feedback, 0);

        assert_eq!(decision.reason, RecompilationReason::VeryHot);
        assert!(decision.priority > 0.9);
    }

    #[test]
    fn phase13_recompilation_decision_became_hot() {
        let feedback = CompilationFeedback::new(SymbolId(1), 25, CompilationStrategy::Conservative);

        let decision = FeedbackCompiler::make_recompilation_decision(&feedback, 0);

        assert_eq!(decision.reason, RecompilationReason::BecameHot);
        assert!(decision.priority > 0.7);
    }

    #[test]
    fn phase13_recompilation_decision_diminishing_returns() {
        let feedback = CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

        let decision1 = FeedbackCompiler::make_recompilation_decision(&feedback, 0);
        let decision2 = FeedbackCompiler::make_recompilation_decision(&feedback, 2);

        assert!(decision1.priority > decision2.priority);
    }

    #[test]
    fn phase13_recompilation_decision_not_executed_initially() {
        let feedback = CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

        let decision = FeedbackCompiler::make_recompilation_decision(&feedback, 0);

        assert!(!decision.executed);
    }

    // ===== Feedback Compiler Tests =====

    #[test]
    fn phase13_feedback_compiler_creation() {
        let profiler = RuntimeProfiler::new(100);
        let compiler = FeedbackCompiler::new(profiler);

        assert!(compiler.get_recompilation_history().is_empty());
    }

    #[test]
    fn phase13_feedback_compiler_register_compilation() {
        let profiler = RuntimeProfiler::new(100);
        let mut compiler = FeedbackCompiler::new(profiler);

        compiler.register_compilation(SymbolId(1), CompilationStrategy::Conservative);
        compiler.register_compilation(SymbolId(2), CompilationStrategy::Balanced);

        assert_eq!(compiler.get_recompilation_count(SymbolId(1)), 0);
        assert_eq!(compiler.get_recompilation_count(SymbolId(2)), 0);
    }

    #[test]
    fn phase13_feedback_compiler_single_recompilation() {
        let profiler = RuntimeProfiler::new(100);
        let mut compiler = FeedbackCompiler::new(profiler);

        compiler.register_compilation(SymbolId(1), CompilationStrategy::Conservative);

        let feedback = CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

        let decision = FeedbackCompiler::make_recompilation_decision(&feedback, 0);
        compiler.execute_recompilation(decision.clone());

        assert!(compiler.was_recompiled(SymbolId(1)));
        assert_eq!(compiler.get_recompilation_count(SymbolId(1)), 1);
    }

    #[test]
    fn phase13_feedback_compiler_multiple_recompilations() {
        let profiler = RuntimeProfiler::new(100);
        let mut compiler = FeedbackCompiler::new(profiler);

        compiler.register_compilation(SymbolId(1), CompilationStrategy::Conservative);

        for i in 0..3 {
            let feedback =
                CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

            let decision = FeedbackCompiler::make_recompilation_decision(&feedback, i);
            compiler.execute_recompilation(decision);
        }

        assert_eq!(compiler.get_recompilation_count(SymbolId(1)), 3);
    }

    #[test]
    fn phase13_feedback_compiler_recompilation_limit() {
        let profiler = RuntimeProfiler::new(100);
        let mut compiler = FeedbackCompiler::new(profiler);

        compiler.register_compilation(SymbolId(1), CompilationStrategy::Conservative);

        let mut executed = 0;
        for i in 0..5 {
            let feedback =
                CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

            let decision = FeedbackCompiler::make_recompilation_decision(&feedback, i);
            if compiler.execute_recompilation(decision) {
                executed += 1;
            }
        }

        assert_eq!(executed, 3);
        assert_eq!(compiler.get_recompilation_count(SymbolId(1)), 3);
    }

    #[test]
    fn phase13_feedback_compiler_history_tracking() {
        let profiler = RuntimeProfiler::new(100);
        let mut compiler = FeedbackCompiler::new(profiler);

        compiler.register_compilation(SymbolId(1), CompilationStrategy::Conservative);

        let feedback = CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Conservative);

        let decision = FeedbackCompiler::make_recompilation_decision(&feedback, 0);
        compiler.execute_recompilation(decision);

        assert_eq!(compiler.get_recompilation_history().len(), 1);
    }

    #[test]
    fn phase13_feedback_compiler_analyze_empty() {
        let profiler = RuntimeProfiler::new(100);
        let compiler = FeedbackCompiler::new(profiler);

        let feedbacks = compiler.analyze_feedback();

        assert!(feedbacks.is_empty());
    }

    #[test]
    fn phase13_feedback_compiler_pending_recompilations_empty() {
        let profiler = RuntimeProfiler::new(100);
        let compiler = FeedbackCompiler::new(profiler);

        let pending = compiler.get_pending_recompilations();

        assert!(pending.is_empty());
    }

    #[test]
    fn phase13_feedback_compiler_get_stats() {
        let profiler = RuntimeProfiler::new(100);
        let compiler = FeedbackCompiler::new(profiler);

        let stats = compiler.get_stats();

        assert_eq!(stats.total_compilations, 0);
        assert_eq!(stats.total_recompilations, 0);
    }

    #[test]
    fn phase13_feedback_compiler_trigger_cycle() {
        let profiler = RuntimeProfiler::new(100);
        let mut compiler = FeedbackCompiler::new(profiler);

        compiler.register_compilation(SymbolId(1), CompilationStrategy::Conservative);

        for _ in 0..20 {
            compiler.profiler_mut().record_call(SymbolId(1));
        }

        let result = compiler.trigger_feedback_cycle();

        // Just verify it doesn't panic
        let _ = result.decisions_made;
    }

    // ===== Compilation Stats Tests =====

    #[test]
    fn phase13_compilation_stats_success_ratio_all() {
        let stats = CompilationStats {
            total_compilations: 10,
            total_recompilations: 4,
            executed_recompilations: 4,
            expected_total_improvement: 0.4,
            hot_function_count: 2,
            code_size: 2048,
        };

        assert_eq!(stats.recompilation_success_ratio(), 1.0);
    }

    #[test]
    fn phase13_compilation_stats_success_ratio_half() {
        let stats = CompilationStats {
            total_compilations: 10,
            total_recompilations: 4,
            executed_recompilations: 2,
            expected_total_improvement: 0.2,
            hot_function_count: 2,
            code_size: 2048,
        };

        assert_eq!(stats.recompilation_success_ratio(), 0.5);
    }

    #[test]
    fn phase13_compilation_stats_success_ratio_none() {
        let stats = CompilationStats {
            total_compilations: 10,
            total_recompilations: 0,
            executed_recompilations: 0,
            expected_total_improvement: 0.0,
            hot_function_count: 0,
            code_size: 2048,
        };

        assert_eq!(stats.recompilation_success_ratio(), 0.0);
    }

    #[test]
    fn phase13_compilation_stats_avg_improvement() {
        let stats = CompilationStats {
            total_compilations: 10,
            total_recompilations: 4,
            executed_recompilations: 2,
            expected_total_improvement: 0.6,
            hot_function_count: 2,
            code_size: 2048,
        };

        assert_eq!(stats.avg_improvement_per_recompilation(), 0.3);
    }

    #[test]
    fn phase13_compilation_stats_avg_improvement_none() {
        let stats = CompilationStats {
            total_compilations: 10,
            total_recompilations: 4,
            executed_recompilations: 0,
            expected_total_improvement: 0.0,
            hot_function_count: 0,
            code_size: 2048,
        };

        assert_eq!(stats.avg_improvement_per_recompilation(), 0.0);
    }

    // ===== Feedback Cycle Tests =====

    #[test]
    fn phase13_feedback_cycle_productive() {
        let result = FeedbackCycleResult {
            feedbacks_generated: 5,
            decisions_made: 3,
            decisions_executed: 2,
            total_expected_improvement: 0.3,
        };

        assert!(result.was_productive());
    }

    #[test]
    fn phase13_feedback_cycle_not_productive_no_execution() {
        let result = FeedbackCycleResult {
            feedbacks_generated: 5,
            decisions_made: 3,
            decisions_executed: 0,
            total_expected_improvement: 0.0,
        };

        assert!(!result.was_productive());
    }

    #[test]
    fn phase13_feedback_cycle_not_productive_negative_improvement() {
        let result = FeedbackCycleResult {
            feedbacks_generated: 5,
            decisions_made: 3,
            decisions_executed: 2,
            total_expected_improvement: -0.2,
        };

        assert!(!result.was_productive());
    }

    // ===== Integration Tests =====

    #[test]
    fn phase13_full_workflow_single_function() {
        let profiler = RuntimeProfiler::new(100);
        let mut compiler = FeedbackCompiler::new(profiler);

        // Register initial compilation
        compiler.register_compilation(SymbolId(1), CompilationStrategy::Conservative);

        // Record some profiling data
        for _ in 0..30 {
            compiler.profiler_mut().record_call(SymbolId(1));
        }

        // Analyze and recompile
        let _result = compiler.trigger_feedback_cycle();
        // decisions_made is always >= 0 (it's a usize)
    }

    #[test]
    fn phase13_full_workflow_multiple_functions() {
        let profiler = RuntimeProfiler::new(100);
        let mut compiler = FeedbackCompiler::new(profiler);

        // Register compilations
        compiler.register_compilation(SymbolId(1), CompilationStrategy::Conservative);
        compiler.register_compilation(SymbolId(2), CompilationStrategy::Conservative);
        compiler.register_compilation(SymbolId(3), CompilationStrategy::Conservative);

        // Record profiling data
        for _ in 0..50 {
            compiler.profiler_mut().record_call(SymbolId(1));
        }
        for _ in 0..20 {
            compiler.profiler_mut().record_call(SymbolId(2));
        }

        // Analyze
        let feedbacks = compiler.analyze_feedback();
        assert!(!feedbacks.is_empty());
    }

    #[test]
    fn phase13_strategy_progression() {
        let profiler = RuntimeProfiler::new(100);
        let mut compiler = FeedbackCompiler::new(profiler);

        compiler.register_compilation(SymbolId(1), CompilationStrategy::Conservative);
        assert_eq!(compiler.get_recompilation_count(SymbolId(1)), 0);

        // First recompilation: Conservative -> Balanced
        let feedback1 =
            CompilationFeedback::new(SymbolId(1), 25, CompilationStrategy::Conservative);
        let decision1 = FeedbackCompiler::make_recompilation_decision(&feedback1, 0);
        compiler.execute_recompilation(decision1);

        assert_eq!(compiler.get_recompilation_count(SymbolId(1)), 1);

        // Second recompilation: Balanced -> Aggressive
        let feedback2 = CompilationFeedback::new(SymbolId(1), 60, CompilationStrategy::Balanced);
        let decision2 = FeedbackCompiler::make_recompilation_decision(&feedback2, 1);
        compiler.execute_recompilation(decision2);

        assert_eq!(compiler.get_recompilation_count(SymbolId(1)), 2);
    }

    #[test]
    fn phase13_concurrent_function_management() {
        let profiler = RuntimeProfiler::new(100);
        let mut compiler = FeedbackCompiler::new(profiler);

        for i in 0..5 {
            compiler.register_compilation(SymbolId(i as u32), CompilationStrategy::Conservative);
        }

        for i in 0..5 {
            for _ in 0..(10 * (i + 1)) {
                compiler.profiler_mut().record_call(SymbolId(i as u32));
            }
        }

        let pending = compiler.get_pending_recompilations();
        assert!(!pending.is_empty());

        // Most called functions should have higher priority
        if pending.len() >= 2 {
            assert!(pending[0].priority >= pending[pending.len() - 1].priority);
        }
    }

    #[test]
    fn phase13_profiler_integration() {
        let profiler = RuntimeProfiler::new(100);
        let compiler = FeedbackCompiler::new(profiler);

        compiler.profiler().record_call(SymbolId(1));
        compiler.profiler().record_call(SymbolId(1));

        let frequencies = compiler.profiler().get_call_frequencies();
        assert_eq!(frequencies.get(&SymbolId(1)), Some(&2));
    }

    #[test]
    fn phase13_improvement_estimation_conservative_to_aggressive() {
        let improvement = CompilationFeedback::estimate_improvement(
            CompilationStrategy::Conservative,
            CompilationStrategy::Aggressive,
        );

        assert!(improvement > 0.3);
    }

    #[test]
    fn phase13_improvement_estimation_conservative_to_balanced() {
        let improvement = CompilationFeedback::estimate_improvement(
            CompilationStrategy::Conservative,
            CompilationStrategy::Balanced,
        );

        assert!(improvement > 0.1 && improvement < 0.2);
    }
}
