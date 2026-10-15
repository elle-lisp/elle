// audited: 2026-10-14
//! The crate's one error type, the source locations it carries, and the map from bytecode offset to location.
//!
//! src/error/AGENTS.md

pub mod formatting;
mod types;

// Re-export core types
pub use crate::reader::SourceLoc;
pub use types::{ErrorKind, LError, LResult, StackFrame, TraceSource};

/// Mapping from bytecode instruction index to source location
pub type LocationMap = rustc_hash::FxHashMap<usize, SourceLoc>;

/// Parse a "file:line:col: message" error string into components.
/// Returns `Some((file, line, col, message))` on success, `None` if the
/// string doesn't match the expected format.
pub fn parse_located_error(error: &str) -> Option<(&str, usize, usize, &str)> {
    let colon_idx = error.find(": ")?;
    let loc_part = &error[..colon_idx];
    let parts: Vec<&str> = loc_part.rsplitn(3, ':').collect();
    if parts.len() >= 2 {
        let col = parts[0].parse::<usize>().ok()?;
        let line = parts[1].parse::<usize>().ok()?;
        let file = if parts.len() == 3 {
            parts[2]
        } else {
            crate::reader::UNKNOWN_FILE
        };
        let message = &error[colon_idx + 2..];
        Some((file, line, col, message))
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
