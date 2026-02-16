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

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.file {
            Some(file) => write!(f, "{}:{}:{}", file, self.line, self.col),
            None => write!(f, "{}:{}", self.line, self.col),
        }
    }
}
