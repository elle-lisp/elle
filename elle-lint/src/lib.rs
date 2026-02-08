//! Elle Linter - Opinionated static analysis for Elle Lisp
//!
//! Provides comprehensive linting rules for Elle code including:
//! - Naming conventions
//! - Arity validation
//! - Unused variable detection
//! - Pattern matching validation
//!
//! This crate provides a wrapper around the compiler's integrated linter.

pub use elle::compiler::linter::diagnostics::{Diagnostic, Severity};

use elle::compiler::ast::ExprWithLoc;
use elle::compiler::converters::value_to_expr;
use elle::compiler::linter::Linter as CompilerLinter;
use elle::symbol::SymbolTable;
use elle::{init_stdlib, register_primitives, VM};
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
    compiler_linter: CompilerLinter,
}

impl Linter {
    pub fn new(config: LintConfig) -> Self {
        Self {
            config,
            compiler_linter: CompilerLinter::new(),
        }
    }

    /// Lint Elle code from a string
    pub fn lint_str(&mut self, code: &str, _filename: &str) -> Result<(), String> {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        register_primitives(&mut vm, &mut symbols);
        init_stdlib(&mut vm, &mut symbols);

        // Parse code using lexer and reader to handle multiple forms
        let mut lexer = elle::Lexer::new(code);
        let mut tokens = Vec::new();
        loop {
            match lexer.next_token() {
                Ok(Some(token)) => tokens.push(token),
                Ok(None) => break,
                Err(e) => return Err(format!("Lex error: {}", e)),
            }
        }

        let mut reader = elle::Reader::new(tokens);
        while let Some(result) = reader.try_read(&mut symbols) {
            match result {
                Ok(value) => {
                    // Convert value to expr for linting
                    let expr = value_to_expr(&value, &mut symbols)
                        .map_err(|e| format!("Conversion error: {}", e))?;
                    // Wrap in ExprWithLoc with no location info (we don't have precise source locations from Value)
                    let expr_with_loc = ExprWithLoc::new(expr, None);
                    self.compiler_linter.lint_expr(&expr_with_loc, &symbols);
                }
                Err(e) => return Err(format!("Read error: {}", e)),
            }
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

    /// Get all diagnostics
    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.compiler_linter.diagnostics()
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
        for diag in self.diagnostics() {
            if diag.severity >= self.config.min_severity {
                output.push_str(&diag.format_human());
            }
        }
        output
    }

    fn format_json(&self) -> String {
        let diagnostics: Vec<_> = self
            .diagnostics()
            .iter()
            .filter(|d| d.severity >= self.config.min_severity)
            .map(|d| {
                let (line, col) = match &d.location {
                    Some(loc) => (loc.line as u32, loc.col as u32),
                    None => (0, 0),
                };
                serde_json::json!({
                    "severity": d.severity.to_string(),
                    "code": d.code,
                    "rule": d.rule,
                    "message": d.message,
                    "line": line,
                    "column": col,
                    "suggestions": d.suggestions,
                })
            })
            .collect();

        serde_json::to_string_pretty(&serde_json::json!({
            "diagnostics": diagnostics
        }))
        .unwrap_or_default()
    }

    /// Get exit code (0 = no errors, 1 = errors, 2 = warnings)
    pub fn exit_code(&self) -> i32 {
        if self
            .diagnostics()
            .iter()
            .any(|d| d.severity == Severity::Error)
        {
            1
        } else if self
            .diagnostics()
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
