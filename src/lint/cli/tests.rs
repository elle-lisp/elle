//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_linter_creation() {
    let config = LintConfig::default();
    let linter = Linter::new(config);
    assert_eq!(linter.exit_code(), 0);
}

#[test]
fn test_lint_simple_code() {
    crate::value::arena::with_test_region(|| {
        let config = LintConfig {
            min_severity: Severity::Warning,
            ..Default::default()
        };
        let mut linter = Linter::new(config);

        let result = linter.lint_str("(+ 1 2)", "test.lisp");
        assert!(result.is_ok());
    });
}
