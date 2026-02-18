//! Find references support for LSP
//!
//! Handles textDocument/references requests by finding all usages of a symbol
//! and returning their locations. Supports the include_declaration parameter
//! to optionally include the symbol's definition location.

use elle::symbol::SymbolTable;
use elle::symbols::SymbolIndex;
use serde_json::{json, Value};

/// Find all references to a symbol at a given position
///
/// Returns an array of Location objects representing all uses of the symbol,
/// optionally including the definition location if include_declaration is true.
pub fn find_references(
    line: u32,
    character: u32,
    include_declaration: bool,
    symbol_index: &SymbolIndex,
    _symbol_table: &SymbolTable,
) -> Vec<Value> {
    // LSP uses 0-based line numbers but SourceLoc uses 1-based
    let target_line = line as usize + 1;
    let target_col = character as usize + 1;

    let mut references = Vec::new();

    // Look for the symbol at the cursor position (check both usages and definitions)
    let mut target_symbol = None;
    let mut closest_distance = usize::MAX;

    // Check symbol usages first
    for (sym_id, usages) in &symbol_index.symbol_usages {
        for usage_loc in usages {
            if usage_loc.line == target_line {
                let distance = (target_col as isize - usage_loc.col as isize).unsigned_abs();
                if distance < closest_distance && distance <= 10 {
                    // Within 10 characters of the symbol
                    target_symbol = Some(*sym_id);
                    closest_distance = distance;
                }
            }
        }
    }

    // Check symbol definitions
    for (sym_id, loc) in &symbol_index.symbol_locations {
        if loc.line == target_line {
            let distance = (target_col as isize - loc.col as isize).unsigned_abs();
            if distance < closest_distance && distance <= 10 {
                target_symbol = Some(*sym_id);
                closest_distance = distance;
            }
        }
    }

    // If we found a symbol, collect all its references
    if let Some(sym_id) = target_symbol {
        // Add all usages of the symbol
        if let Some(usages) = symbol_index.symbol_usages.get(&sym_id) {
            for usage_loc in usages {
                let uri = format!("file://{}", usage_loc.file);
                references.push(json!({
                    "uri": uri,
                    "range": {
                        "start": {
                            "line": usage_loc.line.saturating_sub(1),
                            "character": usage_loc.col.saturating_sub(1)
                        },
                        "end": {
                            "line": usage_loc.line.saturating_sub(1),
                            "character": usage_loc.col
                        }
                    }
                }));
            }
        }

        // Optionally add the definition location
        if include_declaration {
            if let Some(def_loc) = symbol_index.symbol_locations.get(&sym_id) {
                let uri = format!("file://{}", def_loc.file);
                references.push(json!({
                    "uri": uri,
                    "range": {
                        "start": {
                            "line": def_loc.line.saturating_sub(1),
                            "character": def_loc.col.saturating_sub(1)
                        },
                        "end": {
                            "line": def_loc.line.saturating_sub(1),
                            "character": def_loc.col
                        }
                    }
                }));
            } else if let Some(def) = symbol_index.definitions.get(&sym_id) {
                // Fallback to definition from symbol table
                if let Some(def_loc) = &def.location {
                    let uri = format!("file://{}", def_loc.file);
                    references.push(json!({
                        "uri": uri,
                        "range": {
                            "start": {
                                "line": def_loc.line.saturating_sub(1),
                                "character": def_loc.col.saturating_sub(1)
                            },
                            "end": {
                                "line": def_loc.line.saturating_sub(1),
                                "character": def_loc.col
                            }
                        }
                    }));
                }
            }
        }
    }

    references
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_references_returns_empty_for_empty_index() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();

        let references = find_references(0, 0, false, &index, &symbol_table);
        assert!(references.is_empty());
    }

    #[test]
    fn test_find_references_with_include_declaration_false() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();

        let references = find_references(0, 0, false, &index, &symbol_table);
        assert!(references.is_empty());
    }

    #[test]
    fn test_find_references_with_include_declaration_true() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();

        let references = find_references(0, 0, true, &index, &symbol_table);
        assert!(references.is_empty());
    }

    #[test]
    fn test_find_references_out_of_range_position() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();

        let references = find_references(1000, 1000, false, &index, &symbol_table);
        assert!(references.is_empty());
    }
}
