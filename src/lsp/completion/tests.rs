//! Unit tests (`super` is the parent impl module).
//!
//! Builtin completions are now sourced from the VM's docs map, so these tests
//! build a real `CompilerState` to obtain that map rather than passing an empty
//! one (which would, correctly, yield no builtins).

use super::*;

#[test]
fn test_completion_empty_prefix_lists_builtins() {
    crate::value::arena::with_test_region(|| {
        let state = crate::lsp::state::CompilerState::new();
        let index = SymbolIndex::new();
        let builtins = state.builtin_names();
        let completions = get_completions(0, 0, "", &index, &builtins, state.docs());
        assert!(!completions.is_empty(), "builtins should be offered");
    });
}

#[test]
fn test_completion_with_prefix() {
    crate::value::arena::with_test_region(|| {
        let state = crate::lsp::state::CompilerState::new();
        let index = SymbolIndex::new();
        let builtins = state.builtin_names();
        let completions = get_completions(0, 0, "pair", &index, &builtins, state.docs());
        assert!(completions.iter().any(|item| {
            item.get("label")
                .and_then(|l| l.as_str())
                .map(|l| l.starts_with("pair"))
                .unwrap_or(false)
        }));
    });
}

// The stdlib operator `+` is a closure (not a primitive in `vm.docs`); it must
// still be offered. Regression guard for sourcing builtins from `vm.docs` alone.
#[test]
fn test_completion_includes_stdlib_operator() {
    crate::value::arena::with_test_region(|| {
        let state = crate::lsp::state::CompilerState::new();
        let index = SymbolIndex::new();
        let builtins = state.builtin_names();
        let completions = get_completions(0, 0, "+", &index, &builtins, state.docs());
        assert!(
            completions
                .iter()
                .any(|i| i.get("label").and_then(|l| l.as_str()) == Some("+")),
            "completion should include the stdlib operator '+'"
        );
    });
}

#[test]
fn test_completion_no_match() {
    crate::value::arena::with_test_region(|| {
        let state = crate::lsp::state::CompilerState::new();
        let index = SymbolIndex::new();
        let builtins = state.builtin_names();
        let completions = get_completions(0, 0, "xyz123", &index, &builtins, state.docs());
        assert!(completions.is_empty());
    });
}
