//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_symbol_kind_lsp_kind() {
    assert_eq!(SymbolKind::Function.lsp_kind(), "Function");
    assert_eq!(SymbolKind::Variable.lsp_kind(), "Variable");
    assert_eq!(SymbolKind::Builtin.lsp_kind(), "Class");
}

#[test]
fn test_symbol_def_builder() {
    let sym_id = SymbolId(1);
    let def = SymbolDef::new(sym_id, "test-var".to_string(), SymbolKind::Variable)
        .with_arity(2)
        .with_documentation("A test variable".to_string());

    assert_eq!(def.arity, Some(2));
    assert_eq!(def.documentation, Some("A test variable".to_string()));
}

#[test]
fn test_symbol_index_creation() {
    let index = SymbolIndex::new();
    assert_eq!(index.definitions.len(), 0);
    assert_eq!(index.available_symbols.len(), 0);
}
