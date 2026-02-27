//! Symbol renaming support for LSP

use crate::symbol::SymbolTable;
use crate::symbols::SymbolIndex;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Reserved words that cannot be used as symbol names
const RESERVED_WORDS: &[&str] = &[
    "def",
    "var",
    "fn",
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
    "set",
    "do",
    "case",
    "and",
    "or",
    "not",
    "delay",
    "force",
    "call-with-current-continuation",
    "call/cc",
    "splice",
    "eval",
    "load",
    "require",
    "module",
    "use-modules",
];

/// Validate that a new name is acceptable for renaming
fn validate_new_name(new_name: &str) -> Result<(), String> {
    if new_name.is_empty() {
        return Err("New name cannot be empty".to_string());
    }

    if !new_name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "Invalid identifier format: '{}' contains invalid characters",
            new_name
        ));
    }

    if RESERVED_WORDS.contains(&new_name) {
        return Err(format!(
            "'{}' is a reserved word and cannot be used as a symbol name",
            new_name
        ));
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
    for def in symbol_index.definitions.values() {
        let sym_name = &def.name;
        if sym_name == old_name {
            continue;
        }
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
pub fn rename_symbol(
    line: u32,
    character: u32,
    new_name: &str,
    symbol_index: &SymbolIndex,
    symbol_table: &SymbolTable,
    _source_text: &str,
    uri: &str,
) -> Result<Value, String> {
    validate_new_name(new_name)?;

    let target_line = line as usize + 1;
    let target_col = character as usize + 1;

    let mut target_symbol = None;
    let mut closest_distance = usize::MAX;
    let mut old_name = String::new();

    for (sym_id, usages) in &symbol_index.symbol_usages {
        for usage_loc in usages {
            if usage_loc.line == target_line {
                let distance = (target_col as isize - usage_loc.col as isize).unsigned_abs();
                if distance < closest_distance && distance <= 10 {
                    if let Some(def) = symbol_index.definitions.get(sym_id) {
                        target_symbol = Some(*sym_id);
                        closest_distance = distance;
                        old_name = def.name.clone();
                    }
                }
            }
        }
    }

    for (sym_id, loc) in &symbol_index.symbol_locations {
        if loc.line == target_line {
            let distance = (target_col as isize - loc.col as isize).unsigned_abs();
            if distance < closest_distance && distance <= 10 {
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

    check_rename_conflict(&old_name, new_name, symbol_index, symbol_table)?;

    let sym_id = target_symbol.unwrap();
    let mut text_edits = Vec::new();

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
        assert!(validate_new_name("def").is_err());
        assert!(validate_new_name("var").is_err());
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
        let source = "(var foo 1)";
        let uri = "file:///test.elle";

        let result = rename_symbol(0, 0, "bar", &index, &symbol_table, source, uri);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "No symbol found at the given position");
    }

    #[test]
    fn test_rename_symbol_returns_workspace_edit() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();
        let source = "(var foo 1)";
        let uri = "file:///test.elle";

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
