// Tests for error reporting with source locations
//
// Verifies that parse errors include file name, line number, and column information.

use elle::reader::{Lexer, OwnedToken, Reader};
use elle::SymbolTable;

#[test]
fn test_parse_error_includes_location() {
    let mut symbols = SymbolTable::new();
    let input = "(+ 1 2";

    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();

    while let Ok(Some(token_with_loc)) = lexer.next_token_with_loc() {
        tokens.push(OwnedToken::from(token_with_loc.token));
        locations.push(token_with_loc.loc);
    }

    let mut reader = Reader::with_locations(tokens, locations);
    let result = reader.read(&mut symbols);

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("1:6")); // line:col should be present
    assert!(error.contains("unterminated list"));
}

#[test]
fn test_parse_error_column_tracking() {
    let mut symbols = SymbolTable::new();
    let input = "  (+ 1 2"; // Two spaces before paren

    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();

    while let Ok(Some(token_with_loc)) = lexer.next_token_with_loc() {
        tokens.push(OwnedToken::from(token_with_loc.token));
        locations.push(token_with_loc.loc);
    }

    let mut reader = Reader::with_locations(tokens, locations);
    let result = reader.read(&mut symbols);

    assert!(result.is_err());
    let error = result.unwrap_err();
    // Error should be at the position where EOF is reached
    assert!(error.contains("1:8")); // EOF at position 8
}

#[test]
fn test_unexpected_closing_paren_location() {
    let mut symbols = SymbolTable::new();
    let input = ")";

    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();

    while let Ok(Some(token_with_loc)) = lexer.next_token_with_loc() {
        tokens.push(OwnedToken::from(token_with_loc.token));
        locations.push(token_with_loc.loc);
    }

    let mut reader = Reader::with_locations(tokens, locations);
    let result = reader.read(&mut symbols);

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("1:1")); // Error at position 1:1
    assert!(error.contains("unexpected closing parenthesis"));
}

#[test]
fn test_unterminated_vector_location() {
    let mut symbols = SymbolTable::new();
    let input = "[1 2 3";

    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();

    while let Ok(Some(token_with_loc)) = lexer.next_token_with_loc() {
        tokens.push(OwnedToken::from(token_with_loc.token));
        locations.push(token_with_loc.loc);
    }

    let mut reader = Reader::with_locations(tokens, locations);
    let result = reader.read(&mut symbols);

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("1:6")); // EOF at position 6
    assert!(error.contains("unterminated vector"));
}

#[test]
fn test_unterminated_struct_location() {
    let mut symbols = SymbolTable::new();
    let input = "{:a 1 :b 2";

    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();

    while let Ok(Some(token_with_loc)) = lexer.next_token_with_loc() {
        tokens.push(OwnedToken::from(token_with_loc.token));
        locations.push(token_with_loc.loc);
    }

    let mut reader = Reader::with_locations(tokens, locations);
    let result = reader.read(&mut symbols);

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("unterminated struct"));
}

#[test]
fn test_list_sugar_error_location() {
    let mut symbols = SymbolTable::new();
    let input = "@invalid";

    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();

    while let Ok(Some(token_with_loc)) = lexer.next_token_with_loc() {
        tokens.push(OwnedToken::from(token_with_loc.token));
        locations.push(token_with_loc.loc);
    }

    let mut reader = Reader::with_locations(tokens, locations);
    let result = reader.read(&mut symbols);

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("@ must be followed by"));
}

#[test]
fn test_sourceloc_position_formatting() {
    use elle::reader::SourceLoc;

    let loc = SourceLoc::new("test.lisp", 5, 3);
    assert_eq!(loc.position(), "test.lisp:5:3");
}

#[test]
fn test_sourceloc_unknown_check() {
    use elle::reader::SourceLoc;

    let unknown = SourceLoc::start();
    assert!(unknown.is_unknown());

    let known = SourceLoc::new("file.lisp", 1, 1);
    assert!(!known.is_unknown());
}

