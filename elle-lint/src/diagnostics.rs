//! Diagnostic types for linter violations

use serde_json::{json, Value};
use std::fmt;

/// Severity level of a diagnostic
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Info => write!(f, "info"),
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
        }
    }
}

/// A linter diagnostic
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub rule: String,
    pub message: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub context: String,
    pub suggestions: Vec<String>,
}

impl Diagnostic {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        severity: Severity,
        code: impl Into<String>,
        rule: impl Into<String>,
        message: impl Into<String>,
        file: impl Into<String>,
        line: usize,
        column: usize,
        context: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            code: code.into(),
            rule: rule.into(),
            message: message.into(),
            file: file.into(),
            line,
            column,
            context: context.into(),
            suggestions: Vec::new(),
        }
    }

    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions = suggestions;
        self
    }

    /// Format as human-readable output
    pub fn format_human(&self) -> String {
        let mut output = String::new();

        // Main error line
        output.push_str(&format!(
            "{}:{}:{} {}: {}\n",
            self.file, self.line, self.column, self.severity, self.rule
        ));

        // Context with arrow
        output.push_str(&format!("  --> {}:{}\n", self.file, self.line));
        output.push_str("    |\n");
        output.push_str(&format!("  {} | {}\n", self.line, self.context));

        // Calculate spaces for caret
        let spaces = " ".repeat(self.column.saturating_sub(1));
        output.push_str(&format!("    | {}{}\n", spaces, "^"));

        // Message
        output.push_str(&format!("\n{}\n", self.message));

        // Suggestions
        if !self.suggestions.is_empty() {
            output.push_str("suggestions:\n");
            for suggestion in &self.suggestions {
                output.push_str(&format!("  - {}\n", suggestion));
            }
        }

        output.push('\n');
        output
    }

    /// Convert to JSON representation
    pub fn to_json(&self) -> Value {
        json!({
            "severity": self.severity.to_string(),
            "code": self.code,
            "rule": self.rule,
            "message": self.message,
            "file": self.file,
            "line": self.line,
            "column": self.column,
            "context": self.context,
            "suggestions": self.suggestions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn test_diagnostic_creation() {
        let diag = Diagnostic::new(
            Severity::Error,
            "E001".to_string(),
            "undefined-function".to_string(),
            "function 'foo' is not defined".to_string(),
            "test.l".to_string(),
            5,
            2,
            "(foo 42)".to_string(),
        );

        assert_eq!(diag.severity, Severity::Error);
        assert_eq!(diag.rule, "undefined-function");
        assert_eq!(diag.line, 5);
    }

    #[test]
    fn test_diagnostic_with_suggestions() {
        let diag = Diagnostic::new(
            Severity::Error,
            "E001".to_string(),
            "undefined-function".to_string(),
            "function 'foo' is not defined".to_string(),
            "test.l".to_string(),
            5,
            2,
            "(foo 42)".to_string(),
        )
        .with_suggestions(vec!["for".to_string(), "floor".to_string()]);

        assert_eq!(diag.suggestions.len(), 2);
    }
}
