// Phase 12: Runtime Profiling Collection & Integration
//
// Integrates profiling collection into the actual compilation pipeline:
// - Function call tracking
// - Compilation event recording
// - Optimization tracking
// - Profiling data aggregation
// - Thread-safe profiler state management
// - Periodic profiling snapshots

use crate::value::SymbolId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// JIT compilation statistics
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

/// Profiling event types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfilingEvent {
    /// Function was called
    FunctionCall,
    /// Expression was compiled
    ExpressionCompiled,
    /// Function was compiled
    FunctionCompiled,
    /// Optimization pass was run
    OptimizationPass,
    /// Dead code was eliminated
    DeadCodeEliminated,
    /// Constant was propagated
    ConstantPropagated,
    /// Tail call was optimized
    TailCallOptimized,
    /// Function was inlined
    FunctionInlined,
}

impl ProfilingEvent {
    /// Get the weight/cost of this event for sampling
    pub fn weight(&self) -> usize {
        match self {
            ProfilingEvent::FunctionCall => 1,
            ProfilingEvent::ExpressionCompiled => 5,
            ProfilingEvent::FunctionCompiled => 10,
            ProfilingEvent::OptimizationPass => 2,
            ProfilingEvent::DeadCodeEliminated => 1,
            ProfilingEvent::ConstantPropagated => 1,
            ProfilingEvent::TailCallOptimized => 3,
            ProfilingEvent::FunctionInlined => 4,
        }
    }
}

/// A profiling sample at a point in time
#[derive(Debug, Clone)]
pub struct ProfilingSnapshot {
    /// Timestamp (seconds since epoch)
    pub timestamp: u64,
    /// Event counts for this snapshot
    pub events: HashMap<ProfilingEvent, usize>,
    /// Current JIT statistics
    pub stats: JitStats,
    /// Call frequencies at this point
    pub call_frequencies: HashMap<SymbolId, usize>,
}

impl ProfilingSnapshot {
    /// Create a new profiling snapshot
    pub fn new(stats: JitStats, call_frequencies: HashMap<SymbolId, usize>) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        ProfilingSnapshot {
            timestamp,
            events: HashMap::new(),
            stats,
            call_frequencies,
        }
    }

    /// Get the total event count in this snapshot
    pub fn total_events(&self) -> usize {
        self.events.values().sum()
    }

    /// Get the weighted event count
    pub fn weighted_events(&self) -> usize {
        self.events
            .iter()
            .map(|(event, count)| event.weight() * count)
            .sum()
    }
}

/// Thread-safe runtime profiler
pub struct RuntimeProfiler {
    /// Current statistics being collected
    stats: Arc<Mutex<JitStats>>,
    /// Call frequencies being tracked
    call_frequencies: Arc<Mutex<HashMap<SymbolId, usize>>>,
    /// Historical snapshots
    snapshots: Arc<Mutex<Vec<ProfilingSnapshot>>>,
    /// Maximum number of snapshots to keep
    max_snapshots: usize,
    /// Whether profiling is enabled
    enabled: Arc<Mutex<bool>>,
}

impl RuntimeProfiler {
    /// Create a new runtime profiler
    pub fn new(max_snapshots: usize) -> Self {
        RuntimeProfiler {
            stats: Arc::new(Mutex::new(JitStats::new())),
            call_frequencies: Arc::new(Mutex::new(HashMap::new())),
            snapshots: Arc::new(Mutex::new(Vec::new())),
            max_snapshots,
            enabled: Arc::new(Mutex::new(true)),
        }
    }

    /// Enable profiling
    pub fn enable(&self) {
        if let Ok(mut enabled) = self.enabled.lock() {
            *enabled = true;
        }
    }

    /// Disable profiling
    pub fn disable(&self) {
        if let Ok(mut enabled) = self.enabled.lock() {
            *enabled = false;
        }
    }

