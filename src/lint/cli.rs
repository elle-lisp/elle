//! Lint CLI wrapper — configuration, output formatting, and the Linter type.

use crate::context::SymbolTableGuard;
use crate::hir::HirLinter;
use crate::lint::diagnostics::{Diagnostic, Severity};
use crate::symbol::SymbolTable;
use crate::{analyze_file, init_stdlib, register_primitives, VM};

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
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let _signals = register_primitives(&mut vm, &mut symbols);
        let _sym_guard = SymbolTableGuard::new(&mut symbols);
        init_stdlib(&mut vm, &mut symbols);

        // Use pipeline: parse -> expand -> analyze -> HIR
        let source_name = if filename.is_empty() {
            "<lint>"
        } else {
            filename
        };
        let analysis = match analyze_file(code, &mut symbols, &mut vm, source_name) {
            Ok(a) => a,
            Err(e) => {
                // Convert fatal analysis error to a diagnostic instead of propagating
                self.diagnostics
                    .push(Self::error_to_diagnostic(&e, source_name));
                return Ok(());
            }
        };

        // Convert accumulated analysis errors to diagnostics
        for error in &analysis.errors {
            self.diagnostics
                .push(Self::lerror_to_diagnostic(error, source_name));
        }

        // Lint the analyzed file
        let mut hir_linter = HirLinter::new();
        hir_linter.lint(&analysis.hir, &symbols, &analysis.arena);
        self.diagnostics
            .extend(hir_linter.diagnostics().iter().cloned());

        Ok(())
    }

    /// Convert an LError to a Diagnostic
    fn lerror_to_diagnostic(error: &crate::error::LError, file: &str) -> Diagnostic {
        use crate::error::ErrorKind;
        let (code, rule) = match &error.kind {
            ErrorKind::UndefinedVariable { .. } => ("E001", "undefined-variable"),
            ErrorKind::SignalMismatch { .. } => ("E002", "signal-mismatch"),
            ErrorKind::UnterminatedForm { .. } => ("E003", "unterminated-form"),
            ErrorKind::CompileError { .. } => ("E004", "compile-error"),
            ErrorKind::SyntaxError { .. } => ("E005", "syntax-error"),
            _ => ("E000", "analysis-error"),
        };
        let loc = error
            .location
            .clone()
            .unwrap_or_else(|| crate::reader::SourceLoc::new(file, 0, 0));
        Diagnostic::new(Severity::Error, code, rule, error.description(), Some(loc))
    }

    fn error_to_diagnostic(error: &str, file: &str) -> Diagnostic {
        if let Some((f, line, col, message)) = crate::error::parse_located_error(error) {
            Diagnostic::new(
                Severity::Error,
                "E000",
                "analysis-error",
                message,
                Some(crate::reader::SourceLoc::new(f, line, col)),
            )
        } else {
            Diagnostic::new(
                Severity::Error,
                "E000",
                "analysis-error",
                error,
                Some(crate::reader::SourceLoc::new(file, 0, 0)),
            )
        }
    }

    /// Lint a file
    pub fn lint_file(&mut self, path: &str) -> Result<(), String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

        self.lint_str(&content, path)
    }

    /// Get all diagnostics
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Check if there are any errors
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Check if there are any warnings
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning)
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
        if self.has_errors() {
            1
        } else if self.has_warnings() {
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
        let config = LintConfig {
            min_severity: Severity::Warning,
            ..Default::default()
        };
        let mut linter = Linter::new(config);

        let result = linter.lint_str("(+ 1 2)", "test.lisp");
        assert!(result.is_ok());
    }
}
