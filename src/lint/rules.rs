//! Linting rules for Elle code

use super::diagnostics::{Diagnostic, Severity};
use crate::primitives::registration::ALL_TABLES;
use crate::reader::SourceLoc;
use crate::value::types::Arity;
use crate::value::SymbolId;

/// Check naming conventions for a symbol
pub fn check_naming_convention(
    name: &str,
    location: &Option<SourceLoc>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Allow single letter variables and built-in functions
    if name.len() == 1 {
        return;
    }

    // Check for kebab-case requirement
    // Allowed suffixes: ? (predicate), ! (mutation)
    let base_name = if name.ends_with('?') || name.ends_with('!') {
        &name[..name.len() - 1]
    } else {
        name
    };

    // Check if it's valid kebab-case
    if !is_valid_kebab_case(base_name) {
        let suggestion = to_kebab_case(base_name);
        let suggested_name = if name.ends_with('?') {
            format!("{}?", suggestion)
        } else if name.ends_with('!') {
            format!("{}!", suggestion)
        } else {
            suggestion
        };

        let diag = Diagnostic::new(
            Severity::Warning,
            "W001",
            "naming-kebab-case",
            format!("identifier '{}' should use kebab-case", name),
            location.clone(),
        )
        .with_suggestions(vec![format!("rename to '{}'", suggested_name)]);

        diagnostics.push(diag);
    }
}

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

/// Check if a name is valid kebab-case
fn is_valid_kebab_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    // Must be all lowercase letters, numbers, and hyphens
    // Cannot start or end with hyphen
    if s.starts_with('-') || s.ends_with('-') {
        return false;
    }

    // Must contain only lowercase, digits, and hyphens
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Convert a string to kebab-case
fn to_kebab_case(s: &str) -> String {
    let mut result = String::new();

    for (i, c) in s.chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            result.push('-');
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c.to_ascii_lowercase());
        }
    }

    result
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
    fn test_valid_kebab_case() {
        assert!(is_valid_kebab_case("square"));
        assert!(is_valid_kebab_case("square-number"));
        assert!(is_valid_kebab_case("add-two-numbers"));
        assert!(is_valid_kebab_case("foo1"));
        assert!(is_valid_kebab_case("foo-1-bar"));
    }

    #[test]
    fn test_invalid_kebab_case() {
        assert!(!is_valid_kebab_case("camelCase"));
        assert!(!is_valid_kebab_case("PascalCase"));
        assert!(!is_valid_kebab_case("snake_case"));
        assert!(!is_valid_kebab_case("-leading"));
        assert!(!is_valid_kebab_case("trailing-"));
    }

    #[test]
    fn test_to_kebab_case() {
        assert_eq!(to_kebab_case("squareNumber"), "square-number");
        assert_eq!(to_kebab_case("myVariable"), "my-variable");
        assert_eq!(to_kebab_case("FOO"), "f-o-o");
    }

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
