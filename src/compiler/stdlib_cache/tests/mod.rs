// audited: 2026-10-21
//! The stdlib cache's tests: what the serialized form carries, how the file
//! behaves, and what a later compile reads back.
//!
//! docs/impl/stdlib-cache.md
//!
//! One test file holds each of those three subjects.

mod file;
mod format;
mod fragment;
