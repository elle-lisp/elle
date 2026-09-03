//! Source location tracking

use std::fmt;

use super::files::{self, FileId};

/// A span in source code (byte offsets plus line/column for errors).
///
/// `Copy` POD, 20 bytes, no Rust heap allocation: a span rides inside a
/// region-resident [`Syntax`](super::Syntax) node, inside serialized LIR, and
/// inside a closure template's `origin`, so it must be plain bytes
/// (docs/impl/syntax.md § "Span"). The file name lives in the process-wide
/// interner and the span carries its [`FileId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub line: u32,
    pub col: u32,
    file: FileId,
}

impl Span {
    pub fn new(start: usize, end: usize, line: u32, col: u32) -> Self {
        Span {
            start: start as u32,
            end: end as u32,
            line,
            col,
            file: FileId::NONE,
        }
    }

    pub fn with_file(mut self, file: impl AsRef<str>) -> Self {
        self.file = files::intern(file.as_ref());
        self
    }

    /// Create a synthetic span (for generated code)
    pub fn synthetic() -> Self {
        Span::default()
    }

    /// The source file this span came from, or `None` for a synthetic span or
    /// one whose token had no known origin.
    pub fn file(&self) -> Option<&'static str> {
        files::name(self.file)
    }

    /// The interned id of this span's file — the form the packed
    /// representation stores, for callers that compare or key on the file
    /// rather than print it.
    pub fn file_id(&self) -> FileId {
        self.file
    }

    /// Point this span at an already-interned file.
    pub fn set_file_id(&mut self, file: FileId) {
        self.file = file;
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
            file: if self.file.is_some() {
                self.file
            } else {
                other.file
            },
        }
    }
}

impl Span {
    /// Convert to a SourceLoc for error reporting
    pub fn to_source_loc(&self) -> crate::reader::SourceLoc {
        crate::reader::SourceLoc::new(
            self.file().unwrap_or(crate::reader::UNKNOWN_FILE),
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
        match self.file() {
            Some(file) => write!(f, "{}:{}:{}", file, self.line, self.col),
            None => write!(f, "{}:{}", self.line, self.col),
        }
    }
}

// ── serde: the name travels, never the id ────────────────────────────────
//
// A `Span` crosses a process boundary inside serialized LIR — the stdlib disk
// cache and `send`'s bundles — where a `FileId` names nothing. Both impls are
// written by hand for that one reason: `Serialize` writes the spelling, and
// `Deserialize` re-interns it into the receiving process's table.

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename = "Span")]
struct SpanWire<'a> {
    start: u32,
    end: u32,
    line: u32,
    col: u32,
    file: Option<&'a str>,
}

impl serde::Serialize for Span {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        SpanWire {
            start: self.start,
            end: self.end,
            line: self.line,
            col: self.col,
            file: self.file(),
        }
        .serialize(ser)
    }
}

impl<'de> serde::Deserialize<'de> for Span {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        // `String`, not `&str`: bincode's reader is not zero-copy, so a
        // borrowed field fails to deserialize from an owned buffer.
        #[derive(serde::Deserialize)]
        #[serde(rename = "Span")]
        struct Owned {
            start: u32,
            end: u32,
            line: u32,
            col: u32,
            file: Option<String>,
        }
        let w = Owned::deserialize(de)?;
        Ok(Span {
            start: w.start,
            end: w.end,
            line: w.line,
            col: w.col,
            file: w.file.map_or(FileId::NONE, |f| files::intern(&f)),
        })
    }
}

#[cfg(test)]
mod tests;
