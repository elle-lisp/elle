//! Scope tracking for binding resolution
//!
//! Tracks variable bindings during AST traversal to enable
//! compile-time resolution of all variable references.

use crate::value::SymbolId;
use std::collections::HashMap;

/// Information about a bound variable
#[derive(Debug, Clone)]
pub struct Binding {
    /// Symbol ID of the variable
    pub sym: SymbolId,
    /// Index within the current frame (params and locals)
    pub index: usize,
    /// Whether this variable is captured by a nested lambda
    pub captured: bool,
    /// Whether this variable is mutated via set!
    pub mutated: bool,
}

/// A single lexical scope level
#[derive(Debug)]
pub struct Scope {
    /// Bindings in this scope, keyed by symbol
    bindings: HashMap<SymbolId, Binding>,
    /// Order of bindings (for deterministic index assignment)
    binding_order: Vec<SymbolId>,
    /// Whether this is a function scope (creates new activation frame)
    pub is_function: bool,
    /// Base index for new bindings (after captures/params in function scope)
    base_index: usize,
}

impl Scope {
    /// Create a new scope
    pub fn new(is_function: bool, base_index: usize) -> Self {
        Scope {
            bindings: HashMap::new(),
            binding_order: Vec::new(),
            is_function,
            base_index,
        }
    }

    /// Bind a new variable in this scope
    /// Returns the assigned index
    pub fn bind(&mut self, sym: SymbolId) -> usize {
        let index = self.base_index + self.binding_order.len();
        let binding = Binding {
            sym,
            index,
            captured: false,
            mutated: false,
        };
        self.bindings.insert(sym, binding);
        self.binding_order.push(sym);
        index
    }

    /// Look up a binding in this scope only
    pub fn get(&self, sym: SymbolId) -> Option<&Binding> {
        self.bindings.get(&sym)
    }

    /// Get mutable reference to a binding
    pub fn get_mut(&mut self, sym: SymbolId) -> Option<&mut Binding> {
        self.bindings.get_mut(&sym)
    }

    /// Check if a symbol is bound in this scope
    pub fn contains(&self, sym: SymbolId) -> bool {
        self.bindings.contains_key(&sym)
    }

    /// Get all bindings in order
    pub fn bindings_in_order(&self) -> impl Iterator<Item = &Binding> {
        self.binding_order
            .iter()
            .filter_map(|sym| self.bindings.get(sym))
    }

    /// Number of bindings in this scope
    pub fn len(&self) -> usize {
        self.binding_order.len()
    }

    /// Check if scope is empty
    pub fn is_empty(&self) -> bool {
        self.binding_order.is_empty()
    }
}

/// Stack of scopes for tracking nested lexical environments
#[derive(Debug)]
pub struct ScopeStack {
    scopes: Vec<Scope>,
}

impl ScopeStack {
    /// Create a new scope stack (starts empty, no global scope)
    pub fn new() -> Self {
        ScopeStack { scopes: Vec::new() }
    }

    /// Push a new scope
    pub fn push(&mut self, is_function: bool, base_index: usize) {
        self.scopes.push(Scope::new(is_function, base_index));
    }

    /// Pop the current scope
    pub fn pop(&mut self) -> Option<Scope> {
        self.scopes.pop()
    }

    /// Get current scope depth
    pub fn depth(&self) -> usize {
        self.scopes.len()
    }

    /// Bind a variable in the current scope
    /// Returns the assigned index, or None if no scope exists
    pub fn bind(&mut self, sym: SymbolId) -> Option<usize> {
        self.scopes.last_mut().map(|scope| scope.bind(sym))
    }

    /// Look up a variable, searching from innermost to outermost scope
    /// Returns (scope_index, binding) where scope_index is the position in the stack
    pub fn lookup(&self, sym: SymbolId) -> Option<(usize, &Binding)> {
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            if let Some(binding) = scope.get(sym) {
                return Some((i, binding));
            }
        }
        None
    }

    /// Mark a variable as captured (called when a nested lambda references it)
    pub fn mark_captured(&mut self, sym: SymbolId) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(binding) = scope.get_mut(sym) {
                binding.captured = true;
                return;
            }
        }
    }

    /// Mark a variable as mutated (called when set! is used)
    pub fn mark_mutated(&mut self, sym: SymbolId) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(binding) = scope.get_mut(sym) {
                binding.mutated = true;
                return;
            }
        }
    }

    /// Find the innermost function scope index
    /// Returns None if not inside any function
    pub fn innermost_function_scope(&self) -> Option<usize> {
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            if scope.is_function {
                return Some(i);
            }
        }
        None
    }

    /// Get the current scope (innermost)
    pub fn current(&self) -> Option<&Scope> {
        self.scopes.last()
    }

    /// Get mutable reference to current scope
    pub fn current_mut(&mut self) -> Option<&mut Scope> {
        self.scopes.last_mut()
    }
}

impl Default for ScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_bind() {
        let mut scope = Scope::new(true, 0);
        let sym1 = SymbolId(1);
        let sym2 = SymbolId(2);

        assert_eq!(scope.bind(sym1), 0);
        assert_eq!(scope.bind(sym2), 1);
        assert!(scope.contains(sym1));
        assert!(scope.contains(sym2));
    }

    #[test]
    fn test_scope_with_base_index() {
        let mut scope = Scope::new(true, 5);
        let sym = SymbolId(1);

        // With base_index=5, first binding should be at index 5
        assert_eq!(scope.bind(sym), 5);
    }

    #[test]
    fn test_scope_stack_lookup() {
        let mut stack = ScopeStack::new();
        let outer_sym = SymbolId(1);
        let inner_sym = SymbolId(2);

        // Push outer scope and bind
        stack.push(true, 0);
        stack.bind(outer_sym);

        // Push inner scope and bind
        stack.push(false, 0);
        stack.bind(inner_sym);

        // Should find inner_sym in inner scope (index 1)
        let (scope_idx, _) = stack.lookup(inner_sym).unwrap();
        assert_eq!(scope_idx, 1);

        // Should find outer_sym in outer scope (index 0)
        let (scope_idx, _) = stack.lookup(outer_sym).unwrap();
        assert_eq!(scope_idx, 0);

        // Unknown symbol should return None
        assert!(stack.lookup(SymbolId(999)).is_none());
    }

    #[test]
    fn test_mark_captured_and_mutated() {
        let mut stack = ScopeStack::new();
        let sym = SymbolId(1);

        stack.push(true, 0);
        stack.bind(sym);

        // Initially not captured or mutated
        let (_, binding) = stack.lookup(sym).unwrap();
        assert!(!binding.captured);
        assert!(!binding.mutated);

        // Mark as captured
        stack.mark_captured(sym);
        let (_, binding) = stack.lookup(sym).unwrap();
        assert!(binding.captured);

        // Mark as mutated
        stack.mark_mutated(sym);
        let (_, binding) = stack.lookup(sym).unwrap();
        assert!(binding.mutated);
    }

    #[test]
    fn test_innermost_function_scope() {
        let mut stack = ScopeStack::new();

        // No scopes
        assert!(stack.innermost_function_scope().is_none());

        // Function scope at 0
        stack.push(true, 0);
        assert_eq!(stack.innermost_function_scope(), Some(0));

        // Non-function scope at 1
        stack.push(false, 0);
        assert_eq!(stack.innermost_function_scope(), Some(0));

        // Another function scope at 2
        stack.push(true, 0);
        assert_eq!(stack.innermost_function_scope(), Some(2));
    }
}
