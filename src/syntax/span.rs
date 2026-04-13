//! Source location tracking

use std::fmt;

/// A span in source code (byte offsets plus line/column for errors)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub col: u32,
    pub file: Option<String>,
}

impl Span {
    pub fn new(start: usize, end: usize, line: u32, col: u32) -> Self {
        Span {
            start,
            end,
            line,
            col,
            file: None,
        }
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    /// Create a synthetic span (for generated code)
    pub fn synthetic() -> Self {
        Span {
            start: 0,
            end: 0,
            line: 0,
            col: 0,
            file: None,
        }
    }

    /// Merge two spans into one covering both
    pub fn merge(&self, other: &Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            line: self.line.min(other.line),
            col: if self.line < other.line {
                self.col
            } else if self.line > other.line {
                other.col
            } else {
                self.col.min(other.col)
            },
            file: self.file.clone().or_else(|| other.file.clone()),
        }
    }
}

impl Span {
    /// Convert to a SourceLoc for error reporting
    pub fn to_source_loc(&self) -> crate::reader::SourceLoc {
        crate::reader::SourceLoc::new(
            self.file.clone().unwrap_or_else(|| "<unknown>".to_string()),
            self.line as usize,
            self.col as usize,
        )
    }

    /// Create an LError with CompileError kind and this span's location
    pub fn compile_err(&self, msg: impl Into<String>) -> crate::error::LError {
        crate::error::LError::compile_error(msg).with_location(self.to_source_loc())
    }

    /// Create an LError with UndefinedVariable kind and this span's location
    pub fn undefined_var(&self, name: impl Into<String>) -> crate::error::LError {
        crate::error::LError::undefined_variable(name).with_location(self.to_source_loc())
    }

    /// Create an LError with UndefinedVariable kind, suggestions, and this span's location
    pub fn undefined_var_suggest(
        &self,
        name: impl Into<String>,
        suggestions: Vec<String>,
    ) -> crate::error::LError {
        crate::error::LError::undefined_variable_with_suggestions(name, suggestions)
            .with_location(self.to_source_loc())
    }

    /// Create a SignalMismatch LError with this span's location
    pub fn signal_mismatch(
        &self,
        function: impl Into<String>,
        required_mask: impl Into<String>,
        actual_mask: impl Into<String>,
    ) -> crate::error::LError {
        crate::error::LError::signal_mismatch(function, required_mask, actual_mask)
            .with_location(self.to_source_loc())
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.file {
            Some(file) => write!(f, "{}:{}:{}", file, self.line, self.col),
            None => write!(f, "{}:{}", self.line, self.col),
        }
    }
}
