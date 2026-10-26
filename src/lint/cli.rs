// audited: 2026-10-26
//! The linter `elle lint` drives: its configuration, the diagnostics of each file it reads, and their output.
//!
//! src/lint/README.md

use crate::analyze_file;
use crate::hir::HirLinter;
use crate::lint::diagnostics::{Diagnostic, Severity};
use crate::runtime::Runtime;

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

/// A diagnostic and the path the linter was given for its file. A rule's
/// location carries no file, so the output names this path instead.
struct Finding {
    file: String,
    diagnostic: Diagnostic,
}

/// Main linter instance
pub struct Linter {
    config: LintConfig,
    findings: Vec<Finding>,
}

impl Linter {
    pub fn new(config: LintConfig) -> Self {
        Self {
            config,
            findings: Vec::new(),
        }
    }

    /// Lint Elle code from a string.
    ///
    /// Drives a full [`Runtime`] per call, as file-run, the REPL and embedding
    /// do: primitives and stdlib on construction, and the teardown sweep that
    /// frees every region on drop (docs/impl/region/rules.md).
    pub fn lint_str(&mut self, code: &str, filename: &str) -> Result<(), String> {
        let mut rt = Runtime::new();
        let (vm, symbols, cctx) = rt.parts();

        // Use pipeline: parse -> expand -> analyze -> HIR
        let source_name = if filename.is_empty() {
            "<lint>"
        } else {
            filename
        };
        let mut found = Vec::new();
        match analyze_file(code, symbols, vm, cctx, source_name) {
            Ok(analysis) => {
                // Convert accumulated analysis errors to diagnostics
                for error in &analysis.errors {
                    found.push(Self::lerror_to_diagnostic(error, source_name));
                }

                // Lint the analyzed file
                let mut hir_linter = HirLinter::new();
                hir_linter.lint(&analysis.hir, rt.symbols(), &analysis.arena);
                found.extend(hir_linter.diagnostics().iter().cloned());
            }
            // A fatal analysis error is a diagnostic, not a failure to lint.
            Err(e) => found.push(Self::error_to_diagnostic(&e, source_name)),
        }

        self.findings
            .extend(found.into_iter().map(|diagnostic| Finding {
                file: source_name.to_string(),
                diagnostic,
            }));
        Ok(())
    }

    /// Convert an LError to a Diagnostic
    fn lerror_to_diagnostic(error: &crate::error::LError, file: &str) -> Diagnostic {
        let crate::lint::diagnostics::LintCode { code, rule } =
            crate::lint::diagnostics::LintCode::for_error_kind(&error.kind);
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

    /// Every diagnostic, in the order the files were linted.
    pub fn diagnostics(&self) -> Vec<&Diagnostic> {
        self.findings.iter().map(|f| &f.diagnostic).collect()
    }

    /// Check if there are any errors
    pub fn has_errors(&self) -> bool {
        self.findings
            .iter()
            .any(|f| f.diagnostic.severity == Severity::Error)
    }

    /// Check if there are any warnings
    pub fn has_warnings(&self) -> bool {
        self.findings
            .iter()
            .any(|f| f.diagnostic.severity == Severity::Warning)
    }

    /// Format diagnostics for output
    pub fn format_output(&self) -> String {
        match self.config.format {
            OutputFormat::Human => self.format_human(),
            OutputFormat::Json => self.format_json(),
        }
    }

    /// The findings at or above the configured level.
    fn shown(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.diagnostic.severity >= self.config.min_severity)
    }

    fn format_human(&self) -> String {
        self.shown()
            .map(|f| f.diagnostic.format_human(&f.file))
            .collect()
    }

    fn format_json(&self) -> String {
        let diagnostics: Vec<_> = self
            .shown()
            .map(
                |Finding {
                     file,
                     diagnostic: d,
                 }| {
                    let (line, col) = match &d.location {
                        Some(loc) => (loc.line as u32, loc.col as u32),
                        None => (0, 0),
                    };
                    serde_json::json!({
                        "file": file,
                        "severity": d.severity.to_string(),
                        "code": d.code,
                        "rule": d.rule,
                        "message": d.message,
                        "line": line,
                        "column": col,
                        "suggestions": d.suggestions,
                    })
                },
            )
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
mod tests;
