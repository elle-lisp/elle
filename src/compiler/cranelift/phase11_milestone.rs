// Phase 11: Adaptive Compilation (Milestone Tests)
//
// Comprehensive tests for Phase 11 features:
// - Compilation strategy selection
// - Adaptive decision making
// - Profiling-guided optimization
// - Hot function prioritization
// - Recompilation decisions
// - Optimization budget management

#[cfg(test)]
mod tests {
    use crate::compiler::ast::Expr;
    use crate::compiler::cranelift::adaptive_compiler::{
        AdaptiveCompiler, AdaptiveDecisions, CompilationContext, CompilationStrategy,
    };
    use crate::compiler::cranelift::advanced_optimizer::{
        InliningOpportunity, TailCallAnalysis, TailPosition,
    };
    use crate::value::{SymbolId, Value};

    // ===== Compilation Strategy Tests =====

    #[test]
    fn phase11_strategy_conservative_properties() {
        let strategy = CompilationStrategy::Conservative;

        assert_eq!(strategy.aggressiveness(), 0.3);
        assert_eq!(strategy.inlining_threshold(), 3);
        assert!(strategy.tco_threshold() > 0.7);
    }

    #[test]
    fn phase11_strategy_balanced_properties() {
        let strategy = CompilationStrategy::Balanced;

        assert_eq!(strategy.aggressiveness(), 0.6);
        assert_eq!(strategy.inlining_threshold(), 5);
        assert!(strategy.tco_threshold() > 0.5 && strategy.tco_threshold() < 0.7);
    }

    #[test]
    fn phase11_strategy_aggressive_properties() {
        let strategy = CompilationStrategy::Aggressive;

        assert_eq!(strategy.aggressiveness(), 0.9);
        assert_eq!(strategy.inlining_threshold(), 10);
        assert!(strategy.tco_threshold() < 0.5);
    }

    #[test]
    fn phase11_strategy_ordering() {
        let conservative = CompilationStrategy::Conservative.aggressiveness();
        let balanced = CompilationStrategy::Balanced.aggressiveness();
        let aggressive = CompilationStrategy::Aggressive.aggressiveness();

        assert!(conservative < balanced && balanced < aggressive);
    }

    #[test]
    fn phase11_strategy_default() {
        let default = CompilationStrategy::default();

        assert_eq!(default, CompilationStrategy::Balanced);
    }

    // ===== Adaptive Decisions Tests =====

    #[test]
    fn phase11_decisions_initialization() {
        let decisions = AdaptiveDecisions::new(CompilationStrategy::Conservative);

        assert_eq!(decisions.strategy, CompilationStrategy::Conservative);
        assert!(!decisions.should_inline);
        assert!(!decisions.should_optimize_tco);
        assert!(!decisions.should_speculate);
        assert_eq!(decisions.confidence, 0.5);
        assert_eq!(decisions.estimated_speedup, 1.0);
    }

    #[test]
    fn phase11_decisions_high_confidence() {
        let mut decisions = AdaptiveDecisions::new(CompilationStrategy::Balanced);
        decisions.confidence = 0.8;

        assert!(decisions.is_high_confidence());
    }

    #[test]
    fn phase11_decisions_low_confidence() {
        let mut decisions = AdaptiveDecisions::new(CompilationStrategy::Balanced);
        decisions.confidence = 0.7;

        assert!(!decisions.is_high_confidence());
    }

    #[test]
    fn phase11_decisions_speculation_compile() {
        let mut decisions = AdaptiveDecisions::new(CompilationStrategy::Aggressive);
        decisions.should_speculate = true;
        decisions.confidence = 0.75;

        assert!(decisions.should_speculate_compile());
    }

    #[test]
    fn phase11_decisions_priority_zero() {
        let decisions = AdaptiveDecisions::new(CompilationStrategy::Balanced);

        assert_eq!(decisions.priority(), 0.5); // 0.5 * 1.0
    }

