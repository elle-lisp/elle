// Integration tests for LSP features
// These tests verify the LSP handlers work correctly

#[cfg(test)]
mod tests {
    use elle::compiler::symbol_index::SymbolIndex;
    use elle::SymbolTable;
    use elle_lsp::{definition, references};

    // --- Definition tests ---

    #[test]
    fn test_definition_returns_none_for_empty_index() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();

        let result = definition::find_definition(0, 0, &index, &symbol_table);
        assert!(result.is_none());
    }

    #[test]
    fn test_definition_returns_none_for_out_of_range_position() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();

        // Large line and character numbers should not panic
        let result = definition::find_definition(1000, 1000, &index, &symbol_table);
        assert!(result.is_none());
    }

    // --- References tests ---

    #[test]
    fn test_references_returns_empty_for_empty_index() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();

        let results = references::find_references(0, 0, false, &index, &symbol_table);
        assert!(results.is_empty());
    }

    #[test]
    fn test_references_returns_empty_for_out_of_range_position() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();

        // Large line and character numbers should not panic
        let results = references::find_references(1000, 1000, false, &index, &symbol_table);
        assert!(results.is_empty());
    }

    #[test]
    fn test_references_with_include_declaration_false() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();

        let results = references::find_references(0, 0, false, &index, &symbol_table);
        assert!(results.is_empty());
    }

    #[test]
    fn test_references_with_include_declaration_true() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();

        let results = references::find_references(0, 0, true, &index, &symbol_table);
        assert!(results.is_empty());
    }
}
