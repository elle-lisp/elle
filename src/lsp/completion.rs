//! Code completion support for LSP

use crate::primitives::def::Doc;
use crate::symbols::{SymbolIndex, SymbolKind};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Default documentation string when a symbol carries none of its own.
fn default_doc(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "User-defined function",
        SymbolKind::Variable => "Variable",
        SymbolKind::Macro => "Macro",
        SymbolKind::Module => "Module",
        SymbolKind::Builtin => "Built-in",
    }
}

/// Get completion items at the given position.
///
/// Two sources, in priority order: the document's own definitions (from the
/// symbol index) and the runtime's globally-bound callables (`builtin_names` —
/// primitives + core + stdlib, including operators like `+` that are stdlib
/// closures absent from `docs`). `docs` supplies documentation text where a
/// builtin has it. Both are authoritative runtime state, so the completion set
/// never drifts from a hand-maintained list.
pub(crate) fn get_completions(
    _line: u32,
    _character: u32,
    prefix: &str,
    symbol_index: &SymbolIndex,
    builtin_names: &[&str],
    docs: &HashMap<String, Doc>,
) -> Vec<Value> {
    let mut items = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();

    // User-defined symbols from the index.
    for (name, id, kind) in &symbol_index.available_symbols {
        if !name.starts_with(prefix) {
            continue;
        }
        let doc = symbol_index
            .definitions
            .get(id)
            .and_then(|d| d.documentation.clone())
            .unwrap_or_else(|| default_doc(*kind).to_string());
        items.push(json!({
            "label": name,
            "kind": kind.lsp_completion_kind(),
            "documentation": doc,
        }));
        seen.insert(name.as_str());
    }

    // Builtins from the runtime's callable globals (deduplicated against user
    // symbols), with documentation pulled from the docs map when present.
    for name in builtin_names {
        if !name.starts_with(prefix) || seen.contains(name) {
            continue;
        }
        seen.insert(name);
        items.push(json!({
            "label": name,
            "kind": SymbolKind::Builtin.lsp_completion_kind(),
            "documentation": docs.get(*name).map(|d| d.format()).unwrap_or_default(),
        }));
    }

    // Stable ordering by label.
    items.sort_by(|a, b| {
        let a_label = a.get("label").and_then(|l| l.as_str()).unwrap_or("");
        let b_label = b.get("label").and_then(|l| l.as_str()).unwrap_or("");
        a_label.cmp(b_label)
    });

    items
}

#[cfg(test)]
mod tests;