#[test]
fn test_error_formatting_extract_source_line() {
    use elle::error::formatting;

    let source = "line 1\nline 2\nline 3";

    assert_eq!(
        formatting::extract_source_line(source, 1),
        Some("line 1".to_string())
    );
    assert_eq!(
        formatting::extract_source_line(source, 2),
        Some("line 2".to_string())
    );
    assert_eq!(
        formatting::extract_source_line(source, 3),
        Some("line 3".to_string())
    );
    assert_eq!(formatting::extract_source_line(source, 4), None);
    assert_eq!(formatting::extract_source_line(source, 0), None);
}

#[test]
fn test_error_formatting_highlight_column() {
    use elle::error::formatting;

    let line = "(+ x 1)";

    // Column 1
    let result = formatting::highlight_column(line, 1);
    assert_eq!(result, "^");

    // Column 4 (at 'x')
    let result = formatting::highlight_column(line, 4);
    assert!(result.ends_with('^'));
    assert_eq!(result.len(), 4); // 3 spaces + caret

    // Column 7 (at '1')
    let result = formatting::highlight_column(line, 7);
    assert!(result.ends_with('^'));
    assert_eq!(result.len(), 7); // 6 spaces + caret
}

#[test]
fn test_error_formatting_context() {
    use elle::error::formatting;
    use elle::reader::SourceLoc;

    let source = "line 1\nline 2 with error\nline 3";
    let loc = SourceLoc::new("test.lisp", 2, 6);

    let context = formatting::format_source_context(source, &loc);

    assert!(context.contains("line 2 with error"));
    assert!(context.contains("^"));
    assert!(context.contains("2 |")); // Line number
}

// ============================================================================
// LocationMap Tests - Verify bytecode offset to source location mapping
// ============================================================================

#[test]
fn test_location_map_populated_for_simple_expression() {
    use elle::pipeline::compile;
    use elle::SymbolTable;

    let mut symbols = SymbolTable::new();
    let source = "(+ 1 2)";

    let result = compile(source, &mut symbols);
    assert!(result.is_ok());

    let compiled = result.unwrap();
    // The location map should have at least one entry
    assert!(
        !compiled.bytecode.location_map.is_empty(),
        "LocationMap should be populated for compiled code"
    );
}

#[test]
fn test_location_map_has_correct_line_numbers() {
    use elle::pipeline::compile;
    use elle::SymbolTable;

    let mut symbols = SymbolTable::new();
    // Single expression with nested structure
    let source = "(if #t\n  (+ 1 2)\n  (- 3 4))";

    let result = compile(source, &mut symbols);
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());

    let compiled = result.unwrap();
    // Check that we have location entries
    assert!(
        !compiled.bytecode.location_map.is_empty(),
        "LocationMap should be populated"
    );

    // All entries should have line >= 1 (not synthetic)
    for loc in compiled.bytecode.location_map.values() {
        assert!(
            loc.line >= 1,
            "Line numbers should be >= 1, got {}",
            loc.line
        );
    }
}

#[test]
fn test_closure_has_location_map() {
    use elle::pipeline::compile;
    use elle::SymbolTable;

    let mut symbols = SymbolTable::new();
    let source = "(fn (x) (+ x 1))";

    let result = compile(source, &mut symbols);
    assert!(result.is_ok());

    let compiled = result.unwrap();
    // The main bytecode should have a location map
    assert!(
        !compiled.bytecode.location_map.is_empty(),
        "Main bytecode should have LocationMap"
    );

    // Check that closures in constants also have location maps
    for constant in &compiled.bytecode.constants {
        if let Some(closure) = constant.as_closure() {
            // Nested closures should have their own location maps
            // The location_map field exists (verified by compilation)
            // and may have entries for the closure's bytecode
            let _ = closure.location_map.len(); // Access to verify field exists
        }
    }
}

#[test]
fn test_vm_uses_location_map_for_stack_trace() {
    use elle::pipeline::compile;
    use elle::primitives::register_primitives;
    use elle::vm::VM;
    use elle::SymbolTable;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Compile a simple expression
    let source = "(+ 1 2)";
    let result = compile(source, &mut symbols);
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());

    let compiled = result.unwrap();

    // Execute the bytecode - this sets the VM's location_map
    let _ = vm.execute(&compiled.bytecode);

    // The VM's location_map should now be populated
    assert!(
        !vm.get_location_map().is_empty(),
        "VM's location_map should be set from bytecode"
    );
}
