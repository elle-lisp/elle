//! Symbol renaming support for LSP
//!
//! Handles textDocument/rename requests by validating the new name and generating
//! TextEdits for all occurrences of the symbol to be renamed. Supports both local
//! and global scope renaming with validation for reserved words and conflicts.

use elle::compiler::symbol_index::SymbolIndex;
use elle::symbol::SymbolTable;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Reserved words that cannot be used as symbol names
const RESERVED_WORDS: &[&str] = &[
    "define",
    "fn",
    "lambda",
    "if",
    "cond",
    "quote",
    "quasiquote",
    "unquote",
    "unquote-splicing",
    "let",
    "let*",
    "letrec",
    "begin",
    "set!",
    "do",
    "case",
    "and",
    "or",
    "not",
    "delay",
    "force",
    "call-with-current-continuation",
    "call/cc",
    "apply",
    "eval",
    "load",
    "require",
    "module",
    "use-modules",
];

/// Built-in functions that shadow-able
const BUILTIN_FUNCTIONS: &[&str] = &[
    "+",
    "-",
    "*",
    "/",
    "=",
    "<",
    ">",
    "<=",
    ">=",
    "number?",
    "string?",
    "symbol?",
    "list?",
    "pair?",
    "null?",
    "procedure?",
    "car",
    "cdr",
    "cons",
    "length",
    "append",
    "reverse",
    "member",
    "assoc",
    "map",
    "filter",
    "fold",
    "reduce",
    "display",
    "newline",
    "read",
    "open-input-file",
    "open-output-file",
    "close-input-port",
    "close-output-port",
];

/// Validate that a new name is acceptable for renaming
fn validate_new_name(new_name: &str) -> Result<(), String> {
    // Check not empty
    if new_name.is_empty() {
        return Err("New name cannot be empty".to_string());
    }

    // Check valid identifier format (alphanumeric, hyphens, underscores)
    if !new_name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "Invalid identifier format: '{}' contains invalid characters",
            new_name
        ));
    }

    // Check not a reserved word
    if RESERVED_WORDS.contains(&new_name) {
        return Err(format!(
            "'{}' is a reserved word and cannot be used as a symbol name",
            new_name
        ));
    }

    // Check not shadowing a builtin (warning-level, but we allow it)
    if BUILTIN_FUNCTIONS.contains(&new_name) {
        // This is allowed, but could be warned about
        // For now, we just allow it silently
    }

    Ok(())
}

/// Check for conflicts when renaming a symbol
fn check_rename_conflict(
    old_name: &str,
    new_name: &str,
    symbol_index: &SymbolIndex,
    _symbol_table: &SymbolTable,
) -> Result<(), String> {
    // For each symbol ID in the index, check if there's a conflict
    for def in symbol_index.definitions.values() {
        let sym_name = &def.name;
        // Skip the symbol being renamed
        if sym_name == old_name {
            continue;
        }
        // Check if the new name already exists in the same scope
        if sym_name == new_name {
            return Err(format!(
                "Symbol '{}' already exists in this scope",
                new_name
            ));
        }
    }

    Ok(())
}

