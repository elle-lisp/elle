//! Error type definitions for Elle
//!
//! The module root holds the `LError` wrapper, its stack-trace types, and the
//! core constructors; the bulk lives in submodules re-exported here so every
//! `crate::error::types::<Item>` path resolves unchanged:
//! - `kind`: the `ErrorKind` enum.
//! - `display`: `description`, source-context formatting, `Display`/`Error`, conversions.
//! - `builders`: the per-category `LError::*` constructors.

use crate::reader::SourceLoc;

mod builders;
mod display;
mod kind;

pub use kind::ErrorKind;

/// Stack frame for error traces
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    pub function_name: Option<String>,
    pub location: Option<SourceLoc>,
}

/// Source of stack trace — supports deferred capture
#[derive(Debug, Clone, Default)]
pub enum TraceSource {
    /// No trace available
    #[default]
    None,
    /// Captured from bytecode VM
    Vm(Vec<StackFrame>),
    /// Captured from CPS continuation chain (future)
    Cps(Vec<StackFrame>),
}

/// Unified error type for Elle
#[derive(Debug, Clone)]
pub struct LError {
    pub kind: ErrorKind,
    pub location: Option<SourceLoc>,
    pub trace: TraceSource,
}

/// Result type alias
pub type LResult<T> = Result<T, LError>;

impl LError {
    /// Create a new error with just a kind
    pub fn new(kind: ErrorKind) -> Self {
        LError {
            kind,
            location: None,
            trace: TraceSource::None,
        }
    }

    /// Add location information
    pub fn with_location(mut self, loc: SourceLoc) -> Self {
        self.location = Some(loc);
        self
    }

    /// Add trace information
    pub fn with_trace(mut self, trace: TraceSource) -> Self {
        self.trace = trace;
        self
    }
}
