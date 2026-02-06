//! Naming convention rules

use crate::diagnostics::{Diagnostic, Severity};
use elle::value::Value;

/// Check naming conventions
pub fn check_naming_conventions(
    value: &Value,
    filename: &str,
    line: usize,
    diagnostics: &mut Vec<Diagnostic>,
    symbols: &elle::SymbolTable,
) {
    if let Value::Cons(_) = value {
        // Convert cons list to vec for easier handling
        if let Ok(list) = value.list_to_vec() {
            if list.is_empty() {
                return;
            }

            // Check if this is a define statement
            if let Ok(first_sym) = list[0].as_symbol() {
                if let Some(sym_name) = symbols.name(first_sym) {
                    if sym_name == "define" && list.len() >= 2 {
                        // Get the name being defined
                        if let Ok(name_sym) = list[1].as_symbol() {
                            if let Some(name) = symbols.name(name_sym) {
                                check_identifier_naming(name, filename, line, 2, diagnostics);
                            }
                        }
                    }
                }
            }

            // Recursively check nested expressions
            for element in list.iter().skip(1) {
                check_naming_conventions(element, filename, line, diagnostics, symbols);
            }
        }
    }
}

/// Check if an identifier follows naming conventions
fn check_identifier_naming(
    name: &str,
    filename: &str,
    line: usize,
    column: usize,
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
            "W001".to_string(),
            "naming-kebab-case".to_string(),
            format!("identifier '{}' should use kebab-case", name),
            filename.to_string(),
            line,
            column,
            format!("(define {} ...)", name),
        )
        .with_suggestions(vec![format!("rename to '{}'", suggested_name)]);

        diagnostics.push(diag);
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
}