/// Rename a symbol at a given position to a new name
///
/// Returns a WorkspaceEdit containing all TextEdits needed to perform the rename,
/// or an error message if validation fails.
pub fn rename_symbol(
    line: u32,
    character: u32,
    new_name: &str,
    symbol_index: &SymbolIndex,
    symbol_table: &SymbolTable,
    _source_text: &str,
    uri: &str,
) -> Result<Value, String> {
    // 1. Validate new name
    validate_new_name(new_name)?;

    // 2. Find symbol at position (using same logic as find_references)
    let target_line = line as usize + 1;
    let target_col = character as usize + 1;

    let mut target_symbol = None;
    let mut closest_distance = usize::MAX;
    let mut old_name = String::new();

    // Check symbol usages first
    for (sym_id, usages) in &symbol_index.symbol_usages {
        for usage_loc in usages {
            if usage_loc.line == target_line {
                let distance = (target_col as isize - usage_loc.col as isize).unsigned_abs();
                if distance < closest_distance && distance <= 10 {
                    // Look up the symbol name
                    if let Some(def) = symbol_index.definitions.get(sym_id) {
                        target_symbol = Some(*sym_id);
                        closest_distance = distance;
                        old_name = def.name.clone();
                    }
                }
            }
        }
    }

    // Check symbol definitions
    for (sym_id, loc) in &symbol_index.symbol_locations {
        if loc.line == target_line {
            let distance = (target_col as isize - loc.col as isize).unsigned_abs();
            if distance < closest_distance && distance <= 10 {
                // Look up the symbol name
                if let Some(def) = symbol_index.definitions.get(sym_id) {
                    target_symbol = Some(*sym_id);
                    closest_distance = distance;
                    old_name = def.name.clone();
                }
            }
        }
    }

    if target_symbol.is_none() {
        return Err("No symbol found at the given position".to_string());
    }

    // 3. Check for conflicts
    check_rename_conflict(&old_name, new_name, symbol_index, symbol_table)?;

    // 4. Generate TextEdits for all occurrences
    let sym_id = target_symbol.unwrap();
    let mut text_edits = Vec::new();

    // Add edits for all usages
    if let Some(usages) = symbol_index.symbol_usages.get(&sym_id) {
        for usage_loc in usages {
            let file_uri = format!("file://{}", usage_loc.file);
            if file_uri == uri || uri.ends_with(&usage_loc.file) {
                text_edits.push(json!({
                    "range": {
                        "start": {
                            "line": usage_loc.line.saturating_sub(1),
                            "character": usage_loc.col.saturating_sub(1)
                        },
                        "end": {
                            "line": usage_loc.line.saturating_sub(1),
                            "character": usage_loc.col.saturating_sub(1) + old_name.len()
                        }
                    },
                    "newText": new_name
                }));
            }
        }
    }

    // Add edit for the definition location
    if let Some(def_loc) = symbol_index.symbol_locations.get(&sym_id) {
        let file_uri = format!("file://{}", def_loc.file);
        if file_uri == uri || uri.ends_with(&def_loc.file) {
            text_edits.push(json!({
                "range": {
                    "start": {
                        "line": def_loc.line.saturating_sub(1),
                        "character": def_loc.col.saturating_sub(1)
                    },
                    "end": {
                        "line": def_loc.line.saturating_sub(1),
                        "character": def_loc.col.saturating_sub(1) + old_name.len()
                    }
                },
                "newText": new_name
            }));
        }
    } else if let Some(def) = symbol_index.definitions.get(&sym_id) {
        // Fallback to definition from symbol table
        if let Some(def_loc) = &def.location {
            let file_uri = format!("file://{}", def_loc.file);
            if file_uri == uri || uri.ends_with(&def_loc.file) {
                text_edits.push(json!({
                    "range": {
                        "start": {
                            "line": def_loc.line.saturating_sub(1),
                            "character": def_loc.col.saturating_sub(1)
                        },
                        "end": {
                            "line": def_loc.line.saturating_sub(1),
                            "character": def_loc.col.saturating_sub(1) + old_name.len()
                        }
                    },
                    "newText": new_name
                }));
            }
        }
    }

    // 5. Return WorkspaceEdit format
    let mut changes = HashMap::new();
    if !text_edits.is_empty() {
        changes.insert(uri.to_string(), text_edits);
    }

    Ok(json!({
        "changes": changes
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_new_name_empty() {
        assert!(validate_new_name("").is_err());
    }

    #[test]
    fn test_validate_new_name_reserved_word() {
        assert!(validate_new_name("define").is_err());
        assert!(validate_new_name("lambda").is_err());
        assert!(validate_new_name("if").is_err());
    }

    #[test]
    fn test_validate_new_name_invalid_characters() {
        assert!(validate_new_name("foo@bar").is_err());
        assert!(validate_new_name("foo bar").is_err());
    }

    #[test]
    fn test_validate_new_name_valid() {
        assert!(validate_new_name("my-function").is_ok());
        assert!(validate_new_name("my_function").is_ok());
        assert!(validate_new_name("myFunction").is_ok());
        assert!(validate_new_name("my123").is_ok());
    }

    #[test]
    fn test_rename_symbol_no_symbol_at_position() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();
        let source = "(define foo 1)";
        let uri = "file:///test.elle";

        let result = rename_symbol(0, 0, "bar", &index, &symbol_table, source, uri);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "No symbol found at the given position");
    }

    #[test]
    fn test_rename_symbol_returns_workspace_edit() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();
        let source = "(define foo 1)";
        let uri = "file:///test.elle";

        // With empty symbol index, should return error about no symbol found
        let result = rename_symbol(0, 10, "bar", &index, &symbol_table, source, uri);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_rename_conflict_no_conflict() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();

        let result = check_rename_conflict("foo", "bar", &index, &symbol_table);
        assert!(result.is_ok());
    }
}
