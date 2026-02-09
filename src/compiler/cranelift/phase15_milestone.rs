// Phase 15: Escape Analysis (Milestone Tests)
#[cfg(test)]
mod tests {
    use crate::compiler::cranelift::escape_analyzer::*;
    use crate::value::SymbolId;

    #[test]
    fn phase15_escape_state_variants() {
        assert!(EscapeState::NoEscape.allows_stack_allocation());
        assert!(!EscapeState::GlobalEscape.allows_stack_allocation());
    }

    #[test]
    fn phase15_escape_state_safety() {
        assert_eq!(EscapeState::NoEscape.safety_level(), 1.0);
        assert_eq!(EscapeState::GlobalEscape.safety_level(), 0.0);
    }

    #[test]
    fn phase15_escape_combine() {
        let result = EscapeState::NoEscape.combine(EscapeState::GlobalEscape);
        assert_eq!(result, EscapeState::GlobalEscape);
    }

    #[test]
    fn phase15_allocation_profile_basic() {
        let mut profile = AllocationProfile::new();
        profile.record_allocation(128);
        assert_eq!(profile.allocation_count, 1);
        assert_eq!(profile.total_cost, 128);
    }

    #[test]
    fn phase15_allocation_profile_finalize() {
        let mut profile = AllocationProfile::new();
        profile.record_allocation(256);
        profile.finalize();
        assert!(profile.can_stack_allocate());
    }

    #[test]
    fn phase15_allocation_profile_escape_to_args() {
        let mut profile = AllocationProfile::new();
        profile.record_allocation(128);
        profile.escapes_to_args = true;
        profile.finalize();
        assert!(!profile.can_stack_allocate());
    }

    #[test]
    fn phase15_allocation_savings() {
        let mut profile = AllocationProfile::new();
        profile.record_allocation(100);
        profile.record_allocation(200);
        profile.finalize();
        assert_eq!(profile.stack_allocation_savings(), 300);
    }

    #[test]
    fn phase15_analysis_result_creation() {
        let result = EscapeAnalysisResult::new(SymbolId(1), 3);
        assert_eq!(result.func, SymbolId(1));
        assert_eq!(result.parameter_escape.len(), 3);
    }

    #[test]
    fn phase15_analysis_result_opportunities() {
        let mut result = EscapeAnalysisResult::new(SymbolId(1), 1);
        result.optimizable_allocations = 5;
        result.potential_savings = 500;
        assert!(result.has_opportunities());
    }

    #[test]
    fn phase15_analyzer_creation() {
        let analyzer = EscapeAnalyzer::new();
        let stats = analyzer.get_stats();
        assert_eq!(stats.functions_analyzed, 0);
    }

    #[test]
    fn phase15_analyzer_record() {
        let mut analyzer = EscapeAnalyzer::new();
        analyzer.record_parameter_allocation(SymbolId(1), 0, 128);
        analyzer.record_parameter_allocation(SymbolId(1), 0, 256);

        let result = analyzer.analyze(SymbolId(1), 1);
        assert_eq!(result.optimizable_allocations, 2);
    }

    #[test]
    fn phase15_analyzer_escape_to_args() {
        let mut analyzer = EscapeAnalyzer::new();
        analyzer.record_parameter_allocation(SymbolId(1), 0, 128);
        analyzer.mark_escape_to_args(SymbolId(1), 0);

        let result = analyzer.analyze(SymbolId(1), 1);
        assert_eq!(result.optimizable_allocations, 0);
    }

    #[test]
    fn phase15_analyzer_escape_to_global() {
        let mut analyzer = EscapeAnalyzer::new();
        analyzer.record_parameter_allocation(SymbolId(1), 0, 128);
        analyzer.mark_escape_to_global(SymbolId(1), 0);

        let result = analyzer.analyze(SymbolId(1), 1);
        assert_eq!(result.optimizable_allocations, 0);
    }

