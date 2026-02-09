// JIT Coordinator
//
// Coordinates opportunistic JIT compilation using the profiling feedback loop.
//
// Strategy D: Hybrid with runtime profiling
// 1. Start all code with bytecode compilation
// 2. Collect profiling data during execution
// 3. Identify "hot" functions that are called frequently
// 4. Recompile hot functions using JIT for better performance
// 5. Use feedback from Phase 13 (Feedback-Based Recompilation)

use crate::compiler::cranelift::profiler::RuntimeProfiler;
use crate::value::SymbolId;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// JIT Coordinator manages opportunistic JIT compilation
pub struct JitCoordinator {
    /// Runtime profiler for collecting execution data
    profiler: Arc<RuntimeProfiler>,
    /// Functions that have been JIT compiled
    jit_functions: Arc<Mutex<HashSet<SymbolId>>>,
    /// Threshold for considering a function "hot"
    hot_threshold: usize,
    /// Whether JIT mode is enabled
    enabled: bool,
}

impl JitCoordinator {
    /// Create a new JIT coordinator
    pub fn new(enabled: bool) -> Self {
        JitCoordinator {
            profiler: Arc::new(RuntimeProfiler::new(1000)),
            jit_functions: Arc::new(Mutex::new(HashSet::new())),
            hot_threshold: 10, // Compile after 10 invocations
            enabled,
        }
    }

    /// Check if JIT mode is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get reference to the profiler
    pub fn profiler(&self) -> Arc<RuntimeProfiler> {
        Arc::clone(&self.profiler)
    }

    /// Check if a function should be JIT compiled based on profiling
    pub fn should_jit_compile(&self, func: SymbolId) -> bool {
        if !self.enabled {
            return false;
        }

        // Skip if already compiled
        if let Ok(compiled) = self.jit_functions.lock() {
            if compiled.contains(&func) {
                return false;
            }
        }

        // Check call frequency
        let frequencies = self.profiler.get_call_frequencies();
        frequencies
            .get(&func)
            .map(|count| *count >= self.hot_threshold)
            .unwrap_or(false)
    }

    /// Mark a function as JIT compiled
    pub fn mark_jit_compiled(&self, func: SymbolId) {
        if let Ok(mut compiled) = self.jit_functions.lock() {
            compiled.insert(func);
        }
    }

    /// Get profiling statistics
    pub fn get_stats(&self) -> String {
        let summary = self.profiler.summary();
        let compiled_count = self.jit_functions.lock().map(|f| f.len()).unwrap_or(0);

        format!(
            "JIT Coordinator: {} hot functions identified, {} JIT compiled",
            summary.hot_functions.len(),
            compiled_count
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_coordinator_disabled() {
        let coordinator = JitCoordinator::new(false);
        assert!(!coordinator.is_enabled());
    }

    #[test]
    fn test_jit_coordinator_enabled() {
        let coordinator = JitCoordinator::new(true);
        assert!(coordinator.is_enabled());
    }

    #[test]
    fn test_jit_coordinator_stats() {
        let coordinator = JitCoordinator::new(true);
        let stats = coordinator.get_stats();
        assert!(stats.contains("JIT Coordinator"));
    }

    #[test]
    fn test_jit_coordinator_profiler() {
        let coordinator = JitCoordinator::new(true);
        let profiler = coordinator.profiler();
        assert!(profiler.is_enabled());
    }
}