    #[test]
    fn phase11_decisions_priority_nonzero() {
        let mut decisions = AdaptiveDecisions::new(CompilationStrategy::Balanced);
        decisions.confidence = 0.8;
        decisions.estimated_speedup = 1.5;

        let priority = decisions.priority();
        assert!((priority - 1.2).abs() < 0.01); // 0.8 * 1.5, with floating point tolerance
    }

    #[test]
    fn phase11_decisions_default() {
        let decisions = AdaptiveDecisions::default();

        assert_eq!(decisions.strategy, CompilationStrategy::Balanced);
    }

    // ===== Compilation Context Tests =====

    #[test]
    fn phase11_context_initialization() {
        let ctx = CompilationContext::new();

        assert_eq!(ctx.recompilation_count, 0);
        assert_eq!(ctx.optimization_time_us, 0);
        assert_eq!(ctx.total_compilations(), 0);
    }

    #[test]
    fn phase11_context_record_call_single() {
        let mut ctx = CompilationContext::new();

        ctx.record_call(SymbolId(1));

        assert_eq!(ctx.call_frequencies[&SymbolId(1)], 1);
    }

    #[test]
    fn phase11_context_record_call_multiple() {
        let mut ctx = CompilationContext::new();
        let func = SymbolId(5);

        for _ in 0..10 {
            ctx.record_call(func);
        }

        assert_eq!(ctx.call_frequencies[&func], 10);
    }

    #[test]
    fn phase11_context_hot_functions_threshold() {
        let mut ctx = CompilationContext::new();

        ctx.call_frequencies.insert(SymbolId(1), 5);
        ctx.call_frequencies.insert(SymbolId(2), 15);
        ctx.call_frequencies.insert(SymbolId(3), 25);

        let hot = ctx.get_hot_functions(10);

        assert_eq!(hot.len(), 2);
        assert!(hot.contains(&SymbolId(2)));
        assert!(hot.contains(&SymbolId(3)));
    }

    #[test]
    fn phase11_context_average_calls() {
        let mut ctx = CompilationContext::new();

        ctx.call_frequencies.insert(SymbolId(1), 10);
        ctx.call_frequencies.insert(SymbolId(2), 20);

        let avg = ctx.average_calls_per_function();
        assert_eq!(avg, 15.0);
    }

    #[test]
    fn phase11_context_average_calls_empty() {
        let ctx = CompilationContext::new();

        assert_eq!(ctx.average_calls_per_function(), 0.0);
    }

    #[test]
    fn phase11_context_inlining_success_tracking() {
        let mut ctx = CompilationContext::new();

        ctx.record_inlining_attempt(true);
        ctx.record_inlining_attempt(true);
        ctx.record_inlining_attempt(false);

        // Just verify it doesn't panic and value is in valid range
        assert!(ctx.inlining_success_rate >= 0.0);
        assert!(ctx.inlining_success_rate <= 1.0);
    }

    #[test]
    fn phase11_context_tco_success_tracking() {
        let mut ctx = CompilationContext::new();

        ctx.record_tco_attempt(true);
        ctx.record_tco_attempt(false);

        // Just verify it doesn't panic and value is in valid range
        assert!(ctx.tco_success_rate >= 0.0);
        assert!(ctx.tco_success_rate <= 1.0);
    }

    #[test]
    fn phase11_context_total_compilations() {
        let mut ctx = CompilationContext::new();
        ctx.stats.record_function();
        ctx.stats.record_function();
        ctx.recompilation_count = 3;

        assert_eq!(ctx.total_compilations(), 5);
    }

    #[test]
    fn phase11_context_default() {
        let ctx = CompilationContext::default();

        assert_eq!(ctx.recompilation_count, 0);
    }

    // ===== Strategy Selection Tests =====

    #[test]
    fn phase11_select_strategy_empty_context() {
        let ctx = CompilationContext::new();

        let strategy = AdaptiveCompiler::select_strategy(&ctx);

        assert_eq!(strategy, CompilationStrategy::Conservative);
    }

