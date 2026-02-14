// Phase 12: Runtime Profiling Collection (Milestone Tests)
//
// Comprehensive tests for Phase 12 features:
// - Runtime profiling data collection
// - Profiling events and tracking
// - Snapshots and historical data
// - Thread-safe profiler state
// - Hot function detection
// - Profiling summaries

#[cfg(test)]
mod tests {
    use crate::compiler::cranelift::advanced_optimizer::JitStats;
    use crate::compiler::cranelift::profiler::{
        ProfilingEvent, ProfilingSnapshot, ProfilingSummary, RuntimeProfiler,
    };
    use crate::value::SymbolId;
    use std::collections::HashMap;

    // ===== Profiling Event Tests =====

    #[test]
    fn phase12_profiling_event_function_call() {
        assert_eq!(ProfilingEvent::FunctionCall.weight(), 1);
    }

    #[test]
    fn phase12_profiling_event_compilation() {
        assert!(
            ProfilingEvent::ExpressionCompiled.weight() > ProfilingEvent::FunctionCall.weight()
        );
        assert!(
            ProfilingEvent::FunctionCompiled.weight() > ProfilingEvent::ExpressionCompiled.weight()
        );
    }

    #[test]
    fn phase12_profiling_event_optimization() {
        let tco = ProfilingEvent::TailCallOptimized.weight();
        let inline = ProfilingEvent::FunctionInlined.weight();
        let dce = ProfilingEvent::DeadCodeEliminated.weight();

        assert!(tco > dce);
        assert!(inline > dce);
    }

    #[test]
    fn phase12_profiling_event_weights_ordered() {
        let call = ProfilingEvent::FunctionCall.weight();
        let pass = ProfilingEvent::OptimizationPass.weight();
        let dce = ProfilingEvent::DeadCodeEliminated.weight();
        let expr = ProfilingEvent::ExpressionCompiled.weight();
        let func = ProfilingEvent::FunctionCompiled.weight();

        assert!(call <= pass);
        assert!(dce <= pass);
        assert!(pass < expr);
        assert!(expr < func);
    }

    // ===== Profiling Snapshot Tests =====

    #[test]
    fn phase12_snapshot_creation() {
        let stats = JitStats::new();
        let frequencies = HashMap::new();

        let snapshot = ProfilingSnapshot::new(stats, frequencies);

        assert_eq!(snapshot.total_events(), 0);
        assert_eq!(snapshot.weighted_events(), 0);
    }

    #[test]
    fn phase12_snapshot_with_data() {
        let mut stats = JitStats::new();
        stats.record_expression();
        stats.record_function();

        let mut frequencies = HashMap::new();
        frequencies.insert(SymbolId(1), 5);

        let snapshot = ProfilingSnapshot::new(stats, frequencies);

        assert_eq!(snapshot.stats.expressions_compiled, 1);
        assert_eq!(snapshot.stats.functions_compiled, 1);
        assert_eq!(snapshot.call_frequencies.get(&SymbolId(1)), Some(&5));
    }

    #[test]
    fn phase12_snapshot_timestamp() {
        let snapshot = ProfilingSnapshot::new(JitStats::new(), HashMap::new());

        assert!(snapshot.timestamp > 0);
    }

    #[test]
    fn phase12_snapshot_weighted_events_single() {
        let mut events = HashMap::new();
        events.insert(ProfilingEvent::FunctionCall, 10);

        let snapshot = ProfilingSnapshot {
            timestamp: 0,
            events,
            stats: JitStats::new(),
            call_frequencies: HashMap::new(),
        };

        assert_eq!(snapshot.weighted_events(), 10);
    }

    #[test]
    fn phase12_snapshot_weighted_events_multiple() {
        let mut events = HashMap::new();
        events.insert(ProfilingEvent::FunctionCall, 5);
        events.insert(ProfilingEvent::FunctionCompiled, 2);

        let snapshot = ProfilingSnapshot {
            timestamp: 0,
            events,
            stats: JitStats::new(),
            call_frequencies: HashMap::new(),
        };

        // 5 * 1 + 2 * 10 = 25
        assert_eq!(snapshot.weighted_events(), 25);
    }

    // ===== Runtime Profiler Tests =====

    #[test]
    fn phase12_profiler_creation() {
        let profiler = RuntimeProfiler::new(100);

        assert!(profiler.is_enabled());
    }

