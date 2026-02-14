// Phase 15: Escape Analysis & Allocation Optimization
//
// Analyzes which allocations can be stack-allocated or eliminated:
// - Escape state tracking (does value escape?)
// - Allocation profiling and elimination analysis
// - Stack allocation opportunities
// - Memory optimization recommendations
// - Allocation cost tracking

use crate::value::SymbolId;
use std::collections::HashMap;

/// Escape state of a value
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EscapeState {
    /// Value doesn't escape scope (can be stack-allocated)
    NoEscape,
    /// Value escapes to caller (might need heap)
    ArgsEscape,
    /// Value escapes to global state
    GlobalEscape,
    /// Value escapes in unknown ways (assume worst case)
    #[default]
    Unknown,
}

impl EscapeState {
    /// Check if this state allows stack allocation
    pub fn allows_stack_allocation(&self) -> bool {
        matches!(self, EscapeState::NoEscape)
    }

    /// Check if this state requires heap allocation
    pub fn requires_heap(&self) -> bool {
        !matches!(self, EscapeState::NoEscape)
    }

    /// Get the safety level (0.0 to 1.0, higher = safer for stack allocation)
    pub fn safety_level(&self) -> f64 {
        match self {
            EscapeState::NoEscape => 1.0,
            EscapeState::ArgsEscape => 0.3,
            EscapeState::GlobalEscape => 0.0,
            EscapeState::Unknown => 0.2,
        }
    }

    /// Combine two escape states (worst case)
    pub fn combine(self, other: EscapeState) -> EscapeState {
        match (self, other) {
            (EscapeState::GlobalEscape, _) | (_, EscapeState::GlobalEscape) => {
                EscapeState::GlobalEscape
            }
            (EscapeState::Unknown, _) | (_, EscapeState::Unknown) => EscapeState::Unknown,
            (EscapeState::ArgsEscape, _) | (_, EscapeState::ArgsEscape) => EscapeState::ArgsEscape,
            _ => EscapeState::NoEscape,
        }
    }
}

/// Profile of allocations for a function parameter or return value
#[derive(Debug, Clone)]
pub struct AllocationProfile {
    /// Escape state of this value
    pub escape_state: EscapeState,
    /// Estimated allocation size (bytes)
    pub estimated_size: usize,
    /// Number of times allocated
    pub allocation_count: usize,
    /// Whether it escapes to function arguments
    pub escapes_to_args: bool,
    /// Whether it escapes to global state
    pub escapes_to_global: bool,
    /// Whether it's returned from function
    pub is_returned: bool,
    /// Estimated total allocation cost
    pub total_cost: usize,
}

impl AllocationProfile {
    /// Create new allocation profile
    pub fn new() -> Self {
        AllocationProfile {
            escape_state: EscapeState::Unknown,
            estimated_size: 0,
            allocation_count: 0,
            escapes_to_args: false,
            escapes_to_global: false,
            is_returned: false,
            total_cost: 0,
        }
    }

    /// Record an allocation
    pub fn record_allocation(&mut self, size: usize) {
        self.allocation_count += 1;
        self.total_cost += size;
        if self.estimated_size < size {
            self.estimated_size = size;
        }
    }

    /// Finalize the profile based on escape information
    pub fn finalize(&mut self) {
        let escape_state = if self.escapes_to_global {
            EscapeState::GlobalEscape
        } else if self.escapes_to_args || self.is_returned {
            EscapeState::ArgsEscape
        } else if self.allocation_count > 0 {
            EscapeState::NoEscape
        } else {
            EscapeState::Unknown
        };

        self.escape_state = escape_state;
    }

    /// Check if this can be stack-allocated
    pub fn can_stack_allocate(&self) -> bool {
        self.escape_state.allows_stack_allocation()
    }

    /// Calculate potential savings from stack allocation
    pub fn stack_allocation_savings(&self) -> usize {
        if self.can_stack_allocate() {
            self.total_cost
        } else {
            0
        }
    }

    /// Get allocation efficiency (savings / total_cost)
    pub fn allocation_efficiency(&self) -> f64 {
        if self.total_cost == 0 {
            0.0
        } else {
            self.stack_allocation_savings() as f64 / self.total_cost as f64
        }
    }
}

impl Default for AllocationProfile {
    fn default() -> Self {
        Self::new()
    }
}