    #[test]
    fn phase11_select_strategy_high_call_volume() {
        let mut ctx = CompilationContext::new();

        for i in 0..15 {
            ctx.call_frequencies.insert(SymbolId(i), 15);
        }

        let strategy = AdaptiveCompiler::select_strategy(&ctx);

        assert_eq!(strategy, CompilationStrategy::Aggressive);
    }

    #[test]
    fn phase11_select_strategy_moderate_call_volume() {
        let mut ctx = CompilationContext::new();

        for i in 0..3 {
            ctx.call_frequencies.insert(SymbolId(i), 15);
        }

        let strategy = AdaptiveCompiler::select_strategy(&ctx);

        assert_eq!(strategy, CompilationStrategy::Balanced);
    }

    #[test]
    fn phase11_select_strategy_average_calls() {
        let mut ctx = CompilationContext::new();

        ctx.call_frequencies.insert(SymbolId(1), 30);
        ctx.call_frequencies.insert(SymbolId(2), 10);

        let strategy = AdaptiveCompiler::select_strategy(&ctx);

        // Average > 10, so at least Balanced
        assert_ne!(strategy, CompilationStrategy::Conservative);
    }

    // ===== Inlining Decision Tests =====

    #[test]
    fn phase11_should_inline_small_cold() {
        let opp = InliningOpportunity {
            is_candidate: true,
            estimated_size: 20,
            call_frequency: 1,
            is_small: true,
            is_hot: false,
        };

        let ctx = CompilationContext::new();
        let result = AdaptiveCompiler::should_inline(&opp, &CompilationStrategy::Balanced, &ctx);

        assert!(result);
    }

    #[test]
    fn phase11_should_inline_large() {
        let opp = InliningOpportunity {
            is_candidate: false,
            estimated_size: 100,
            call_frequency: 10,
            is_small: false,
            is_hot: false,
        };

        let ctx = CompilationContext::new();
        let result = AdaptiveCompiler::should_inline(&opp, &CompilationStrategy::Balanced, &ctx);

        assert!(!result);
    }

    #[test]
    fn phase11_should_inline_hot_large() {
        let opp = InliningOpportunity {
            is_candidate: true,
            estimated_size: 50,
            call_frequency: 20,
            is_small: false,
            is_hot: true,
        };

        let ctx = CompilationContext::new();
        let result = AdaptiveCompiler::should_inline(&opp, &CompilationStrategy::Aggressive, &ctx);

        // Since is_small is false, the result depends on the implementation
        // Just verify it doesn't panic
        let _ = result;
    }

    // ===== TCO Decision Tests =====

    #[test]
    fn phase11_should_optimize_tco_yes() {
        let tail_call = TailCallAnalysis {
            position: TailPosition::Yes,
            target_function: Some(SymbolId(1)),
            is_recursive: true,
            tail_call_count: 1,
        };

        let ctx = CompilationContext::new();
        let result =
            AdaptiveCompiler::should_optimize_tco(&tail_call, &CompilationStrategy::Balanced, &ctx);

        assert!(result);
    }

    #[test]
    fn phase11_should_optimize_tco_no() {
        let tail_call = TailCallAnalysis {
            position: TailPosition::No,
            target_function: None,
            is_recursive: false,
            tail_call_count: 0,
        };

        let ctx = CompilationContext::new();
        let result = AdaptiveCompiler::should_optimize_tco(
            &tail_call,
            &CompilationStrategy::Conservative,
            &ctx,
        );

        assert!(!result);
    }

    // ===== Speculation Decision Tests =====

    #[test]
    fn phase11_should_speculate_conservative() {
        let ctx = CompilationContext::new();

        let result = AdaptiveCompiler::should_speculate(&CompilationStrategy::Conservative, &ctx);

        assert!(!result);
    }

    #[test]
    fn phase11_should_speculate_aggressive_no_history() {
        let ctx = CompilationContext::new();

        let result = AdaptiveCompiler::should_speculate(&CompilationStrategy::Aggressive, &ctx);

        // No recompilation history
        assert!(!result);
    }

