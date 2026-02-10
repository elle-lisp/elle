//! Diagnostic types for linter violations

use crate::reader::SourceLoc;
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

/// A linter diagnostic with source location
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub rule: String,
    pub message: String,
    pub location: Option<SourceLoc>,
    pub suggestions: Vec<String>,
}

impl Diagnostic {
    pub fn new(
        severity: Severity,
        code: impl Into<String>,
        rule: impl Into<String>,
        message: impl Into<String>,
        location: Option<SourceLoc>,
    ) -> Self {
        Self {
            severity,
            code: code.into(),
            rule: rule.into(),
            message: message.into(),
            location,
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

        match &self.location {
            Some(loc) => {
                output.push_str(&format!(
                    "{}:{} {}: {}\n",
                    loc.line, loc.col, self.severity, self.rule
                ));
                output.push_str(&format!("  message: {}\n", self.message));
            }
            None => {
                output.push_str(&format!("{}: {}\n", self.severity, self.rule));
                output.push_str(&format!("  message: {}\n", self.message));
            }
        }

        if !self.suggestions.is_empty() {
            output.push_str("  suggestions:\n");
            for suggestion in &self.suggestions {
                output.push_str(&format!("    - {}\n", suggestion));
            }
        }

        output
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
        let loc = SourceLoc::from_line_col(5, 2);
        let diag = Diagnostic::new(
            Severity::Warning,
            "W001",
            "naming-kebab-case",
            "identifier should use kebab-case",
            Some(loc),
        );

        assert_eq!(diag.severity, Severity::Warning);
        assert_eq!(diag.rule, "naming-kebab-case");
    }

    #[test]
    fn test_diagnostic_without_location() {
        let diag = Diagnostic::new(Severity::Info, "I001", "test-rule", "test message", None);

        assert_eq!(diag.severity, Severity::Info);
        assert!(diag.location.is_none());
    }
}
