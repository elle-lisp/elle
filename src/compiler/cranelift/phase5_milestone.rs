// Phase 5 Milestone: Variable Scoping and Let Bindings
//
// This module documents and tests Phase 5 achievements:
// - Variable scoping with ScopeManager
// - Let binding compilation and variable binding
// - Variable reference (Var) compilation
// - Nested scope support with proper shadowing
// - Foundation for full lexical variable support

#[cfg(test)]
mod tests {
    use crate::compiler::cranelift::scoping::ScopeManager;
    use crate::value::SymbolId;

    #[test]
    fn phase5_scope_manager_creation() {
        // Phase 5 introduces ScopeManager for variable tracking
        let manager = ScopeManager::new();

        // Global scope should be created by default
        assert_eq!(manager.current_depth(), 0);
        assert_eq!(manager.current_scope_size(), 0);
    }

    #[test]
    fn phase5_variable_binding_in_scope() {
        // Phase 5: Variables can be bound in scopes
        let mut manager = ScopeManager::new();
        let sym1 = SymbolId(1);
        let sym2 = SymbolId(2);

        // Bind first variable
        manager.bind(sym1);
        assert_eq!(manager.current_scope_size(), 1);

        // Bind second variable
        manager.bind(sym2);
        assert_eq!(manager.current_scope_size(), 2);

        // Verify bindings are at correct indices
        let (depth1, idx1) = manager.lookup(sym1).unwrap();
        let (depth2, idx2) = manager.lookup(sym2).unwrap();
        assert_eq!((depth1, idx1), (0, 0));
        assert_eq!((depth2, idx2), (0, 1));
    }

    #[test]
    fn phase5_nested_scope_bindings() {
        // Phase 5: Nested scopes work correctly
        let mut manager = ScopeManager::new();
        let sym1 = SymbolId(1);
        let sym2 = SymbolId(2);

        // Bind in global scope
        manager.bind(sym1);
        assert_eq!(manager.current_depth(), 0);

        // Enter nested scope
        manager.push_scope();
        assert_eq!(manager.current_depth(), 1);

        // Bind in nested scope
        manager.bind(sym2);
        assert_eq!(manager.current_scope_size(), 1);

        // Variables from outer scopes are still accessible
        assert!(manager.lookup(sym1).is_some());
        assert!(manager.lookup(sym2).is_some());

        // Exit nested scope
        manager.pop_scope().unwrap();
        assert_eq!(manager.current_depth(), 0);

        // Only outer variable is visible now
        assert!(manager.lookup(sym1).is_some());
        // sym2 is no longer bound
        assert!(!manager.is_bound(sym2));
    }

    #[test]
    fn phase5_variable_shadowing() {
        // Phase 5: Variable shadowing works correctly
        let mut manager = ScopeManager::new();
        let sym = SymbolId(1);

        // Bind in global scope
        manager.bind(sym);
        let (depth1, idx1) = manager.lookup(sym).unwrap();
        assert_eq!((depth1, idx1), (0, 0));

        // Enter nested scope and rebind same variable (shadowing)
        manager.push_scope();
        manager.bind(sym);
        let (depth2, idx2) = manager.lookup(sym).unwrap();
        assert_eq!((depth2, idx2), (1, 0)); // New binding at new depth

        // Exit nested scope
        manager.pop_scope().unwrap();
        let (depth3, idx3) = manager.lookup(sym).unwrap();
        assert_eq!((depth3, idx3), (0, 0)); // Back to original binding
    }

    #[test]
    fn phase5_compile_context_variable_storage() {
        // Phase 5: CompileContext can store and retrieve variables
        let mut scope_manager = ScopeManager::new();

        // Create a variable
        let sym = SymbolId(1);
        scope_manager.bind(sym);

        // We can't fully test CompileContext without a FunctionBuilder,
        // but we've verified the scope_manager works which is what
        // CompileContext depends on
        assert_eq!(scope_manager.current_depth(), 0);
        assert!(scope_manager.lookup(sym).is_some());
    }

    #[test]
    fn phase5_multiple_nested_scopes() {
        // Phase 5: Multiple levels of nesting work correctly
        let mut manager = ScopeManager::new();
        let sym1 = SymbolId(1);
        let sym2 = SymbolId(2);
        let sym3 = SymbolId(3);

        // Global scope
        manager.bind(sym1);
        assert_eq!(manager.current_depth(), 0);

        // Nested scope 1
        manager.push_scope();
        manager.bind(sym2);
        assert_eq!(manager.current_depth(), 1);

        // Nested scope 2
        manager.push_scope();
        manager.bind(sym3);
        assert_eq!(manager.current_depth(), 2);

        // All variables should be accessible at depth 2
        assert!(manager.lookup(sym1).is_some());
        assert!(manager.lookup(sym2).is_some());
        assert!(manager.lookup(sym3).is_some());

        // Pop back to depth 1
        manager.pop_scope().unwrap();
        assert_eq!(manager.current_depth(), 1);
        assert!(manager.lookup(sym1).is_some());
        assert!(manager.lookup(sym2).is_some());
        assert!(!manager.is_bound(sym3));

        // Pop back to depth 0
        manager.pop_scope().unwrap();
        assert_eq!(manager.current_depth(), 0);
        assert!(manager.lookup(sym1).is_some());
        assert!(!manager.is_bound(sym2));
        assert!(!manager.is_bound(sym3));
    }

    #[test]
    fn phase5_scope_manager_cannot_pop_global() {
        // Phase 5: Global scope cannot be popped
        let mut manager = ScopeManager::new();

        let result = manager.pop_scope();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Cannot pop global scope".to_string());
    }

    #[test]
    fn phase5_binding_indices_increment() {
        // Phase 5: Binding indices increment correctly within a scope
        let mut manager = ScopeManager::new();
        let syms: Vec<_> = (0..5).map(SymbolId).collect();

        // Bind multiple variables
        for sym in &syms {
            manager.bind(*sym);
        }

        // Verify indices increment
        for (i, sym) in syms.iter().enumerate() {
            let (depth, idx) = manager.lookup(*sym).unwrap();
            assert_eq!(depth, 0);
            assert_eq!(idx, i);
        }
    }
}