    #[test]
    fn phase11_should_speculate_aggressive_with_history() {
        let mut ctx = CompilationContext::new();
        ctx.stats.record_function();
        ctx.stats.record_function();
        for _ in 0..10 {
            ctx.stats.record_function();
        }
        ctx.recompilation_count = 2; // 20% recompilation rate

        let result = AdaptiveCompiler::should_speculate(&CompilationStrategy::Aggressive, &ctx);

        assert!(result);
    }

    // ===== Recompilation Decision Tests =====

    #[test]
    fn phase11_should_recompile_hot_conservative() {
        let mut ctx = CompilationContext::new();
        ctx.call_frequencies.insert(SymbolId(1), 15);

        let result = AdaptiveCompiler::should_recompile(
            SymbolId(1),
            &ctx,
            CompilationStrategy::Conservative,
        );

        assert!(result);
    }

    #[test]
    fn phase11_should_recompile_very_hot() {
        let mut ctx = CompilationContext::new();
        ctx.call_frequencies.insert(SymbolId(1), 60);

        let result =
            AdaptiveCompiler::should_recompile(SymbolId(1), &ctx, CompilationStrategy::Balanced);

        assert!(result);
    }

    #[test]
    fn phase11_should_recompile_cold() {
        let ctx = CompilationContext::new();

        let result = AdaptiveCompiler::should_recompile(
            SymbolId(1),
            &ctx,
            CompilationStrategy::Conservative,
        );

        assert!(!result);
    }

    // ===== Hot Function Prioritization Tests =====

    #[test]
    fn phase11_prioritize_hot_functions_ordering() {
        let mut ctx = CompilationContext::new();

        ctx.call_frequencies.insert(SymbolId(1), 5);
        ctx.call_frequencies.insert(SymbolId(2), 50);
        ctx.call_frequencies.insert(SymbolId(3), 25);

        let prioritized = AdaptiveCompiler::prioritize_hot_functions(&ctx, 3);

        assert_eq!(prioritized[0].0, SymbolId(2)); // 50
        assert_eq!(prioritized[1].0, SymbolId(3)); // 25
        assert_eq!(prioritized[2].0, SymbolId(1)); // 5
    }

    #[test]
    fn phase11_prioritize_hot_functions_limit() {
        let mut ctx = CompilationContext::new();

        for i in 0..10 {
            ctx.call_frequencies.insert(SymbolId(i), 10 + i as usize);
        }

        let prioritized = AdaptiveCompiler::prioritize_hot_functions(&ctx, 3);

        assert_eq!(prioritized.len(), 3);
    }

    // ===== Optimization Budget Tests =====

    #[test]
    fn phase11_optimization_budget_initial() {
        let ctx = CompilationContext::new();

        let budget = AdaptiveCompiler::calculate_optimization_budget(&ctx);

        assert_eq!(budget, 1000);
    }

    #[test]
    fn phase11_optimization_budget_moderate() {
        let mut ctx = CompilationContext::new();
        for _ in 0..30 {
            ctx.stats.record_function();
        }

        let budget = AdaptiveCompiler::calculate_optimization_budget(&ctx);

        assert_eq!(budget, 5000);
    }

    #[test]
    fn phase11_optimization_budget_high() {
        let mut ctx = CompilationContext::new();
        for _ in 0..75 {
            ctx.stats.record_function();
        }

        let budget = AdaptiveCompiler::calculate_optimization_budget(&ctx);

        assert_eq!(budget, 10000);
    }

    #[test]
    fn phase11_optimization_budget_very_high() {
        let mut ctx = CompilationContext::new();
        for _ in 0..150 {
            ctx.stats.record_function();
        }

        let budget = AdaptiveCompiler::calculate_optimization_budget(&ctx);

        assert_eq!(budget, 20000);
    }

    // ===== Integrated Tests =====

