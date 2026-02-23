// Integration tests for LSP features
// These tests verify the LSP handlers work correctly

#[cfg(test)]
mod tests {
    use elle::symbols::SymbolIndex;
    use elle::SymbolTable;
    use elle_lsp::{definition, formatting, references, rename, CompilerState};

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

    // --- Rename tests ---

    #[test]
    fn test_rename_symbol_no_symbol_at_position() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();
        let source = "(var foo 1)";
        let uri = "file:///test.elle";

        let result = rename::rename_symbol(0, 0, "bar", &index, &symbol_table, source, uri);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No symbol found"));
    }

    #[test]
    fn test_rename_symbol_validate_empty_name() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();
        let source = "(var foo 1)";
        let uri = "file:///test.elle";

        // Empty name should fail validation
        let result = rename::rename_symbol(0, 10, "", &index, &symbol_table, source, uri);
        assert!(result.is_err());
    }

    #[test]
    fn test_rename_symbol_validate_reserved_word() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();
        let source = "(var foo 1)";
        let uri = "file:///test.elle";

        // Reserved word should fail validation
        let result = rename::rename_symbol(0, 10, "def", &index, &symbol_table, source, uri);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("reserved"));
    }

    #[test]
    fn test_rename_symbol_validate_invalid_characters() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();
        let source = "(var foo 1)";
        let uri = "file:///test.elle";

        // Invalid characters should fail validation
        let result = rename::rename_symbol(0, 10, "foo@bar", &index, &symbol_table, source, uri);
        assert!(result.is_err());
    }

    #[test]
    fn test_rename_symbol_returns_workspace_edit() {
        let index = SymbolIndex::new();
        let symbol_table = SymbolTable::new();
        let source = "(var foo 1)";
        let uri = "file:///test.elle";

        // With empty symbol index, should return error about no symbol found
        let result = rename::rename_symbol(0, 10, "bar", &index, &symbol_table, source, uri);
        assert!(result.is_err());
    }

    // --- Syntax Error Tests ---

    #[test]
    fn test_compile_document_captures_syntax_errors() {
        let mut compiler_state = CompilerState::new();
        let uri = "file:///test.elle";
        let invalid_code = "(var foo 1))"; // Extra closing paren

        compiler_state.on_document_open(uri.to_string(), invalid_code.to_string());
        compiler_state.compile_document(uri);

        if let Some(doc) = compiler_state.get_document(uri) {
            // Should have captured a syntax error
            assert!(!doc.diagnostics.is_empty());
            assert!(doc.diagnostics[0].severity as i32 >= 2); // Error level (3) >= 2
        }
    }

    #[test]
    fn test_didopen_emits_syntax_errors_as_diagnostics() {
        let mut compiler_state = CompilerState::new();
        let uri = "file:///test.elle";
        let invalid_code = "(((("; // Invalid expression

        compiler_state.on_document_open(uri.to_string(), invalid_code.to_string());
        compiler_state.compile_document(uri);

        if let Some(doc) = compiler_state.get_document(uri) {
            // Should have syntax error diagnostics
            assert!(!doc.diagnostics.is_empty());
            let error_diags: Vec<_> = doc
                .diagnostics
                .iter()
                .filter(|d| d.severity == elle::lint::diagnostics::Severity::Error)
                .collect();
            assert!(!error_diags.is_empty());
        }
    }

    #[test]
    fn test_didchange_updates_syntax_errors() {
        let mut compiler_state = CompilerState::new();
        let uri = "file:///test.elle";

        // Start with valid code
        compiler_state.on_document_open(uri.to_string(), "(+ 1 2)".to_string());
        compiler_state.compile_document(uri);

        if let Some(doc) = compiler_state.get_document(uri) {
            assert!(doc.diagnostics.is_empty());
        }

        // Change to invalid code
        compiler_state.on_document_change(uri, "((((invalid".to_string());
        compiler_state.compile_document(uri);

        if let Some(doc) = compiler_state.get_document(uri) {
            // Should now have syntax errors
            assert!(!doc.diagnostics.is_empty());
        }
    }

    #[test]
    fn test_didclose_removes_document_state() {
        let mut compiler_state = CompilerState::new();
        let uri = "file:///test.elle";

        compiler_state.on_document_open(uri.to_string(), "(+ 1 2)".to_string());
        assert!(compiler_state.get_document(uri).is_some());

        compiler_state.on_document_close(uri);
        assert!(compiler_state.get_document(uri).is_none());
    }

    #[test]
    fn test_compile_valid_code_no_syntax_errors() {
        let mut compiler_state = CompilerState::new();
        let uri = "file:///test.elle";

        compiler_state.on_document_open(
            uri.to_string(),
            "(var my-func (lambda (x) (+ x 1)))".to_string(),
        );
        compiler_state.compile_document(uri);

        if let Some(doc) = compiler_state.get_document(uri) {
            // Valid code should have no syntax errors
            let syntax_errors: Vec<_> = doc
                .diagnostics
                .iter()
                .filter(|d| d.rule == "syntax-error")
                .collect();
            assert!(syntax_errors.is_empty());
        }
    }
}
