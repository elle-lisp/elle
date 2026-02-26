//! Find references support for LSP

use crate::symbol::SymbolTable;
use crate::symbols::SymbolIndex;
use serde_json::{json, Value};

/// Find all references to a symbol at a given position
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

    let mut target_symbol = None;
    let mut closest_distance = usize::MAX;

    // Check symbol usages first
    for (sym_id, usages) in &symbol_index.symbol_usages {
        for usage_loc in usages {
            if usage_loc.line == target_line {
                let distance = (target_col as isize - usage_loc.col as isize).unsigned_abs();
                if distance < closest_distance && distance <= 10 {
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