/// Escape analysis result for a function
#[derive(Debug, Clone)]
pub struct EscapeAnalysisResult {
    /// Function ID
    pub func: SymbolId,
    /// Escape states for parameters
    pub parameter_escape: Vec<EscapeState>,
    /// Escape state for return value
    pub return_escape: EscapeState,
    /// Total allocations that can be optimized
    pub optimizable_allocations: usize,
    /// Total bytes that could be saved
    pub potential_savings: usize,
}

impl EscapeAnalysisResult {
    /// Create new analysis result
    pub fn new(func: SymbolId, param_count: usize) -> Self {
        EscapeAnalysisResult {
            func,
            parameter_escape: vec![EscapeState::Unknown; param_count],
            return_escape: EscapeState::Unknown,
            optimizable_allocations: 0,
            potential_savings: 0,
        }
    }

    /// Get optimization potential (0.0 to 1.0)
    pub fn optimization_potential(&self) -> f64 {
        if self.potential_savings == 0 {
            0.0
        } else {
            (self.potential_savings as f64).min(1000.0) / 1000.0
        }
    }

    /// Check if this function has optimization opportunities
    pub fn has_opportunities(&self) -> bool {
        self.optimizable_allocations > 0 && self.potential_savings > 0
    }
}

/// Escape analyzer
pub struct EscapeAnalyzer {
    /// Allocation profiles per function
    function_profiles: HashMap<SymbolId, Vec<AllocationProfile>>,
    /// Analysis results per function
    analysis_results: HashMap<SymbolId, EscapeAnalysisResult>,
    /// Total allocations analyzed
    total_allocations: usize,
    /// Total potential savings identified
    total_potential_savings: usize,
}

impl EscapeAnalyzer {
    /// Create new escape analyzer
    pub fn new() -> Self {
        EscapeAnalyzer {
            function_profiles: HashMap::new(),
            analysis_results: HashMap::new(),
            total_allocations: 0,
            total_potential_savings: 0,
        }
    }

    /// Record an allocation for a function parameter
    pub fn record_parameter_allocation(&mut self, func: SymbolId, param_index: usize, size: usize) {
        let profiles = self.function_profiles.entry(func).or_default();

        while profiles.len() <= param_index {
            profiles.push(AllocationProfile::new());
        }

        profiles[param_index].record_allocation(size);
        self.total_allocations += 1;
    }

    /// Mark a parameter as escaping to caller
    pub fn mark_escape_to_args(&mut self, func: SymbolId, param_index: usize) {
        if let Some(profiles) = self.function_profiles.get_mut(&func) {
            if param_index < profiles.len() {
                profiles[param_index].escapes_to_args = true;
            }
        }
    }

    /// Mark a parameter as escaping to global state
    pub fn mark_escape_to_global(&mut self, func: SymbolId, param_index: usize) {
        if let Some(profiles) = self.function_profiles.get_mut(&func) {
            if param_index < profiles.len() {
                profiles[param_index].escapes_to_global = true;
            }
        }
    }

    /// Mark a parameter as returned
    pub fn mark_parameter_returned(&mut self, func: SymbolId, param_index: usize) {
        if let Some(profiles) = self.function_profiles.get_mut(&func) {
            if param_index < profiles.len() {
                profiles[param_index].is_returned = true;
            }
        }
    }

    /// Perform escape analysis for a function
    pub fn analyze(&mut self, func: SymbolId, param_count: usize) -> EscapeAnalysisResult {
        // Finalize all profiles
        if let Some(profiles) = self.function_profiles.get_mut(&func) {
            for profile in profiles.iter_mut() {
                profile.finalize();
            }
        }

        // Build result
        let mut result = EscapeAnalysisResult::new(func, param_count);

        if let Some(profiles) = self.function_profiles.get(&func) {
            for (i, profile) in profiles.iter().enumerate() {
                if i < param_count {
                    result.parameter_escape[i] = profile.escape_state;

                    if profile.can_stack_allocate() {
                        result.optimizable_allocations += profile.allocation_count;
                        result.potential_savings += profile.stack_allocation_savings();
                    }
                }
            }
        }

        self.total_potential_savings += result.potential_savings;
        self.analysis_results.insert(func, result.clone());

        result
    }

    /// Get analysis result for a function
    pub fn get_result(&self, func: SymbolId) -> Option<&EscapeAnalysisResult> {
        self.analysis_results.get(&func)
    }

