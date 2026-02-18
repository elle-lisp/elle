//! Hover information support for LSP
//!
//! Handles textDocument/hover requests by finding symbols at
//! the cursor position and returning their documentation.

use elle::symbol::SymbolTable;
use elle::symbols::{get_primitive_documentation, SymbolIndex, SymbolKind};
use serde_json::{json, Value};

/// Find hoverable information at a given position
pub fn find_hover_info(
    line: u32,
    character: u32,
    symbol_index: &SymbolIndex,
    symbol_table: &SymbolTable,
) -> Option<Value> {
    // LSP uses 0-based line numbers but SourceLoc uses 1-based
    let target_line = line as usize + 1;
    let target_col = character as usize + 1;

    // Look for symbols at this location
    // For now, find the closest symbol usage or definition
    let mut closest_symbol = None;
    let mut closest_distance = usize::MAX;

    // Check symbol usages
    for (sym_id, usages) in &symbol_index.symbol_usages {
        for usage_loc in usages {
            if usage_loc.line == target_line {
                let distance = (target_col as isize - usage_loc.col as isize).unsigned_abs();
                if distance < closest_distance && distance <= 10 {
                    // Within 10 characters of the symbol
                    closest_symbol = Some(*sym_id);
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
                closest_symbol = Some(*sym_id);
                closest_distance = distance;
            }
        }
    }

    // If we found a symbol, get its info
    closest_symbol.and_then(|sym_id| {
        let mut contents = Vec::new();

        // Get symbol name
        if let Some(name) = symbol_table.name(sym_id) {
            // Try to get documentation
            let doc = if let Some(def) = symbol_index.definitions.get(&sym_id) {
                def.documentation
                    .as_deref()
                    .or_else(|| get_primitive_documentation(name))
            } else {
                get_primitive_documentation(name)
            };

            if let Some(doc_str) = doc {
                contents.push(json!(doc_str));
            } else {
                contents.push(json!(format!("{}: Symbol", name)));
            }

            // Add type info if available
            if let Some(kind) = symbol_index.get_kind(sym_id) {
                let kind_str = match kind {
                    SymbolKind::Function => "Function",
                    SymbolKind::Variable => "Variable",
                    SymbolKind::Builtin => "Built-in",
                    SymbolKind::Macro => "Macro",
                    SymbolKind::Module => "Module",
                };
                contents.push(json!(format!("Type: {}", kind_str)));
            }

            // Add arity if it's a function
            if let Some(arity) = symbol_index.get_arity(sym_id) {
                contents.push(json!(format!("Arity: {}", arity)));
            }

            Some(json!({
                "contents": contents
            }))
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hover_info_returns_none_for_empty_index() {
        let index = SymbolIndex::new();
        let symbol_table = elle::SymbolTable::new();

        let hover = find_hover_info(0, 0, &index, &symbol_table);
        assert!(hover.is_none());
    }
}
