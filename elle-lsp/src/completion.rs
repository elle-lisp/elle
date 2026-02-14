//! Code completion support for LSP
//!
//! Provides completion suggestions for Elle Lisp symbols,
//! including built-ins and user-defined symbols.

use elle::compiler::symbol_index::{get_primitive_documentation, SymbolIndex, SymbolKind};
use elle::symbol::SymbolTable;
use serde_json::{json, Value};

/// Get completion items at the given position
pub fn get_completions(
    _line: u32,
    _character: u32,
    prefix: &str,
    symbol_index: &SymbolIndex,
    _symbol_table: &SymbolTable,
) -> Vec<Value> {
    let mut items = Vec::new();

    // Add user-defined symbols
    for (name, sym_id, kind) in &symbol_index.available_symbols {
        if name.starts_with(prefix) {
            let label = name.clone();
            let doc = match *kind {
                SymbolKind::Function => symbol_index
                    .definitions
                    .get(sym_id)
                    .and_then(|d| d.documentation.as_deref())
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| "User-defined function".to_string()),
                SymbolKind::Variable => symbol_index
                    .definitions
                    .get(sym_id)
                    .and_then(|d| d.documentation.as_deref())
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| "Variable".to_string()),
                _ => symbol_index
                    .definitions
                    .get(sym_id)
                    .and_then(|d| d.documentation.as_deref())
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| "Symbol".to_string()),
            };

            let kind_num = match *kind {
                SymbolKind::Function => 12, // LSP CompletionItemKind.Function
                SymbolKind::Variable => 6,  // LSP CompletionItemKind.Variable
                SymbolKind::Builtin => 2,   // LSP CompletionItemKind.Module
                SymbolKind::Macro => 24,    // LSP CompletionItemKind.Keyword
                SymbolKind::Module => 9,    // LSP CompletionItemKind.Module
            };

            items.push(json!({
                "label": label,
                "kind": kind_num,
                "documentation": doc,
            }));
        }
    }

    // Add built-in primitives that match the prefix
    let builtins = vec![
        // Arithmetic
        ("+", SymbolKind::Builtin, "Add numbers"),
        ("-", SymbolKind::Builtin, "Subtract numbers"),
        ("*", SymbolKind::Builtin, "Multiply numbers"),
        ("/", SymbolKind::Builtin, "Divide numbers"),
        ("mod", SymbolKind::Builtin, "Modulo operation"),
        // Comparison
        ("=", SymbolKind::Builtin, "Test equality"),
        ("<", SymbolKind::Builtin, "Less than"),
        (">", SymbolKind::Builtin, "Greater than"),
        ("<=", SymbolKind::Builtin, "Less than or equal"),
        (">=", SymbolKind::Builtin, "Greater than or equal"),
        // List operations
        ("cons", SymbolKind::Builtin, "Construct list"),
        ("first", SymbolKind::Builtin, "Get first element"),
        ("rest", SymbolKind::Builtin, "Get rest of list"),
        ("length", SymbolKind::Builtin, "Get list length"),
        ("append", SymbolKind::Builtin, "Append lists"),
        // Control flow
        ("if", SymbolKind::Builtin, "Conditional expression"),
        ("define", SymbolKind::Builtin, "Define variable"),
        ("fn", SymbolKind::Builtin, "Create function"),
        (
            "lambda",
            SymbolKind::Builtin,
            "Create function (alias for fn)",
        ),
        ("let", SymbolKind::Builtin, "Local binding"),
        ("begin", SymbolKind::Builtin, "Sequential execution"),
        // Type checking
        ("type", SymbolKind::Builtin, "Get type of value"),
        // String operations
        ("string-append", SymbolKind::Builtin, "Append strings"),
    ];

    for (name, kind, doc) in builtins {
        if name.starts_with(prefix) {
            let kind_num = match kind {
                SymbolKind::Builtin => 2,
                _ => 12,
            };

            // Only add if not already in user symbols
            if !items.iter().any(|item| {
                item.get("label")
                    .and_then(|l| l.as_str())
                    .map(|l| l == name)
                    .unwrap_or(false)
            }) {
                if let Some(full_doc) = get_primitive_documentation(name) {
                    items.push(json!({
                        "label": name,
                        "kind": kind_num,
                        "documentation": full_doc,
                    }));
                } else {
                    items.push(json!({
                        "label": name,
                        "kind": kind_num,
                        "documentation": doc,
                    }));
                }
            }
        }
    }

    // Sort by label for consistent ordering
    items.sort_by(|a, b| {
        let a_label = a.get("label").and_then(|l| l.as_str()).unwrap_or("");
        let b_label = b.get("label").and_then(|l| l.as_str()).unwrap_or("");
        a_label.cmp(b_label)
    });

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_empty() {
        let index = SymbolIndex::new();
        let symbol_table = elle::SymbolTable::new();

        let completions = get_completions(0, 0, "", &index, &symbol_table);
        // Should at least have built-in symbols
        assert!(!completions.is_empty());
    }

    #[test]
    fn test_completion_with_prefix() {
        let index = SymbolIndex::new();
        let symbol_table = elle::SymbolTable::new();

        let completions = get_completions(0, 0, "cons", &index, &symbol_table);
        assert!(!completions.is_empty());
        // Should include "cons"
        assert!(completions.iter().any(|item| {
            item.get("label")
                .and_then(|l| l.as_str())
                .map(|l| l.starts_with("cons"))
                .unwrap_or(false)
        }));
    }

    #[test]
    fn test_completion_no_match() {
        let index = SymbolIndex::new();
        let symbol_table = elle::SymbolTable::new();

        let completions = get_completions(0, 0, "xyz123", &index, &symbol_table);
        // No symbols should match this prefix
        assert!(completions.is_empty());
    }
}