    /// Get all functions with optimization opportunities
    pub fn get_optimizable_functions(&self) -> Vec<SymbolId> {
        self.analysis_results
            .iter()
            .filter(|(_, result)| result.has_opportunities())
            .map(|(func, _)| *func)
            .collect()
    }

    /// Get total analysis statistics
    pub fn get_stats(&self) -> EscapeAnalysisStats {
        let functions_analyzed = self.analysis_results.len();
        let functions_with_opportunities = self
            .analysis_results
            .values()
            .filter(|r| r.has_opportunities())
            .count();

        let total_optimizable = self
            .analysis_results
            .values()
            .map(|r| r.optimizable_allocations)
            .sum();

        EscapeAnalysisStats {
            functions_analyzed,
            functions_with_opportunities,
            total_allocations: self.total_allocations,
            total_optimizable_allocations: total_optimizable,
            total_potential_savings: self.total_potential_savings,
        }
    }

    /// Get escape state for a function parameter
    pub fn get_parameter_escape(&self, func: SymbolId, param_index: usize) -> EscapeState {
        self.analysis_results
            .get(&func)
            .and_then(|result| result.parameter_escape.get(param_index).copied())
            .unwrap_or(EscapeState::Unknown)
    }
}

impl Default for EscapeAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Escape analysis statistics
#[derive(Debug, Clone)]
pub struct EscapeAnalysisStats {
    /// Total functions analyzed
    pub functions_analyzed: usize,
    /// Functions with optimization opportunities
    pub functions_with_opportunities: usize,
    /// Total allocations encountered
    pub total_allocations: usize,
    /// Allocations that can be optimized
    pub total_optimizable_allocations: usize,
    /// Total bytes that could be saved
    pub total_potential_savings: usize,
}

impl EscapeAnalysisStats {
    /// Get opportunity coverage (functions with opportunities / total)
    pub fn opportunity_coverage(&self) -> f64 {
        if self.functions_analyzed == 0 {
            0.0
        } else {
            self.functions_with_opportunities as f64 / self.functions_analyzed as f64
        }
    }

    /// Get optimization ratio (optimizable / total allocations)
    pub fn optimization_ratio(&self) -> f64 {
        if self.total_allocations == 0 {
            0.0
        } else {
            self.total_optimizable_allocations as f64 / self.total_allocations as f64
        }
    }

