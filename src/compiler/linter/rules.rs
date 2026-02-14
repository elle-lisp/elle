//! Linting rules for Elle code

use super::diagnostics::{Diagnostic, Severity};
use crate::reader::SourceLoc;
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
pub fn check_call_arity(
    func_sym: SymbolId,
    arg_count: usize,
    location: &Option<SourceLoc>,
    symbol_table: &crate::SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Try to get the function name
    if let Some(func_name) = symbol_table.name(func_sym) {
        if let Some(expected_arity) = builtin_arity(func_name) {
            // For now, just warn on obvious mismatches
            // In a full implementation, we'd track user-defined function arities
            if arg_count != expected_arity {
                let diag = Diagnostic::new(
                    Severity::Warning,
                    "W002",
                    "arity-mismatch",
                    format!(
                        "function '{}' expects {} argument(s) but got {}",
                        func_name, expected_arity, arg_count
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

/// Get arity of built-in functions
fn builtin_arity(name: &str) -> Option<usize> {
    match name {
        // Arithmetic - these are actually variadic but min 2
        "+" | "-" | "*" | "/" | "mod" | "rem" => Some(2),
        // Comparison
        "=" | "<" | ">" | "<=" | ">=" => Some(2),
        // List operations
        "cons" => Some(2),
        "first" | "rest" => Some(1),
        "length" => Some(1),
        "append" => Some(2),
        "reverse" => Some(1),
        "nth" => Some(2),
        "last" => Some(1),
        "take" | "drop" => Some(2),
        // Math functions
        "abs" | "sqrt" | "sin" | "cos" | "tan" | "log" | "exp" | "floor" | "ceil" | "round" => {
            Some(1)
        }
        "pow" => Some(2),
        "min" | "max" => Some(2),
        // String operations
        "string-upcase" | "string-downcase" => Some(1),
        "string-append" => Some(2),
        "substring" => Some(3),
        "string-index" => Some(2),
        "char-at" => Some(2),
        // Type operations
        "type-of" => Some(1),
        // Logic
        "not" => Some(1),
        // Vector operations
        "vector-ref" => Some(2),
        "vector-set!" => Some(3),
        // Variadic or special forms - return None
        "list" | "vector" | "define" | "quote" | "begin" | "let" | "let*" | "fn" | "match"
        | "if" | "while" | "forever" | "each" => None,
        _ => None,
    }
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
        assert_eq!(builtin_arity("+"), Some(2));
        assert_eq!(builtin_arity("cons"), Some(2));
        assert_eq!(builtin_arity("list"), None);
        assert_eq!(builtin_arity("undefined"), None);
    }
}
