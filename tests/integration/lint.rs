/// Integration tests for lint functionality
use elle::lint::cli::{LintConfig, Linter, OutputFormat};
use elle::lint::diagnostics::Severity;

#[test]
fn test_lint_naming_good() {
    let config = LintConfig {
        min_severity: Severity::Warning,
        format: OutputFormat::Human,
    };
    let mut linter = Linter::new(config);

    let result = linter.lint_file("tests/fixtures/naming-good.lisp");
    assert!(result.is_ok());
    assert_eq!(linter.diagnostics().len(), 0);
}

#[test]
fn test_lint_naming_bad() {
    let config = LintConfig {
        min_severity: Severity::Warning,
        format: OutputFormat::Human,
    };
    let mut linter = Linter::new(config);

    let result = linter.lint_file("tests/fixtures/naming-bad.lisp");
    assert!(result.is_ok());
    assert!(!linter.diagnostics().is_empty());

    // All diagnostics should be warnings about naming
    for diag in linter.diagnostics() {
        assert_eq!(diag.rule, "naming-kebab-case");
        assert_eq!(diag.severity, Severity::Warning);
    }
}

#[test]
fn test_json_output() {
    let config = LintConfig {
        min_severity: Severity::Info,
        format: OutputFormat::Json,
    };
    let mut linter = Linter::new(config);

    let result = linter.lint_file("tests/fixtures/naming-bad.lisp");
    assert!(result.is_ok());

    let output = linter.format_output();
    assert!(output.contains("\"diagnostics\""));
    assert!(output.contains("naming-kebab-case"));
}

#[test]
fn test_exit_code_success() {
    let config = LintConfig::default();
    let linter = Linter::new(config);
    assert_eq!(linter.exit_code(), 0);
}

#[test]
fn test_lint_nonexistent_file() {
    let config = LintConfig::default();
    let mut linter = Linter::new(config);

    let result = linter.lint_file("nonexistent.lisp");
    assert!(result.is_err());
}

#[test]
fn test_simple_elle_code() {
    let config = LintConfig::default();
    let mut linter = Linter::new(config);

    let result = linter.lint_str("(+ 1 2 3)", "test.lisp");
    assert!(result.is_ok());
}