    /// Get average savings per optimizable allocation
    pub fn avg_savings_per_allocation(&self) -> f64 {
        if self.total_optimizable_allocations == 0 {
            0.0
        } else {
            self.total_potential_savings as f64 / self.total_optimizable_allocations as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase15_escape_state_no_escape() {
        assert!(EscapeState::NoEscape.allows_stack_allocation());
        assert!(!EscapeState::NoEscape.requires_heap());
    }

    #[test]
    fn phase15_escape_state_args_escape() {
        assert!(!EscapeState::ArgsEscape.allows_stack_allocation());
        assert!(EscapeState::ArgsEscape.requires_heap());
    }

    #[test]
    fn phase15_escape_state_global_escape() {
        assert!(!EscapeState::GlobalEscape.allows_stack_allocation());
        assert!(EscapeState::GlobalEscape.requires_heap());
    }

    #[test]
    fn phase15_escape_state_safety_levels() {
        assert_eq!(EscapeState::NoEscape.safety_level(), 1.0);
        assert!(EscapeState::ArgsEscape.safety_level() < EscapeState::NoEscape.safety_level());
        assert_eq!(EscapeState::GlobalEscape.safety_level(), 0.0);
    }

    #[test]
    fn phase15_escape_state_combine_global() {
        let result = EscapeState::NoEscape.combine(EscapeState::GlobalEscape);
        assert_eq!(result, EscapeState::GlobalEscape);
    }

    #[test]
    fn phase15_escape_state_combine_args() {
        let result = EscapeState::NoEscape.combine(EscapeState::ArgsEscape);
        assert_eq!(result, EscapeState::ArgsEscape);
    }

    #[test]
    fn phase15_allocation_profile_creation() {
        let profile = AllocationProfile::new();
        assert_eq!(profile.allocation_count, 0);
        assert_eq!(profile.estimated_size, 0);
    }

    #[test]
    fn phase15_allocation_profile_record() {
        let mut profile = AllocationProfile::new();
        profile.record_allocation(128);
        profile.record_allocation(256);

        assert_eq!(profile.allocation_count, 2);
        assert_eq!(profile.estimated_size, 256);
        assert_eq!(profile.total_cost, 384);
    }

    #[test]
    fn phase15_allocation_profile_finalize_no_escape() {
        let mut profile = AllocationProfile::new();
        profile.record_allocation(128);
        profile.finalize();

        assert_eq!(profile.escape_state, EscapeState::NoEscape);
        assert!(profile.can_stack_allocate());
    }

    #[test]
    fn phase15_allocation_profile_finalize_args_escape() {
        let mut profile = AllocationProfile::new();
        profile.record_allocation(128);
        profile.escapes_to_args = true;
        profile.finalize();

        assert_eq!(profile.escape_state, EscapeState::ArgsEscape);
        assert!(!profile.can_stack_allocate());
    }

    #[test]
    fn phase15_allocation_profile_savings() {
        let mut profile = AllocationProfile::new();
        profile.record_allocation(100);
        profile.record_allocation(200);
        profile.finalize();

        assert_eq!(profile.stack_allocation_savings(), 300);
    }

    #[test]
    fn phase15_allocation_profile_efficiency() {
        let mut profile = AllocationProfile::new();
        profile.record_allocation(100);
        profile.finalize();

        let efficiency = profile.allocation_efficiency();
        assert_eq!(efficiency, 1.0);
    }

    #[test]
    fn phase15_escape_analysis_result_creation() {
        let result = EscapeAnalysisResult::new(SymbolId(1), 2);

        assert_eq!(result.func, SymbolId(1));
        assert_eq!(result.parameter_escape.len(), 2);
    }

    #[test]
    fn phase15_escape_analysis_result_potential() {
        let mut result = EscapeAnalysisResult::new(SymbolId(1), 1);
        result.optimizable_allocations = 5;
        result.potential_savings = 500;

        assert!(result.has_opportunities());
        assert!(result.optimization_potential() > 0.0);
    }

    #[test]
    fn phase15_escape_analyzer_creation() {
        let analyzer = EscapeAnalyzer::new();
        let stats = analyzer.get_stats();

        assert_eq!(stats.functions_analyzed, 0);
    }

    #[test]
    fn phase15_escape_analyzer_record_allocation() {
        let mut analyzer = EscapeAnalyzer::new();

        analyzer.record_parameter_allocation(SymbolId(1), 0, 128);
        analyzer.record_parameter_allocation(SymbolId(1), 0, 256);

        let result = analyzer.analyze(SymbolId(1), 1);
        assert_eq!(result.optimizable_allocations, 2);
    }

    #[test]
    fn phase15_escape_analyzer_mark_escape() {
        let mut analyzer = EscapeAnalyzer::new();

        analyzer.record_parameter_allocation(SymbolId(1), 0, 128);
        analyzer.mark_escape_to_args(SymbolId(1), 0);

        let result = analyzer.analyze(SymbolId(1), 1);
        assert_eq!(result.optimizable_allocations, 0);
    }

    #[test]
    fn phase15_escape_analyzer_stats() {
        let mut analyzer = EscapeAnalyzer::new();

        analyzer.record_parameter_allocation(SymbolId(1), 0, 100);
        analyzer.analyze(SymbolId(1), 1);

        let stats = analyzer.get_stats();
        assert_eq!(stats.functions_analyzed, 1);
        assert_eq!(stats.total_allocations, 1);
    }

    #[test]
    fn phase15_escape_analysis_stats_coverage() {
        let stats = EscapeAnalysisStats {
            functions_analyzed: 10,
            functions_with_opportunities: 3,
            total_allocations: 100,
            total_optimizable_allocations: 30,
            total_potential_savings: 3000,
        };

        assert_eq!(stats.opportunity_coverage(), 0.3);
    }

    #[test]
    fn phase15_escape_analysis_stats_optimization_ratio() {
        let stats = EscapeAnalysisStats {
            functions_analyzed: 10,
            functions_with_opportunities: 3,
            total_allocations: 100,
            total_optimizable_allocations: 30,
            total_potential_savings: 3000,
        };

        assert_eq!(stats.optimization_ratio(), 0.3);
    }

    #[test]
    fn phase15_escape_analysis_stats_avg_savings() {
        let stats = EscapeAnalysisStats {
            functions_analyzed: 10,
            functions_with_opportunities: 3,
            total_allocations: 100,
            total_optimizable_allocations: 10,
            total_potential_savings: 1000,
        };

        assert_eq!(stats.avg_savings_per_allocation(), 100.0);
    }
}