    #[test]
    fn phase12_profiler_enable() {
        let profiler = RuntimeProfiler::new(100);

        profiler.disable();
        assert!(!profiler.is_enabled());

        profiler.enable();
        assert!(profiler.is_enabled());
    }

    #[test]
    fn phase12_profiler_record_single_call() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));

        let frequencies = profiler.get_call_frequencies();
        assert_eq!(frequencies.get(&SymbolId(1)), Some(&1));
    }

    #[test]
    fn phase12_profiler_record_multiple_calls() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));
        profiler.record_call(SymbolId(1));
        profiler.record_call(SymbolId(1));

        let frequencies = profiler.get_call_frequencies();
        assert_eq!(frequencies.get(&SymbolId(1)), Some(&3));
    }

    #[test]
    fn phase12_profiler_record_multiple_functions() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));
        profiler.record_call(SymbolId(2));
        profiler.record_call(SymbolId(1));

        let frequencies = profiler.get_call_frequencies();
        assert_eq!(frequencies.get(&SymbolId(1)), Some(&2));
        assert_eq!(frequencies.get(&SymbolId(2)), Some(&1));
    }

    #[test]
    fn phase12_profiler_record_event_expression() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_event(ProfilingEvent::ExpressionCompiled);

        let stats = profiler.get_stats();
        assert_eq!(stats.expressions_compiled, 1);
    }

    #[test]
    fn phase12_profiler_record_event_function() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_event(ProfilingEvent::FunctionCompiled);

        let stats = profiler.get_stats();
        assert_eq!(stats.functions_compiled, 1);
    }

    #[test]
    fn phase12_profiler_record_event_optimization() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_event(ProfilingEvent::OptimizationPass);
        profiler.record_event(ProfilingEvent::DeadCodeEliminated);
        profiler.record_event(ProfilingEvent::TailCallOptimized);

        let stats = profiler.get_stats();
        assert_eq!(stats.optimization_passes, 1);
        assert_eq!(stats.dead_code_eliminated, 1);
        assert_eq!(stats.tail_calls_optimized, 1);
    }

    #[test]
    fn phase12_profiler_code_size_single() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_code_size(1024);

        let stats = profiler.get_stats();
        assert_eq!(stats.code_size, 1024);
    }

    #[test]
    fn phase12_profiler_code_size_accumulate() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_code_size(1024);
        profiler.record_code_size(512);
        profiler.record_code_size(256);

        let stats = profiler.get_stats();
        assert_eq!(stats.code_size, 1792);
    }

    #[test]
    fn phase12_profiler_snapshot_single() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));
        profiler.record_event(ProfilingEvent::ExpressionCompiled);

        let snapshot = profiler.snapshot();

        assert_eq!(snapshot.stats.expressions_compiled, 1);
        assert_eq!(snapshot.call_frequencies.get(&SymbolId(1)), Some(&1));
    }

    #[test]
    fn phase12_profiler_snapshot_multiple() {
        let profiler = RuntimeProfiler::new(100);

        profiler.snapshot();
        profiler.record_event(ProfilingEvent::ExpressionCompiled);
        profiler.snapshot();

        let snapshots = profiler.get_snapshots();
        assert_eq!(snapshots.len(), 2);
    }

    #[test]
    fn phase12_profiler_snapshot_respects_limit() {
        let profiler = RuntimeProfiler::new(3);

        for _ in 0..5 {
            profiler.snapshot();
        }

        let snapshots = profiler.get_snapshots();
        assert_eq!(snapshots.len(), 3);
    }

    #[test]
    fn phase12_profiler_snapshot_fifo_eviction() {
        let profiler = RuntimeProfiler::new(2);

        profiler.record_call(SymbolId(1));
        profiler.snapshot(); // Snapshot 1

        profiler.record_call(SymbolId(2));
        profiler.snapshot(); // Snapshot 2

        profiler.record_call(SymbolId(3));
        profiler.snapshot(); // Snapshot 3 (evicts snapshot 1)

        let snapshots = profiler.get_snapshots();
        assert_eq!(snapshots.len(), 2);
        // First remaining snapshot should have SymbolId(2)
        assert!(snapshots[0].call_frequencies.contains_key(&SymbolId(2)));
    }

    #[test]
    fn phase12_profiler_clear() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));
        profiler.record_event(ProfilingEvent::ExpressionCompiled);
        profiler.record_code_size(1024);
        profiler.snapshot();

        profiler.clear();

        let stats = profiler.get_stats();
        let frequencies = profiler.get_call_frequencies();
        let snapshots = profiler.get_snapshots();

        assert_eq!(stats.expressions_compiled, 0);
        assert_eq!(stats.code_size, 0);
        assert!(frequencies.is_empty());
        assert!(snapshots.is_empty());
    }

    #[test]
    fn phase12_profiler_is_function_hot() {
        let profiler = RuntimeProfiler::new(100);

        for _ in 0..10 {
            profiler.record_call(SymbolId(1));
        }
        profiler.record_call(SymbolId(2));

        assert!(profiler.is_function_hot(SymbolId(1), 5));
        assert!(!profiler.is_function_hot(SymbolId(2), 5));
        assert!(!profiler.is_function_hot(SymbolId(3), 5));
    }

    #[test]
    fn phase12_profiler_get_hot_functions_empty() {
        let profiler = RuntimeProfiler::new(100);

        let hot = profiler.get_hot_functions(10);

        assert!(hot.is_empty());
    }

    #[test]
    fn phase12_profiler_get_hot_functions_single() {
        let profiler = RuntimeProfiler::new(100);

        for _ in 0..10 {
            profiler.record_call(SymbolId(1));
        }

        let hot = profiler.get_hot_functions(5);

        assert_eq!(hot.len(), 1);
        assert_eq!(hot[0], (SymbolId(1), 10));
    }

    #[test]
    fn phase12_profiler_get_hot_functions_multiple() {
        let profiler = RuntimeProfiler::new(100);

        for _ in 0..10 {
            profiler.record_call(SymbolId(1));
        }
        for _ in 0..8 {
            profiler.record_call(SymbolId(2));
        }
        profiler.record_call(SymbolId(3));

        let hot = profiler.get_hot_functions(5);

        assert_eq!(hot.len(), 2);
        assert_eq!(hot[0].0, SymbolId(1)); // Most hot first
        assert_eq!(hot[1].0, SymbolId(2));
    }

    #[test]
    fn phase12_profiler_clone() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));
        profiler.record_event(ProfilingEvent::ExpressionCompiled);

        let cloned = profiler.clone();

        let frequencies = cloned.get_call_frequencies();
        let stats = cloned.get_stats();

        assert_eq!(frequencies.get(&SymbolId(1)), Some(&1));
        assert_eq!(stats.expressions_compiled, 1);
    }

    #[test]
    fn phase12_profiler_clone_shared_state() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));

        let cloned = profiler.clone();
        cloned.record_call(SymbolId(1));

        // Both should see the updates
        assert_eq!(profiler.get_call_frequencies().get(&SymbolId(1)), Some(&2));
    }

    #[test]
    fn phase12_profiler_disabled_skips_recording() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));
        profiler.disable();
        profiler.record_call(SymbolId(1));

        let frequencies = profiler.get_call_frequencies();
        assert_eq!(frequencies.get(&SymbolId(1)), Some(&1));
    }

    #[test]
    fn phase12_profiler_re_enable_resumes() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));
        profiler.disable();
        profiler.record_call(SymbolId(1));
        profiler.enable();
        profiler.record_call(SymbolId(1));

        let frequencies = profiler.get_call_frequencies();
        assert_eq!(frequencies.get(&SymbolId(1)), Some(&2));
    }

    // ===== Profiling Summary Tests =====

    #[test]
    fn phase12_summary_empty() {
        let summary = ProfilingSummary {
            total_functions_compiled: 0,
            total_expressions_compiled: 0,
            total_optimizations: 0,
            optimization_ratio: 0.0,
            hot_functions: vec![],
            total_snapshots: 0,
            code_size: 0,
        };

        assert!(!summary.has_hot_functions());
        assert_eq!(summary.hottest_function(), None);
        assert_eq!(summary.avg_optimization_benefit(), 0.0);
    }

    #[test]
    fn phase12_summary_with_hot_functions() {
        let summary = ProfilingSummary {
            total_functions_compiled: 5,
            total_expressions_compiled: 20,
            total_optimizations: 10,
            optimization_ratio: 0.5,
            hot_functions: vec![(SymbolId(1), 50), (SymbolId(2), 30)],
            total_snapshots: 5,
            code_size: 2048,
        };

        assert!(summary.has_hot_functions());
        assert_eq!(summary.hottest_function(), Some((SymbolId(1), 50)));
    }

    #[test]
    fn phase12_summary_avg_optimization_benefit() {
        let summary = ProfilingSummary {
            total_functions_compiled: 5,
            total_expressions_compiled: 10,
            total_optimizations: 5,
            optimization_ratio: 0.5,
            hot_functions: vec![],
            total_snapshots: 0,
            code_size: 0,
        };

        assert_eq!(summary.avg_optimization_benefit(), 0.5);
    }

    #[test]
    fn phase12_summary_from_profiler() {
        let profiler = RuntimeProfiler::new(100);

        for _ in 0..10 {
            profiler.record_call(SymbolId(1));
        }
        profiler.record_event(ProfilingEvent::ExpressionCompiled);
        profiler.record_event(ProfilingEvent::FunctionCompiled);
        profiler.record_event(ProfilingEvent::DeadCodeEliminated);

        let summary = profiler.summary();

        assert_eq!(summary.total_expressions_compiled, 1);
        assert_eq!(summary.total_functions_compiled, 1);
        assert_eq!(summary.total_optimizations, 1);
        assert!(summary.has_hot_functions());
    }

    // ===== Integration Tests =====

    #[test]
    fn phase12_profiling_workflow_basic() {
        let profiler = RuntimeProfiler::new(10);

        // Simulate some compilation
        profiler.record_event(ProfilingEvent::ExpressionCompiled);
        profiler.record_event(ProfilingEvent::FunctionCompiled);

        // Record function calls
        for _ in 0..5 {
            profiler.record_call(SymbolId(1));
        }
        for _ in 0..3 {
            profiler.record_call(SymbolId(2));
        }

        // Record code generation
        profiler.record_code_size(2048);

        // Take snapshot
        let snapshot = profiler.snapshot();

        assert_eq!(snapshot.stats.expressions_compiled, 1);
        assert_eq!(snapshot.stats.functions_compiled, 1);
        assert_eq!(snapshot.stats.code_size, 2048);
        assert_eq!(snapshot.call_frequencies.get(&SymbolId(1)), Some(&5));
    }

    #[test]
    fn phase12_profiling_workflow_multiple_phases() {
        let profiler = RuntimeProfiler::new(100);

        // Phase 1: Initial compilation
        profiler.record_event(ProfilingEvent::FunctionCompiled);
        profiler.record_call(SymbolId(1));
        profiler.snapshot();

        // Phase 2: More calls and optimization
        for _ in 0..9 {
            profiler.record_call(SymbolId(1));
        }
        profiler.record_event(ProfilingEvent::TailCallOptimized);
        profiler.snapshot();

        let summary = profiler.summary();

        assert!(summary.has_hot_functions());
        assert_eq!(summary.hottest_function().unwrap().0, SymbolId(1));
        assert_eq!(summary.total_optimizations, 1);
    }

    #[test]
    fn phase12_profiling_concurrent_functions() {
        let profiler = RuntimeProfiler::new(100);

        for i in 0..5 {
            for _ in 0..(i * 10) {
                profiler.record_call(SymbolId(i as u32));
            }
        }

        let hot = profiler.get_hot_functions(15);

        // Functions 2, 3, 4 should be hot (20, 30, 40 calls)
        assert!(hot.len() >= 2);
    }

    #[test]
    fn phase12_profiling_disable_enable_cycle() {
        let profiler = RuntimeProfiler::new(100);

        // Record initial
        profiler.record_call(SymbolId(1));
        profiler.record_event(ProfilingEvent::ExpressionCompiled);

        // Disable and attempt to record (should be ignored)
        profiler.disable();
        profiler.record_call(SymbolId(2));
        profiler.record_event(ProfilingEvent::ExpressionCompiled);

        // Re-enable and record
        profiler.enable();
        profiler.record_call(SymbolId(1));

        let stats = profiler.get_stats();
        let frequencies = profiler.get_call_frequencies();

        // Should only have data from enabled periods
        assert_eq!(stats.expressions_compiled, 1); // Not 2
        assert_eq!(frequencies.get(&SymbolId(1)), Some(&2)); // Got both
        assert!(!frequencies.contains_key(&SymbolId(2))); // Disabled period ignored
    }
}
