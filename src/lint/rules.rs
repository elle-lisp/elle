//! Linting rules for Elle code

use super::diagnostics::{Diagnostic, Severity};
use crate::primitives::registration::ALL_TABLES;
use crate::reader::SourceLoc;
use crate::value::types::Arity;
use crate::value::SymbolId;

/// Check arity of a function call
pub(crate) fn check_call_arity(
    func_sym: SymbolId,
    arg_count: usize,
    location: &Option<SourceLoc>,
    symbol_table: &crate::SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(func_name) = symbol_table.name(func_sym) {
        if let Some(arity) = builtin_arity(func_name) {
            if !arity.matches(arg_count) {
                let diag = Diagnostic::new(
                    Severity::Warning,
                    "W002",
                    "arity-mismatch",
                    format!(
                        "function '{}' expects {} argument(s) but got {}",
                        func_name, arity, arg_count
                    ),
                    location.clone(),
                );
                diagnostics.push(diag);
            }
        }
    }
}

/// Get arity of a built-in function by looking up `PrimitiveDef::PRIMITIVES` tables.
pub(crate) fn builtin_arity(name: &str) -> Option<Arity> {
    for table in ALL_TABLES {
        for def in *table {
            if def.name == name || def.aliases.contains(&name) {
                return Some(def.arity);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_arity() {
        use crate::value::Arity;
        // +, pair moved to stdlib; test with remaining Rust primitives
        assert_eq!(builtin_arity("abs"), Some(Arity::Exact(1)));
        assert_eq!(builtin_arity("list"), Some(Arity::AtLeast(0)));
        assert_eq!(builtin_arity("undefined"), None);
    }

    #[test]
    fn test_variadic_builtins_no_false_w002() {
        // list is variadic (AtLeast(0)); calling with multiple args must not produce W002
        let mut symbols = crate::SymbolTable::new();
        let mut diagnostics = Vec::new();

        let list = symbols.intern("list");
        check_call_arity(list, 3, &None, &symbols, &mut diagnostics);
        assert!(
            diagnostics.is_empty(),
            "W002 false positive for (list 1 2 3)"
        );

        check_call_arity(list, 5, &None, &symbols, &mut diagnostics);
        assert!(
            diagnostics.is_empty(),
            "W002 false positive for (list 1 2 3 4 5)"
        );
    }

    #[test]
    fn test_exact_arity_still_warns() {
        // abs expects exactly 1 arg
        let mut symbols = crate::SymbolTable::new();
        let mut diagnostics = Vec::new();

        let abs = symbols.intern("abs");
        check_call_arity(abs, 0, &None, &symbols, &mut diagnostics);
        assert_eq!(diagnostics.len(), 1, "W002 should fire for (abs)");

        diagnostics.clear();
        check_call_arity(abs, 2, &None, &symbols, &mut diagnostics);
        assert_eq!(diagnostics.len(), 1, "W002 should fire for (abs 1 2)");
    }
}
