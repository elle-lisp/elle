//! Symbol renaming support for LSP

use crate::error::{LError, LResult};
use crate::lsp::locate;
use crate::reader::SourceLoc;
use crate::symbol::SymbolTable;
use crate::symbols::SymbolIndex;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Names reserved in ADDITION to the analyzer's special-form registry
/// (`hir::analyze::forms::registry`, the single source of truth for special
/// forms): reader-level quotation forms and prelude macros that shadow poorly.
const EXTRA_RESERVED: &[&str] = &[
    "quasiquote",
    "unquote",
    "unquote-splicing",
    "let*",
    "defn",
    "defmacro",
    "not",
];

/// A name is reserved if it is any registered special-form name or alias,
/// or one of the reader-level extras above.
fn is_reserved(name: &str) -> bool {
    crate::hir::analyze::forms::registry::all_names().any(|n| n == name)
        || EXTRA_RESERVED.contains(&name)
}

/// Validate that a new name is acceptable for renaming
fn validate_new_name(new_name: &str) -> LResult<()> {
    if new_name.is_empty() {
        return Err(LError::generic("New name cannot be empty"));
    }

    if !new_name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(LError::generic(format!(
            "Invalid identifier format: '{}' contains invalid characters",
            new_name
        )));
    }

    if is_reserved(new_name) {
        return Err(LError::generic(format!(
            "'{}' is a reserved word and cannot be used as a symbol name",
            new_name
        )));
    }

    Ok(())
}

/// Check for conflicts when renaming a symbol
fn check_rename_conflict(
    old_name: &str,
    new_name: &str,
    symbol_index: &SymbolIndex,
    _symbol_table: &SymbolTable,
) -> LResult<()> {
    for def in symbol_index.definitions.values() {
        let sym_name = &def.name;
        if sym_name == old_name {
            continue;
        }
        if sym_name == new_name {
            return Err(LError::generic(format!(
                "Symbol '{}' already exists in this scope",
                new_name
            )));
        }
    }

    Ok(())
}

/// Does this location belong to the document the rename was requested in?
///
/// The index records real file paths, so the reconstructed URI matches the
/// request URI directly. The `ends_with` arm tolerates relative-vs-absolute
/// spelling differences between the two.
fn same_document(loc: &SourceLoc, uri: &str) -> bool {
    locate::loc_uri(loc) == uri || (!loc.file.is_empty() && uri.ends_with(&loc.file))
}

/// Push a `newText` edit for `loc` if it belongs to `uri`.
fn push_edit(edits: &mut Vec<Value>, loc: &SourceLoc, uri: &str, old_len: usize, new_name: &str) {
    if same_document(loc, uri) {
        edits.push(json!({
            "range": locate::name_range(loc, old_len),
            "newText": new_name,
        }));
    }
}

/// Rename the symbol at a given position to a new name.
///
/// Operates on a single `DefId`, so only the binding actually under the cursor
/// is renamed — two locals that merely share a name are no longer conflated.
pub(crate) fn rename_symbol(
    line: u32,
    character: u32,
    new_name: &str,
    symbol_index: &SymbolIndex,
    symbol_table: &SymbolTable,
    uri: &str,
) -> LResult<Value> {
    validate_new_name(new_name)?;

    let id = locate::symbol_at(symbol_index, line, character)
        .ok_or_else(|| LError::generic("No symbol found at the given position"))?;
    let def = symbol_index
        .definitions
        .get(&id)
        .ok_or_else(|| LError::generic("No symbol found at the given position"))?;
    let old_name = def.name.clone();
    let old_len = old_name.len();

    check_rename_conflict(&old_name, new_name, symbol_index, symbol_table)?;

    let mut text_edits = Vec::new();
    if let Some(usages) = symbol_index.symbol_usages.get(&id) {
        for loc in usages {
            push_edit(&mut text_edits, loc, uri, old_len, new_name);
        }
    }
    let def_loc = symbol_index
        .symbol_locations
        .get(&id)
        .or(def.location.as_ref());
    if let Some(loc) = def_loc {
        push_edit(&mut text_edits, loc, uri, old_len, new_name);
    }

    let mut changes = HashMap::new();
    if !text_edits.is_empty() {
        changes.insert(uri.to_string(), text_edits);
    }

    Ok(json!({ "changes": changes }))
}

#[cfg(test)]
mod tests;
