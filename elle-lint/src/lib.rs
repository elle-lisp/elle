//! Elle Linter - Opinionated static analysis for Elle Lisp
//!
//! Provides comprehensive linting rules for Elle code including:
//! - Naming conventions
//! - Arity validation
//! - Unused variable detection
//! - Pattern matching validation
//! - Module boundary checking

pub mod context;
pub mod diagnostics;
pub mod rules;

use diagnostics::{Diagnostic, Severity};
use elle::value::Value;
use std::path::Path;

/// Main linter configuration
#[derive(Debug, Clone)]
pub struct LintConfig {
    pub min_severity: Severity,
    pub format: OutputFormat,
}

#[derive(Debug, Clone)]
pub enum OutputFormat {
    Human,
    Json,
}

impl Default for LintConfig {
    fn default() -> Self {
        Self {
            min_severity: Severity::Info,
            format: OutputFormat::Human,
        }
    }
}

/// Main linter instance
pub struct Linter {
    config: LintConfig,
    diagnostics: Vec<Diagnostic>,
}

impl Linter {
    pub fn new(config: LintConfig) -> Self {
        Self {
            config,
            diagnostics: Vec::new(),
        }
    }

    /// Lint Elle code from a string
    pub fn lint_str(&mut self, code: &str, filename: &str) -> Result<(), String> {
        let mut symbols = elle::SymbolTable::new();
        elle::register_primitives(&mut elle::VM::new(), &mut symbols);
        elle::init_stdlib(&mut elle::VM::new(), &mut symbols);

        // Parse code
        let mut lexer = elle::Lexer::new(code);
        let mut tokens = Vec::new();
        loop {
            match lexer.next_token() {
                Ok(Some(token)) => tokens.push(token),
                Ok(None) => break,
                Err(e) => return Err(format!("Parse error: {}", e)),
            }
        }

        let mut reader = elle::Reader::new(tokens);
        let mut values = Vec::new();
        while let Some(result) = reader.try_read(&mut symbols) {
            match result {
                Ok(value) => values.push(value),
                Err(e) => return Err(format!("Read error: {}", e)),
            }
        }

        // Apply linting rules
        for (i, value) in values.iter().enumerate() {
            let line = i + 1; // Approximate line numbering
            self.check_value(value, filename, line, &symbols);
        }

        Ok(())
    }

    /// Lint a file
    pub fn lint_file(&mut self, path: &Path) -> Result<(), String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

        let filename = path.to_str().unwrap_or("unknown").to_string();

        self.lint_str(&content, &filename)
    }

    fn check_value(
        &mut self,
        value: &Value,
        filename: &str,
        line: usize,
        symbols: &elle::SymbolTable,
    ) {
        // Apply all rules
        rules::check_naming_conventions(value, filename, line, &mut self.diagnostics, symbols);
    }

    /// Get all diagnostics
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Format diagnostics for output
    pub fn format_output(&self) -> String {
        match self.config.format {
            OutputFormat::Human => self.format_human(),
            OutputFormat::Json => self.format_json(),
        }
    }

    fn format_human(&self) -> String {
        let mut output = String::new();
        for diag in &self.diagnostics {
            if diag.severity >= self.config.min_severity {
                output.push_str(&diag.format_human());
            }
        }
        output
    }

    fn format_json(&self) -> String {
        let diagnostics: Vec<_> = self
            .diagnostics
            .iter()
            .filter(|d| d.severity >= self.config.min_severity)
            .map(|d| d.to_json())
            .collect();

        serde_json::to_string_pretty(&serde_json::json!({
            "diagnostics": diagnostics
        }))
        .unwrap_or_default()
    }

    /// Get exit code (0 = no errors, 1 = errors, 2 = warnings)
    pub fn exit_code(&self) -> i32 {
        if self
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
        {
            1
        } else if self
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning)
        {
            2
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linter_creation() {
        let config = LintConfig::default();
        let linter = Linter::new(config);
        assert_eq!(linter.exit_code(), 0);
    }

    #[test]
    fn test_lint_simple_code() {
        let mut config = LintConfig::default();
        config.min_severity = Severity::Warning;
        let mut linter = Linter::new(config);

        let result = linter.lint_str("(+ 1 2)", "test.lisp");
        assert!(result.is_ok());
    }
}
