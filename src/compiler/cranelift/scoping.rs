// Variable scoping and binding management for Phase 5
//
// Manages variable bindings during compilation, including:
// - Let binding creation and lookup
// - Nested scope stacks
// - Variable resolution at compile-time
// - Stack slot allocation

use crate::value::SymbolId;
use std::collections::HashMap;

/// Represents a variable binding in the current scope
#[derive(Debug, Clone)]
pub struct Binding {
    /// The symbol ID of the variable
    pub sym_id: SymbolId,
    /// The depth at which this binding was created (0 = global, increases with nesting)
    pub depth: usize,
    /// Index within the current depth level
    pub index: usize,
}

/// Manages variable scopes during compilation
/// Phase 5: Enables variable binding and let-expression compilation
pub struct ScopeManager {
    /// Stack of scope levels (each level is a map of symbol_id -> binding)
    scopes: Vec<HashMap<SymbolId, Binding>>,
    /// Current depth in the scope stack
    current_depth: usize,
}

impl ScopeManager {
    /// Create a new scope manager with global scope
    pub fn new() -> Self {
        ScopeManager {
            scopes: vec![HashMap::new()], // Start with global scope
            current_depth: 0,
        }
    }

    /// Enter a new scope (for let bindings, function parameters, etc.)
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.current_depth += 1;
    }

    /// Exit the current scope
    pub fn pop_scope(&mut self) -> Result<(), String> {
        if self.current_depth == 0 {
            return Err("Cannot pop global scope".to_string());
        }
        self.scopes.pop();
        self.current_depth -= 1;
        Ok(())
    }

    /// Bind a variable in the current scope
    /// Returns the binding information (depth, index)
    pub fn bind(&mut self, sym_id: SymbolId) -> (usize, usize) {
        let current_scope = &mut self.scopes[self.current_depth];
        let index = current_scope.len();
        let binding = Binding {
            sym_id,
            depth: self.current_depth,
            index,
        };
        current_scope.insert(sym_id, binding);
        (self.current_depth, index)
    }

    /// Look up a variable binding
    /// Returns (depth, index) if found, None otherwise
    pub fn lookup(&self, sym_id: SymbolId) -> Option<(usize, usize)> {
        // Search from current scope backwards to global
        for depth in (0..=self.current_depth).rev() {
            if let Some(binding) = self.scopes[depth].get(&sym_id) {
                return Some((binding.depth, binding.index));
            }
        }
        None
    }

    /// Check if a variable is bound in current or outer scopes
    pub fn is_bound(&self, sym_id: SymbolId) -> bool {
        self.lookup(sym_id).is_some()
    }

    /// Get the current scope depth
    pub fn current_depth(&self) -> usize {
        self.current_depth
    }

    /// Get the number of bindings in the current scope
    pub fn current_scope_size(&self) -> usize {
        self.scopes[self.current_depth].len()
    }

    /// Get all bindings in the current scope
    pub fn current_bindings(&self) -> Vec<(SymbolId, usize, usize)> {
        self.scopes[self.current_depth]
            .iter()
            .map(|(sym_id, binding)| (*sym_id, binding.depth, binding.index))
            .collect()
    }
}

impl Default for ScopeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_manager_creation() {
        let manager = ScopeManager::new();
        assert_eq!(manager.current_depth(), 0);
    }

    #[test]
    fn test_bind_in_global_scope() {
        let mut manager = ScopeManager::new();
        let sym = SymbolId(1);

        let (depth, index) = manager.bind(sym);
        assert_eq!(depth, 0);
        assert_eq!(index, 0);
    }

    #[test]
    fn test_lookup_variable() {
        let mut manager = ScopeManager::new();
        let sym = SymbolId(1);

        manager.bind(sym);

        let result = manager.lookup(sym);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), (0, 0));
    }

    #[test]
    fn test_lookup_nonexistent_variable() {
        let manager = ScopeManager::new();
        let sym = SymbolId(999);

        assert!(manager.lookup(sym).is_none());
    }

    #[test]
    fn test_push_and_pop_scope() {
        let mut manager = ScopeManager::new();

        assert_eq!(manager.current_depth(), 0);

        manager.push_scope();
        assert_eq!(manager.current_depth(), 1);

        manager.pop_scope().unwrap();
        assert_eq!(manager.current_depth(), 0);
    }

    #[test]
    fn test_nested_scope_binding() {
        let mut manager = ScopeManager::new();
        let sym1 = SymbolId(1);
        let sym2 = SymbolId(2);

        // Bind in global scope
        manager.bind(sym1);

        // Push new scope and bind variable
        manager.push_scope();
        manager.bind(sym2);

        // sym2 should be found in current scope
        assert_eq!(manager.lookup(sym2), Some((1, 0)));

        // sym1 should still be found in outer scope
        assert_eq!(manager.lookup(sym1), Some((0, 0)));
    }

    #[test]
    fn test_variable_shadowing() {
        let mut manager = ScopeManager::new();
        let sym = SymbolId(1);

        // Bind in global scope
        manager.bind(sym);
        assert_eq!(manager.lookup(sym), Some((0, 0)));

        // Push scope and rebind same variable
        manager.push_scope();
        manager.bind(sym);

        // Lookup should find inner binding
        assert_eq!(manager.lookup(sym), Some((1, 0)));

        // Pop scope, should find outer binding
        manager.pop_scope().unwrap();
        assert_eq!(manager.lookup(sym), Some((0, 0)));
    }

    #[test]
    fn test_multiple_bindings_in_scope() {
        let mut manager = ScopeManager::new();
        let sym1 = SymbolId(1);
        let sym2 = SymbolId(2);
        let sym3 = SymbolId(3);

        manager.bind(sym1);
        manager.bind(sym2);
        manager.bind(sym3);

        assert_eq!(manager.current_scope_size(), 3);
        assert_eq!(manager.lookup(sym1), Some((0, 0)));
        assert_eq!(manager.lookup(sym2), Some((0, 1)));
        assert_eq!(manager.lookup(sym3), Some((0, 2)));
    }

    #[test]
    fn test_cannot_pop_global_scope() {
        let mut manager = ScopeManager::new();

        let result = manager.pop_scope();
        assert!(result.is_err());
    }
}