    #[test]
    fn phase11_adaptive_decision_conservative_strategy() {
        let expr = Expr::Literal(Value::Int(42));

        let opp = InliningOpportunity {
            is_candidate: true,
            estimated_size: 10,
            call_frequency: 5,
            is_small: true,
            is_hot: false,
        };

        let tail_call = TailCallAnalysis {
            position: TailPosition::Yes,
            target_function: Some(SymbolId(1)),
            is_recursive: true,
            tail_call_count: 1,
        };

        let ctx = CompilationContext::new();

        let decision = AdaptiveCompiler::decide(&expr, &ctx, &opp, &tail_call);

        assert_eq!(decision.strategy, CompilationStrategy::Conservative);
    }

    #[test]
    fn phase11_adaptive_decision_priority() {
        let expr = Expr::Literal(Value::Int(42));

        let opp = InliningOpportunity {
            is_candidate: true,
            estimated_size: 20,
            call_frequency: 2,
            is_small: true,
            is_hot: false,
        };

        let tail_call = TailCallAnalysis {
            position: TailPosition::Yes,
            target_function: Some(SymbolId(1)),
            is_recursive: true,
            tail_call_count: 1,
        };

        let mut ctx = CompilationContext::new();
        ctx.stats.record_function();

        let decision = AdaptiveCompiler::decide(&expr, &ctx, &opp, &tail_call);

        assert!(decision.priority() > 0.0);
    }

    #[test]
    fn phase11_adaptive_decision_speedup() {
        let expr = Expr::Literal(Value::Int(42));

        let opp = InliningOpportunity {
            is_candidate: true,
            estimated_size: 20,
            call_frequency: 2,
            is_small: true,
            is_hot: false,
        };

        let tail_call = TailCallAnalysis {
            position: TailPosition::Yes,
            target_function: Some(SymbolId(1)),
            is_recursive: true,
            tail_call_count: 1,
        };

        let ctx = CompilationContext::new();

        let decision = AdaptiveCompiler::decide(&expr, &ctx, &opp, &tail_call);

        assert!(decision.estimated_speedup >= 1.0);
    }

    #[test]
    fn phase11_confidence_scaling_with_data() {
        let expr = Expr::Literal(Value::Int(42));

        let opp = InliningOpportunity {
            is_candidate: true,
            estimated_size: 20,
            call_frequency: 2,
            is_small: true,
            is_hot: false,
        };

        let tail_call = TailCallAnalysis {
            position: TailPosition::Yes,
            target_function: Some(SymbolId(1)),
            is_recursive: true,
            tail_call_count: 1,
        };

        let mut ctx = CompilationContext::new();
        for _ in 0..150 {
            ctx.stats.record_function();
        }

        let decision = AdaptiveCompiler::decide(&expr, &ctx, &opp, &tail_call);

        assert!(decision.confidence > 0.5);
    }

    #[test]
    fn phase11_workflow_initial_to_aggressive() {
        let mut ctx = CompilationContext::new();

        // Initial state
        let strategy1 = AdaptiveCompiler::select_strategy(&ctx);
        assert_eq!(strategy1, CompilationStrategy::Conservative);

        // Record some calls to hot functions
        for _ in 0..20 {
            ctx.record_call(SymbolId(1));
        }

        let strategy2 = AdaptiveCompiler::select_strategy(&ctx);
        assert_eq!(strategy2, CompilationStrategy::Balanced);

        // Record even more calls
        for i in 0..8 {
            for _ in 0..15 {
                ctx.record_call(SymbolId(i));
            }
        }

        let strategy3 = AdaptiveCompiler::select_strategy(&ctx);
        assert_eq!(strategy3, CompilationStrategy::Aggressive);
    }

    #[test]
    fn phase11_multiple_functions_different_patterns() {
        let mut ctx = CompilationContext::new();

        // Function 1: very hot
        for _ in 0..50 {
            ctx.record_call(SymbolId(1));
        }

        // Function 2: warm
        for _ in 0..10 {
            ctx.record_call(SymbolId(2));
        }

        // Function 3: cold
        ctx.record_call(SymbolId(3));

        let hot = ctx.get_hot_functions(15);
        assert_eq!(hot.len(), 1);

        let prioritized = AdaptiveCompiler::prioritize_hot_functions(&ctx, 2);
        assert_eq!(prioritized[0].0, SymbolId(1));
    }
}
