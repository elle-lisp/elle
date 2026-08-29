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
fn the_cookbook_code_table_names_exactly_the_warnings_the_linter_raises() {
    // The cookbook's table is a rule author's index of codes already taken, and
    // it is prose: nothing but this test stops it drifting from `WARNINGS`.
    //
    // The trap: drift is silent in both directions and neither is cosmetic. A
    // code the table lists but no rule raises gets skipped as taken, so the
    // next rule takes a further code and the numbering grows holes. A code a
    // rule raises but the table omits gets handed to a second rule, and two
    // rules then answer to one code.
    const COOKBOOK: &str = include_str!("../../../docs/cookbook/lint-rules.md");

    let documented: Vec<(&str, &str)> = COOKBOOK.lines().filter_map(code_row).collect();
    let raised: Vec<(&str, &str)> = WARNINGS.iter().map(|w| (w.code, w.rule)).collect();

    assert_eq!(
        documented, raised,
        "docs/cookbook/lint-rules.md § Diagnostic codes disagrees with \
         diagnostics::WARNINGS"
    );
}
