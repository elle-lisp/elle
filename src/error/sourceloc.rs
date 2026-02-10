//! Source location tracking
//!
//! Re-exports SourceLoc from the reader module where it's primarily defined.
//! SourceLoc tracks file, line, and column information for error reporting.

pub use crate::reader::SourceLoc;
