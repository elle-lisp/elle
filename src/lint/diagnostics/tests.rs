//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_severity_ordering() {
    assert!(Severity::Info < Severity::Warning);
    assert!(Severity::Warning < Severity::Error);
}

#[test]
fn test_diagnostic_creation() {
    let loc = SourceLoc::from_line_col(5, 2);
    let diag = Diagnostic::new(
        Severity::Warning,
        "W002",
        "arity-mismatch",
        "function expects 1 argument but got 2",
        Some(loc),
    );

    assert_eq!(diag.severity, Severity::Warning);
    assert_eq!(diag.rule, "arity-mismatch");
}

#[test]
fn test_diagnostic_without_location() {
    let diag = Diagnostic::new(Severity::Info, "I001", "test-rule", "test message", None);

    assert_eq!(diag.severity, Severity::Info);
    assert!(diag.location.is_none());
}

/// Read a row of the cookbook's code table as `(code, rule)`.
///
/// Only a row whose first cell holds a code — one of `W`/`E`/`I` and three
/// digits — qualifies. The cookbook has other two-column tables with backticked
/// cells, the key-types table among them, and none of those are code rows.
fn code_row(line: &str) -> Option<(&str, &str)> {
    let mut cells = line.strip_prefix('|')?.split('|').map(str::trim);
    let code = cells.next()?.strip_prefix('`')?.strip_suffix('`')?;
    if code.len() != 4
        || !matches!(code.as_bytes()[0], b'W' | b'E' | b'I')
        || !code[1..].bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let rule = cells.next()?.strip_prefix('`')?.strip_suffix('`')?;
    Some((code, rule))
}

#[test]
fn the_cookbook_code_tables_name_exactly_the_codes_that_are_taken() {
    // The cookbook's tables are a rule author's index of codes already taken,
    // and they are prose: nothing but this test stops them drifting from
    // `WARNINGS` and `ERRORS`.
    //
    // The trap: drift is silent in both directions and neither is cosmetic. A
    // code the table lists but nothing raises gets skipped as taken, so the
    // next rule takes a further code and the numbering grows holes. A code
    // something raises but the table omits gets handed to a second rule, and
    // two rules then answer to one code.
    const COOKBOOK: &str = include_str!("../../../docs/cookbook/lint-rules.md");

    let documented: Vec<(&str, &str)> = COOKBOOK.lines().filter_map(code_row).collect();
    // Warnings first, then errors — the order the two tables appear in.
    let taken: Vec<(&str, &str)> = WARNINGS
        .iter()
        .chain(ERRORS)
        .map(|c| (c.code, c.rule))
        .collect();

    assert_eq!(
        documented, taken,
        "docs/cookbook/lint-rules.md § Diagnostic codes disagrees with \
         diagnostics::WARNINGS ++ diagnostics::ERRORS"
    );
}

// The defect this registry closes: the same failure carried a different code
// depending on which surface reported it. A syntax error was `E005` from
// `elle lint`, `E000` from `compile/diagnostics` — whose match had no
// `SyntaxError` arm and fell through — and `E0001` from the LSP, a four-digit
// shape the documented `E00x` scheme does not have.
//
// The counter-factual: with the mapping written out at each site, this test
// reads three different codes for one kind.
#[test]
fn one_error_kind_reports_one_code() {
    use crate::error::ErrorKind;

    let syntax = ErrorKind::SyntaxError {
        message: "unbalanced".to_string(),
        line: Some(1),
    };
    assert_eq!(LintCode::for_error_kind(&syntax), SYNTAX_ERROR);
    assert_eq!(SYNTAX_ERROR.code, "E005");

    // Every code the mapping can produce is a code the tables publish, so a
    // consumer filtering by the documented index cannot miss one.
    for kind in [
        ErrorKind::SyntaxError {
            message: String::new(),
            line: None,
        },
        ErrorKind::UndefinedVariable {
            name: String::new(),
            suggestions: Vec::new(),
        },
        ErrorKind::DivisionByZero,
    ] {
        let code = LintCode::for_error_kind(&kind);
        assert!(
            ERRORS.contains(&code),
            "{:?} maps to {}, which no table publishes",
            kind,
            code.code
        );
    }
}
