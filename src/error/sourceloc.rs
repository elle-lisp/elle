//! Source location tracking

use std::fmt;

/// Source code location (line and column)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLoc {
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for SourceLoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

impl SourceLoc {
    /// Create a new source location
    pub fn new(line: usize, col: usize) -> Self {
        SourceLoc { line, col }
    }

    /// Create a location at the beginning of a file
    pub fn start() -> Self {
        SourceLoc { line: 1, col: 1 }
    }
}
