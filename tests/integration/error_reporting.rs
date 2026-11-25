// audited: 2026-11-23
// A parse error names its line and column, compiled code carries its location
// table, and the root report names what reached it.
//
// docs/impl/vm.md
//
// tests/cli/traceback.lisp pins the form an uncaught error is reported at.

use elle::reader::{Lexer, OwnedToken, Reader};
use elle::SymbolTable;

// A local `compile`: the location tests are compile-only and stdlib-free, so a
// fresh `CompileCtx` per call (primitives + core + prelude) is all they need.
fn compile(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<elle::CodeUnit, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    elle::pipeline::compile(source, symbols, &mut cctx, source_name)
}

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
    let result = elle::primitives::ctx::with_test_ctx(|ctx| reader.read(ctx, &mut symbols));

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
    let result = elle::primitives::ctx::with_test_ctx(|ctx| reader.read(ctx, &mut symbols));

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
    let result = elle::primitives::ctx::with_test_ctx(|ctx| reader.read(ctx, &mut symbols));

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("1:1")); // Error at position 1:1
    assert!(error.contains("unexpected closing parenthesis"));
}

#[test]
fn test_unterminated_array_location() {
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
    let result = elle::primitives::ctx::with_test_ctx(|ctx| reader.read(ctx, &mut symbols));

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("1:6")); // EOF at position 6
    assert!(error.contains("unterminated array"));
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
    let result = elle::primitives::ctx::with_test_ctx(|ctx| reader.read(ctx, &mut symbols));

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("unterminated struct"));
}

#[test]
fn test_list_sugar_error_location() {
    let mut symbols = SymbolTable::new();
    let input = "@)";

    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();

    while let Ok(Some(token_with_loc)) = lexer.next_token_with_loc() {
        tokens.push(OwnedToken::from(token_with_loc.token));
        locations.push(token_with_loc.loc);
    }

    let mut reader = Reader::with_locations(tokens, locations);
    let result = elle::primitives::ctx::with_test_ctx(|ctx| reader.read(ctx, &mut symbols));

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

// ============================================================================
// LocationMap Tests - Verify bytecode offset to source location mapping
// ============================================================================

#[test]
fn test_location_map_populated_for_simple_expression() {
    let mut symbols = SymbolTable::new();
    let source = "(%add 1 2)";

    let result = compile(source, &mut symbols, "<test>");
    assert!(result.is_ok());

    let compiled = result.unwrap();
    // The location table should have at least one entry
    assert!(
        !compiled.entry().locations().is_empty(),
        "the location table should be populated for compiled code"
    );
}

#[test]
fn test_location_map_has_correct_line_numbers() {
    let mut symbols = SymbolTable::new();
    // Single expression with nested structure
    let source = "(if true\n  (%add 1 2)\n  (%sub 3 4))";

    let result = compile(source, &mut symbols, "<test>");
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());

    let compiled = result.unwrap();
    // Check that we have location entries
    assert!(
        !compiled.entry().locations().is_empty(),
        "the location table should be populated"
    );

    // All entries should have line >= 1 (not synthetic)
    for (_, loc) in compiled.entry().locations().iter() {
        assert!(
            loc.line >= 1,
            "Line numbers should be >= 1, got {}",
            loc.line
        );
    }
}

// ============================================================================
// A signal that reaches the root — which signal the report names
// ============================================================================

#[test]
fn an_unhandled_signal_at_the_root_is_named_by_its_keyword() {
    // A signal no fiber masks travels to the root of the program, where the
    // report is all the author gets. The trap: the root holds `SignalBits`,
    // whose `Display` is hex — and a user signal's bit is above 32, so the
    // report reads `0x100000000`. The counter-factual: assert only `is_err()`,
    // and that hex passes for a signal the author named `:root-unhandled`.
    crate::common::eval_source_unscheduled(
        "(signal :root-unhandled) (emit :root-unhandled 1)",
        |result| {
            let err = result.expect_err("nothing masks the signal, so it reaches the root");
            assert!(
                err.contains(":root-unhandled"),
                "the root must name the unhandled signal, got: {}",
                err
            );
        },
    );
}

#[test]
fn an_uncaught_errors_value_is_reported_with_the_names_it_carries() {
    // An error that no handler catches is printed, and the print is all the
    // author gets. The trap: a keyword the Rust runtime mints from a fixed
    // string (`:error`, `:message`) resolves through the static vocabulary with
    // no memo at all, so a report can read perfectly well while every name the
    // program itself coined prints as `#<keyword:hash>`. The names below are
    // coined here and appear in no vocabulary. The counter-factual: assert only
    // that the report is an error, or that it mentions `:error`, and a report of
    // raw hashes passes for the error the author named.
    crate::common::eval_source(
        "(error {:error :sigil-root-kind :form 'sigil-root-form})",
        |result| {
            let report = result.expect_err("nothing catches the error, so it reaches the root");
            assert!(
                report.contains(":sigil-root-kind"),
                "the report must spell the keyword the error carries, got:\n{}",
                report,
            );
            assert!(
                report.contains("sigil-root-form"),
                "the report must spell the symbol the error carries, got:\n{}",
                report,
            );
            assert!(
                !report.contains("#<keyword:"),
                "no keyword in the report may fall back to its hash, got:\n{}",
                report,
            );
            assert!(
                !report.contains("#<symbol:"),
                "no symbol in the report may fall back to its hash, got:\n{}",
                report,
            );
        },
    );
}

#[test]
fn test_closure_has_location_map() {
    let mut symbols = SymbolTable::new();
    let source = "(fn (x) (numeric!) (%add x 1))";

    let result = compile(source, &mut symbols, "<test>");
    assert!(result.is_ok());

    let compiled = result.unwrap();
    // The entry function has a location table
    assert!(
        !compiled.entry().locations().is_empty(),
        "the entry function should have a location table"
    );
}
