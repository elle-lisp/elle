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
