// Integration tests for LSP features
// These tests verify the LSP handlers work correctly

#[cfg(test)]
mod tests {
    use elle::compiler::symbol_index::SymbolIndex;
    use elle::SymbolTable;
    use elle_lsp::{definition, formatting, references};

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

    // --- Formatting tests ---

    #[test]
    fn test_document_end_position_empty() {
        let (line, char) = formatting::document_end_position("");
        assert_eq!(line, 0);
        assert_eq!(char, 0);
    }

    #[test]
    fn test_document_end_position_single_line() {
        let (line, char) = formatting::document_end_position("hello");
        assert_eq!(line, 0);
        assert_eq!(char, 5);
    }

    #[test]
    fn test_document_end_position_multiple_lines() {
        let (line, char) = formatting::document_end_position("hello\nworld");
        assert_eq!(line, 1);
        assert_eq!(char, 5);
    }

    #[test]
    fn test_format_document_simple_number() {
        let source = "42";
        let (end_line, end_char) = formatting::document_end_position(source);
        let result = formatting::format_document(source, end_line, end_char);

        assert!(result.is_ok());
        let edits = result.unwrap();
        assert_eq!(edits.len(), 1);

        let edit = &edits[0];
        assert!(edit.get("range").is_some());
        assert!(edit.get("newText").is_some());
    }

    #[test]
    fn test_format_document_simple_list() {
        let source = "(+ 1 2)";
        let (end_line, end_char) = formatting::document_end_position(source);
        let result = formatting::format_document(source, end_line, end_char);

        assert!(result.is_ok());
        let edits = result.unwrap();
        assert_eq!(edits.len(), 1);

        let edit = &edits[0];
        assert!(edit.get("range").is_some());
        assert!(edit.get("newText").is_some());
    }
}
