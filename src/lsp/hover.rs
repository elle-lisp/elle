//! Hover information support for LSP

use crate::lsp::locate;
use crate::primitives::def::Doc;
use crate::symbols::{SymbolIndex, SymbolKind};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Human-readable kind label for the hover popup.
fn kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "Function",
        SymbolKind::Variable => "Variable",
        SymbolKind::Builtin => "Built-in",
        SymbolKind::Macro => "Macro",
        SymbolKind::Module => "Module",
    }
}

/// Find hoverable information at a given position.
///
/// The symbol's name comes from the index entry (`SymbolDef.name`), which is
/// populated for every binding — including usage-only primitives — so hover
/// works without re-deriving names from the (already-dropped) binding arena.
pub(crate) fn find_hover_info(
    line: u32,
    character: u32,
    symbol_index: &SymbolIndex,
    docs: &HashMap<String, Doc>,
) -> Option<Value> {
    let id = locate::symbol_at(symbol_index, line, character)?;
    let def = symbol_index.definitions.get(&id)?;

    let mut contents = Vec::new();

    // Prefer the user's own docstring; fall back to the builtin doc map.
    let doc = def
        .documentation
        .clone()
        .or_else(|| docs.get(&def.name).map(|d| d.format()));
    match doc {
        Some(doc_str) => contents.push(json!(doc_str)),
        None => contents.push(json!(format!("{}: Symbol", def.name))),
    }

    contents.push(json!(format!("Type: {}", kind_label(def.kind))));
    if let Some(arity) = def.arity {
        contents.push(json!(format!("Arity: {}", arity)));
    }

    Some(json!({ "contents": contents }))
}

#[cfg(test)]
mod tests;