    #[test]
    fn phase15_analyzer_returned() {
        let mut analyzer = EscapeAnalyzer::new();
        analyzer.record_parameter_allocation(SymbolId(1), 0, 128);
        analyzer.mark_parameter_returned(SymbolId(1), 0);

        let result = analyzer.analyze(SymbolId(1), 1);
        assert_eq!(result.optimizable_allocations, 0);
    }

    #[test]
    fn phase15_analyzer_multiple_functions() {
        let mut analyzer = EscapeAnalyzer::new();
        analyzer.record_parameter_allocation(SymbolId(1), 0, 100);
        analyzer.record_parameter_allocation(SymbolId(2), 0, 200);

        analyzer.analyze(SymbolId(1), 1);
        analyzer.analyze(SymbolId(2), 1);

        let stats = analyzer.get_stats();
        assert_eq!(stats.functions_analyzed, 2);
    }

    #[test]
    fn phase15_analyzer_get_optimizable() {
        let mut analyzer = EscapeAnalyzer::new();
        analyzer.record_parameter_allocation(SymbolId(1), 0, 128);
        analyzer.mark_escape_to_args(SymbolId(1), 0);
        analyzer.record_parameter_allocation(SymbolId(2), 0, 256);

        analyzer.analyze(SymbolId(1), 1);
        analyzer.analyze(SymbolId(2), 1);

        let optimizable = analyzer.get_optimizable_functions();
        assert_eq!(optimizable.len(), 1);
        assert_eq!(optimizable[0], SymbolId(2));
    }

    #[test]
    fn phase15_stats_coverage() {
        let stats = EscapeAnalysisStats {
            functions_analyzed: 10,
            functions_with_opportunities: 3,
            total_allocations: 50,
            total_optimizable_allocations: 15,
            total_potential_savings: 1500,
        };

        assert_eq!(stats.opportunity_coverage(), 0.3);
        assert_eq!(stats.optimization_ratio(), 0.3);
        assert_eq!(stats.avg_savings_per_allocation(), 100.0);
    }

    #[test]
    fn phase15_workflow_no_escape() {
        let mut analyzer = EscapeAnalyzer::new();

        for i in 0..5 {
            analyzer.record_parameter_allocation(SymbolId(1), 0, 100 + i * 10);
        }

        let result = analyzer.analyze(SymbolId(1), 1);
        assert!(result.has_opportunities());
        assert_eq!(result.optimizable_allocations, 5);
    }

    #[test]
    fn phase15_workflow_mixed_escape() {
        let mut analyzer = EscapeAnalyzer::new();

        analyzer.record_parameter_allocation(SymbolId(1), 0, 100);
        analyzer.record_parameter_allocation(SymbolId(1), 1, 200);

        analyzer.mark_escape_to_global(SymbolId(1), 0);

        let result = analyzer.analyze(SymbolId(1), 2);
        assert_eq!(result.parameter_escape[0], EscapeState::GlobalEscape);
        assert_eq!(result.parameter_escape[1], EscapeState::NoEscape);
    }

    #[test]
    fn phase15_efficiency_calculation() {
        let mut profile = AllocationProfile::new();
        profile.record_allocation(50);
        profile.record_allocation(50);
        profile.finalize();

        assert_eq!(profile.allocation_efficiency(), 1.0);
    }

    #[test]
    fn phase15_default_escape_state() {
        let state = EscapeState::default();
        assert_eq!(state, EscapeState::Unknown);
    }

    #[test]
    fn phase15_default_profile() {
        let profile = AllocationProfile::default();
        assert_eq!(profile.allocation_count, 0);
    }

    #[test]
    fn phase15_default_analyzer() {
        let analyzer = EscapeAnalyzer::default();
        assert_eq!(analyzer.get_stats().functions_analyzed, 0);
    }

    #[test]
    fn phase15_analyzer_get_parameter_escape() {
        let mut analyzer = EscapeAnalyzer::new();
        analyzer.record_parameter_allocation(SymbolId(1), 0, 128);
        analyzer.analyze(SymbolId(1), 1);

        let escape = analyzer.get_parameter_escape(SymbolId(1), 0);
        assert_eq!(escape, EscapeState::NoEscape);
    }
}