    /// Check if profiling is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.lock().map(|e| *e).unwrap_or(true)
    }

    /// Record a function call
    pub fn record_call(&self, func: SymbolId) {
        if !self.is_enabled() {
            return;
        }

        if let Ok(mut stats) = self.stats.lock() {
            stats.record_call(func);
        }

        if let Ok(mut frequencies) = self.call_frequencies.lock() {
            *frequencies.entry(func).or_insert(0) += 1;
        }
    }

    /// Record a compilation event
    pub fn record_event(&self, event: ProfilingEvent) {
        if !self.is_enabled() {
            return;
        }

        if let Ok(mut stats) = self.stats.lock() {
            match event {
                ProfilingEvent::FunctionCall => {} // Handled by record_call
                ProfilingEvent::ExpressionCompiled => stats.record_expression(),
                ProfilingEvent::FunctionCompiled => stats.record_function(),
                ProfilingEvent::OptimizationPass => stats.optimization_passes += 1,
                ProfilingEvent::DeadCodeEliminated => stats.dead_code_eliminated += 1,
                ProfilingEvent::ConstantPropagated => stats.constants_propagated += 1,
                ProfilingEvent::TailCallOptimized => stats.tail_calls_optimized += 1,
                ProfilingEvent::FunctionInlined => stats.functions_inlined += 1,
            }
        }
    }

    /// Record code size generation
    pub fn record_code_size(&self, size: usize) {
        if !self.is_enabled() {
            return;
        }

        if let Ok(mut stats) = self.stats.lock() {
            stats.code_size += size;
        }
    }

    /// Get current statistics
    pub fn get_stats(&self) -> JitStats {
        self.stats.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Get current call frequencies
    pub fn get_call_frequencies(&self) -> HashMap<SymbolId, usize> {
        self.call_frequencies
            .lock()
            .map(|f| f.clone())
            .unwrap_or_default()
    }

    /// Take a snapshot of current profiling state
    pub fn snapshot(&self) -> ProfilingSnapshot {
        let stats = self.get_stats();
        let frequencies = self.get_call_frequencies();

        let snapshot = ProfilingSnapshot::new(stats, frequencies);

        // Store snapshot if enabled
        if let Ok(mut snapshots) = self.snapshots.lock() {
            snapshots.push(snapshot.clone());

            // Maintain max snapshot limit
            if snapshots.len() > self.max_snapshots {
                snapshots.remove(0);
            }
        }

        snapshot
    }

    /// Get all snapshots
    pub fn get_snapshots(&self) -> Vec<ProfilingSnapshot> {
        self.snapshots.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Clear all profiling data
    pub fn clear(&self) {
        if let Ok(mut stats) = self.stats.lock() {
            *stats = JitStats::new();
        }
        if let Ok(mut frequencies) = self.call_frequencies.lock() {
            frequencies.clear();
        }
        if let Ok(mut snapshots) = self.snapshots.lock() {
            snapshots.clear();
        }
    }

    /// Get profiling summary
    pub fn summary(&self) -> ProfilingSummary {
        let stats = self.get_stats();
        let frequencies = self.get_call_frequencies();
        let snapshots = self.get_snapshots();

        let hot_functions = frequencies
            .iter()
            .filter(|(_, freq)| **freq > 5)
            .map(|(f, freq)| (*f, *freq))
            .collect();

        ProfilingSummary {
            total_functions_compiled: stats.functions_compiled,
            total_expressions_compiled: stats.expressions_compiled,
            total_optimizations: stats.total_optimizations(),
            optimization_ratio: stats.optimization_ratio(),
            hot_functions,
            total_snapshots: snapshots.len(),
            code_size: stats.code_size,
        }
    }

    /// Check if function is hot (called more than threshold)
    pub fn is_function_hot(&self, func: SymbolId, threshold: usize) -> bool {
        self.get_call_frequencies()
            .get(&func)
            .map(|freq| *freq > threshold)
            .unwrap_or(false)
    }

    /// Get hot functions
    pub fn get_hot_functions(&self, threshold: usize) -> Vec<(SymbolId, usize)> {
        let frequencies = self.get_call_frequencies();
        let mut hot: Vec<_> = frequencies
            .into_iter()
            .filter(|(_, freq)| *freq > threshold)
            .collect();
        hot.sort_by(|a, b| b.1.cmp(&a.1));
        hot
    }
}

impl Clone for RuntimeProfiler {
    fn clone(&self) -> Self {
        RuntimeProfiler {
            stats: Arc::clone(&self.stats),
            call_frequencies: Arc::clone(&self.call_frequencies),
            snapshots: Arc::clone(&self.snapshots),
            max_snapshots: self.max_snapshots,
            enabled: Arc::clone(&self.enabled),
        }
    }
}

/// Summary of profiling data
#[derive(Debug, Clone)]
pub struct ProfilingSummary {
    /// Total functions compiled
    pub total_functions_compiled: usize,
    /// Total expressions compiled
    pub total_expressions_compiled: usize,
    /// Total optimizations applied
    pub total_optimizations: usize,
    /// Optimization ratio (0.0 to 1.0)
    pub optimization_ratio: f64,
    /// Hot functions (func_id, call_count)
    pub hot_functions: Vec<(SymbolId, usize)>,
    /// Number of snapshots taken
    pub total_snapshots: usize,
    /// Total code size generated
    pub code_size: usize,
}

impl ProfilingSummary {
    /// Check if there are any hot functions
    pub fn has_hot_functions(&self) -> bool {
        !self.hot_functions.is_empty()
    }

    /// Get the hottest function
    pub fn hottest_function(&self) -> Option<(SymbolId, usize)> {
        self.hot_functions.first().copied()
    }

    /// Calculate average optimization benefit
    pub fn avg_optimization_benefit(&self) -> f64 {
        if self.total_expressions_compiled == 0 {
            0.0
        } else {
            self.total_optimizations as f64 / self.total_expressions_compiled as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase12_profiling_event_weight() {
        assert_eq!(ProfilingEvent::FunctionCall.weight(), 1);
        assert_eq!(ProfilingEvent::FunctionCompiled.weight(), 10);
        assert!(ProfilingEvent::FunctionInlined.weight() > ProfilingEvent::FunctionCall.weight());
    }

    #[test]
    fn phase12_snapshot_creation() {
        let stats = JitStats::new();
        let frequencies = HashMap::new();

        let snapshot = ProfilingSnapshot::new(stats, frequencies);

        assert_eq!(snapshot.total_events(), 0);
    }

    #[test]
    fn phase12_profiler_creation() {
        let profiler = RuntimeProfiler::new(100);

        assert!(profiler.is_enabled());
    }

    #[test]
    fn phase12_profiler_enable_disable() {
        let profiler = RuntimeProfiler::new(100);

        profiler.disable();
        assert!(!profiler.is_enabled());

        profiler.enable();
        assert!(profiler.is_enabled());
    }

    #[test]
    fn phase12_profiler_record_call() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));
        profiler.record_call(SymbolId(1));
        profiler.record_call(SymbolId(2));

        let frequencies = profiler.get_call_frequencies();
        assert_eq!(frequencies.get(&SymbolId(1)), Some(&2));
        assert_eq!(frequencies.get(&SymbolId(2)), Some(&1));
    }

    #[test]
    fn phase12_profiler_record_event() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_event(ProfilingEvent::ExpressionCompiled);
        profiler.record_event(ProfilingEvent::FunctionCompiled);

        let stats = profiler.get_stats();
        assert_eq!(stats.expressions_compiled, 1);
        assert_eq!(stats.functions_compiled, 1);
    }

    #[test]
    fn phase12_profiler_code_size() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_code_size(100);
        profiler.record_code_size(50);

        let stats = profiler.get_stats();
        assert_eq!(stats.code_size, 150);
    }

    #[test]
    fn phase12_profiler_snapshot() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));
        profiler.record_event(ProfilingEvent::ExpressionCompiled);

        let snapshot = profiler.snapshot();
        assert_eq!(snapshot.stats.expressions_compiled, 1);

        let snapshots = profiler.get_snapshots();
        assert_eq!(snapshots.len(), 1);
    }

    #[test]
    fn phase12_profiler_max_snapshots() {
        let profiler = RuntimeProfiler::new(3);

        for _ in 0..5 {
            profiler.snapshot();
        }

        let snapshots = profiler.get_snapshots();
        assert_eq!(snapshots.len(), 3);
    }

    #[test]
    fn phase12_profiler_clear() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));
        profiler.record_event(ProfilingEvent::ExpressionCompiled);
        profiler.snapshot();

        profiler.clear();

        let stats = profiler.get_stats();
        let frequencies = profiler.get_call_frequencies();
        let snapshots = profiler.get_snapshots();

        assert_eq!(stats.expressions_compiled, 0);
        assert!(frequencies.is_empty());
        assert!(snapshots.is_empty());
    }

    #[test]
    fn phase12_profiler_is_function_hot() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));
        for _ in 0..10 {
            profiler.record_call(SymbolId(1));
        }
        profiler.record_call(SymbolId(2));

        assert!(profiler.is_function_hot(SymbolId(1), 5));
        assert!(!profiler.is_function_hot(SymbolId(2), 5));
    }

    #[test]
    fn phase12_profiler_get_hot_functions() {
        let profiler = RuntimeProfiler::new(100);

        for _ in 0..10 {
            profiler.record_call(SymbolId(1));
        }
        for _ in 0..5 {
            profiler.record_call(SymbolId(2));
        }
        profiler.record_call(SymbolId(3));

        let hot = profiler.get_hot_functions(3);
        assert_eq!(hot.len(), 2);
        assert_eq!(hot[0].0, SymbolId(1)); // Most hot first
    }

    #[test]
    fn phase12_profiler_clone() {
        let profiler = RuntimeProfiler::new(100);
        profiler.record_call(SymbolId(1));

        let cloned = profiler.clone();
        let frequencies = cloned.get_call_frequencies();

        assert_eq!(frequencies.get(&SymbolId(1)), Some(&1));
    }

    #[test]
    fn phase12_profiler_disable_disables_recording() {
        let profiler = RuntimeProfiler::new(100);

        profiler.record_call(SymbolId(1));
        profiler.disable();
        profiler.record_call(SymbolId(1));

        let frequencies = profiler.get_call_frequencies();
        assert_eq!(frequencies.get(&SymbolId(1)), Some(&1));
    }

    #[test]
    fn phase12_profiling_summary_creation() {
        let mut frequencies = HashMap::new();
        frequencies.insert(SymbolId(1), 10);
        frequencies.insert(SymbolId(2), 3);

        let mut stats = JitStats::new();
        stats.record_expression();
        stats.record_expression();
        stats.record_function();
        stats.dead_code_eliminated = 1;

        let summary = ProfilingSummary {
            total_functions_compiled: stats.functions_compiled,
            total_expressions_compiled: stats.expressions_compiled,
            total_optimizations: 1,
            optimization_ratio: 0.5,
            hot_functions: vec![(SymbolId(1), 10)],
            total_snapshots: 0,
            code_size: 1024,
        };

        assert!(summary.has_hot_functions());
        assert_eq!(summary.hottest_function(), Some((SymbolId(1), 10)));
    }

    #[test]
    fn phase12_profiling_summary_no_hot() {
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
    }

    #[test]
    fn phase12_profiler_summary() {
        let profiler = RuntimeProfiler::new(100);

        for _ in 0..10 {
            profiler.record_call(SymbolId(1));
        }
        profiler.record_event(ProfilingEvent::ExpressionCompiled);
        profiler.record_event(ProfilingEvent::FunctionCompiled);

        let summary = profiler.summary();

        assert_eq!(summary.total_functions_compiled, 1);
        assert_eq!(summary.total_expressions_compiled, 1);
        assert!(summary.has_hot_functions());
    }

    #[test]
    fn phase12_snapshot_weighted_events() {
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
}
