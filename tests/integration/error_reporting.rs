// Tests for error reporting with source locations
//
// Verifies that parse errors include file name, line number, and column
// information, and that a runtime error reaching the root is printed with the
// location of the form that raised it.

use elle::reader::{Lexer, OwnedToken, Reader};
use elle::SymbolTable;

// Local `compile` shim preserving the pre-CompileCtx arity. These location-map
// tests are compile-only and stdlib-free, so a fresh `CompileCtx` per call
// (primitives + core + prelude) reproduces the old bare-symbols path.
fn compile(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<elle::CompileResult, String> {
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
    let result =
        elle::primitives::ctx::with_test_ctx(|ctx| reader.read(ctx, &mut symbols));

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
    let result =
        elle::primitives::ctx::with_test_ctx(|ctx| reader.read(ctx, &mut symbols));

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
    let result =
        elle::primitives::ctx::with_test_ctx(|ctx| reader.read(ctx, &mut symbols));

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
    let result =
        elle::primitives::ctx::with_test_ctx(|ctx| reader.read(ctx, &mut symbols));

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
    let result =
        elle::primitives::ctx::with_test_ctx(|ctx| reader.read(ctx, &mut symbols));

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
    let result =
        elle::primitives::ctx::with_test_ctx(|ctx| reader.read(ctx, &mut symbols));

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
    // The location map should have at least one entry
    assert!(
        !compiled.bytecode.location_map.is_empty(),
        "LocationMap should be populated for compiled code"
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

// ============================================================================
// Runtime traceback locations — which form an uncaught error is attributed to
// ============================================================================

/// Run a fixture script that is expected to die on an uncaught error, and
/// return its `(stdout, stderr)`.
///
/// Panics when the script exits 0: every fixture here must reach the root with
/// an error, or the location assertions below have nothing to inspect.
fn run_failing_fixture(name: &str) -> (String, String) {
    let script = format!("tests/integration/fixtures/{}", name);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_elle"))
        .arg(&script)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn elle for {}: {}", script, e));
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        !output.status.success(),
        "{} was expected to die on an uncaught error, but exited 0:\nstdout: {}\nstderr: {}",
        script,
        stdout,
        stderr,
    );
    (stdout, stderr)
}

/// The 1-based line of the single code line of `name` that contains `marker`.
///
/// The fixtures carry one marker keyword per raising form, so the assertions
/// name forms by what they raise rather than by a line number that any edit to
/// the fixture would silently invalidate. Comment lines are skipped: each
/// fixture's header names its own markers.
fn fixture_line_of(name: &str, marker: &str) -> usize {
    let script = format!("tests/integration/fixtures/{}", name);
    let source = std::fs::read_to_string(&script)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", script, e));
    let hits: Vec<usize> = source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with('#') && line.contains(marker))
        .map(|(i, _)| i + 1)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "{} must contain {} on exactly one line, found it on {:?}",
        script,
        marker,
        hits,
    );
    hits[0]
}

/// The line number in the sole `  at <file>:<line>:<col>` line of `stderr`.
///
/// Returns `None` when the error was printed with no location at all — which
/// is itself a failure for these fixtures, and is reported as such by the
/// callers rather than silently passing.
fn reported_line(stderr: &str, fixture: &str) -> Option<usize> {
    let needle = format!("{}:", fixture);
    stderr.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix("at ")?;
        let tail = rest.split(&needle).nth(1)?;
        tail.split(':').next()?.parse().ok()
    })
}

#[test]
fn an_uncaught_error_is_attributed_to_the_form_that_raised_it() {
    const FIXTURE: &str = "traceback-plain-raise.lisp";
    let raised = fixture_line_of(FIXTURE, ":only");
    let (_, stderr) = run_failing_fixture(FIXTURE);

    assert_eq!(
        reported_line(&stderr, FIXTURE),
        Some(raised),
        "the error must be reported at the raising form (line {}); stderr was:\n{}",
        raised,
        stderr,
    );
}

#[test]
fn a_handled_error_does_not_lend_its_location_to_a_later_one() {
    // The trap: the recorded location is first-writer-wins so that the raising
    // frame beats the frames it unwinds through. Nothing else clears it, so a
    // location that outlives its own error is reported for every error after
    // it. The counter-factual: assert only that *some* location is printed and
    // this passes while naming a form from a completely unrelated error.
    const FIXTURE: &str = "traceback-after-catch.lisp";
    let handled = fixture_line_of(FIXTURE, ":first");
    let raised = fixture_line_of(FIXTURE, ":second");
    let (_, stderr) = run_failing_fixture(FIXTURE);

    let reported = reported_line(&stderr, FIXTURE);
    assert_ne!(
        reported,
        Some(handled),
        "line {} raised an error that `protect` handled; it must not be \
         reported for the later uncaught one. stderr was:\n{}",
        handled,
        stderr,
    );
    assert_eq!(
        reported,
        Some(raised),
        "the uncaught error must be reported at the raising form (line {}); \
         stderr was:\n{}",
        raised,
        stderr,
    );
}

#[test]
fn an_error_re_propagated_by_defer_still_names_the_form_that_raised_it() {
    // The trap: `defer` catches with a fiber mask, runs its cleanup, then
    // re-raises. The catch ends the first propagation, so a location that only
    // lives in the VM would be gone by the re-raise and the `(defer …)` form
    // would take the blame. The counter-factual: assert merely that some
    // location is printed, and the `(defer …)` line passes for the raise it
    // wraps — including when the raise is several frames deeper.
    const FIXTURE: &str = "traceback-through-defer.lisp";
    let handled = fixture_line_of(FIXTURE, ":first");
    let raised = fixture_line_of(FIXTURE, ":second");
    let (stdout, stderr) = run_failing_fixture(FIXTURE);

    assert!(
        stdout.contains("cleanup ran"),
        "the defer cleanup must run before the error reaches the root; \
         stdout was:\n{}",
        stdout,
    );
    let reported = reported_line(&stderr, FIXTURE);
    assert_ne!(
        reported,
        Some(handled),
        "line {} raised an error that `protect` handled; it must not be \
         reported for the error `defer` re-propagates. stderr was:\n{}",
        handled,
        stderr,
    );
    assert_eq!(
        reported,
        Some(raised),
        "the re-propagated error must still be reported at the raising form \
         (line {}); stderr was:\n{}",
        raised,
        stderr,
    );
}

#[test]
fn an_unjoined_fibers_error_names_the_form_that_raised_it() {
    // The trap: the scheduler catches a spawned fiber's error, then surfaces
    // it from `:pump` long after the raising frame is gone. The counter-factual:
    // surface it as `(error (fiber/value f))` and every orphaned fiber's error
    // is reported at that one stdlib line instead.
    const FIXTURE: &str = "traceback-unjoined-spawn.lisp";
    let raised = fixture_line_of(FIXTURE, ":orphan");
    let (_, stderr) = run_failing_fixture(FIXTURE);

    assert_eq!(
        reported_line(&stderr, FIXTURE),
        Some(raised),
        "the spawned fiber's error must be reported at the raising form \
         (line {}); stderr was:\n{}",
        raised,
        stderr,
    );
}

#[test]
fn test_closure_has_location_map() {

    let mut symbols = SymbolTable::new();
    let source = "(fn (x) (numeric!) (%add x 1))";

    let result = compile(source, &mut symbols, "<test>");
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
            let _ = closure.template.location_map.len(); // Access to verify field exists
        }
    }
}

