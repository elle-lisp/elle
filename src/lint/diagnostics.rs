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

/// A warning the linter can raise: its code and the rule name that always
/// travels with it.
///
/// The pair has one home, so a rule cannot pick up a second code — or a code a
/// second name — by being spelled out again at a new emission site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LintCode {
    pub code: &'static str,
    pub rule: &'static str,
}

impl LintCode {
    pub const fn new(code: &'static str, rule: &'static str) -> Self {
        Self { code, rule }
    }
}

impl LintCode {
    /// The code an analysis failure reports under, on **every** surface.
    ///
    /// The lint CLI, `compile/diagnostics`, and the LSP each used to spell this
    /// mapping out for themselves, and they disagreed: a syntax error was
    /// `E005` from `elle lint`, `E000` from `compile/diagnostics` (whose match
    /// had no `SyntaxError` arm at all), and `E0001` from the LSP — a shape the
    /// documented `E00x` scheme does not even have. A consumer that filters or
    /// counts by code could not do it across surfaces.
    ///
    /// Kinds with no code of their own report as [`ANALYSIS_ERROR`], which is
    /// what each site's fallback arm already did.
    pub fn for_error_kind(kind: &crate::error::ErrorKind) -> LintCode {
        use crate::error::ErrorKind;
        match kind {
            ErrorKind::UndefinedVariable { .. } => UNDEFINED_VARIABLE,
            ErrorKind::SignalMismatch { .. } => SIGNAL_MISMATCH,
            ErrorKind::UnterminatedForm { .. } => UNTERMINATED_FORM,
            ErrorKind::CompileError { .. } => COMPILE_ERROR,
            ErrorKind::SyntaxError { .. } => SYNTAX_ERROR,
            _ => ANALYSIS_ERROR,
        }
    }
}

pub const ARITY_MISMATCH: LintCode = LintCode::new("W002", "arity-mismatch");
pub const MUTABLE_BINDING_NEVER_ASSIGNED: LintCode =
    LintCode::new("W003", "mutable-binding-never-assigned");
pub const UNUSED_BINDING: LintCode = LintCode::new("W004", "unused-binding");
pub const NON_TAIL_SELF_RECURSION: LintCode = LintCode::new("W005", "non-tail-self-recursion");

/// Every warning the linter raises, in code order.
///
/// `docs/cookbook/lint-rules.md` publishes this table to rule authors as the
/// index of codes already taken, and `diagnostics/tests.rs` holds the two in
/// step. A rule that lands without its row in the cookbook fails that test.
pub const WARNINGS: &[LintCode] = &[
    ARITY_MISMATCH,
    MUTABLE_BINDING_NEVER_ASSIGNED,
    UNUSED_BINDING,
    NON_TAIL_SELF_RECURSION,
];

pub const ANALYSIS_ERROR: LintCode = LintCode::new("E000", "analysis-error");
pub const UNDEFINED_VARIABLE: LintCode = LintCode::new("E001", "undefined-variable");
pub const SIGNAL_MISMATCH: LintCode = LintCode::new("E002", "signal-mismatch");
pub const UNTERMINATED_FORM: LintCode = LintCode::new("E003", "unterminated-form");
pub const COMPILE_ERROR: LintCode = LintCode::new("E004", "compile-error");
pub const SYNTAX_ERROR: LintCode = LintCode::new("E005", "syntax-error");

/// Every analysis failure a surface reports, in code order.
///
/// These are not rules — no linter pass raises them. They are the codes
/// [`LintCode::for_error_kind`] hands to the lint CLI, `compile/diagnostics`,
/// and the LSP, published to rule authors for the same reason [`WARNINGS`] is:
/// as the index of codes already taken.
pub const ERRORS: &[LintCode] = &[
    ANALYSIS_ERROR,
    UNDEFINED_VARIABLE,
    SIGNAL_MISMATCH,
    UNTERMINATED_FORM,
    COMPILE_ERROR,
    SYNTAX_ERROR,
];

/// A linter diagnostic with source location
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub rule: String,
    pub message: String,
    pub location: Option<SourceLoc>,
    pub suggestions: Vec<String>,
    /// The nearest enclosing named function this diagnostic occurs in, if any.
    /// Stamped by the HIR linter so per-function consumers (e.g. the portrait
    /// system) can attribute a finding to a function exactly, rather than by a
    /// fragile line-range heuristic. `None` for module/top-level findings.
    pub function: Option<String>,
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
            function: None,
        }
    }

    /// A warning built from its registry entry, which is how every rule raises
    /// one: the code and the rule name arrive together and cannot disagree.
    pub fn warn(lint: LintCode, message: impl Into<String>, location: Option<SourceLoc>) -> Self {
        Self::new(Severity::Warning, lint.code, lint.rule, message, location)
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

    /// Format diagnostic with source context
    ///
    /// Includes source line and caret pointing to error location
    pub fn format_with_context(&self, source: &str) -> String {
        let mut output = String::new();

        match &self.location {
            Some(loc) => {
                output.push_str(&format!(
                    "{} [{}] {}\n",
                    self.severity, self.code, self.rule
                ));
                output.push_str(&format!("  --> {}\n", loc.position()));

                // Add source context if available
                if !loc.is_unknown() {
                    if let Some(line) =
                        crate::error::formatting::extract_source_line(source, loc.line)
                    {
                        output.push_str("   |\n");
                        let line_num_str = loc.line.to_string();
                        let padding = " ".repeat(line_num_str.len());
                        output.push_str(&format!(" {} | {}\n", line_num_str, line));
                        output.push_str(&format!(
                            " {} | {}\n",
                            padding,
                            crate::error::formatting::highlight_column(&line, loc.col)
                        ));
                    }
                }
            }
            None => {
                output.push_str(&format!(
                    "{} [{}] {}\n",
                    self.severity, self.code, self.rule
                ));
            }
        }

        output.push_str(&format!("   message: {}\n", self.message));

        if !self.suggestions.is_empty() {
            output.push_str("   help:\n");
            for suggestion in &self.suggestions {
                output.push_str(&format!("     - {}\n", suggestion));
            }
        }

        output
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_human())
    }
}

#[cfg(test)]
mod tests;
