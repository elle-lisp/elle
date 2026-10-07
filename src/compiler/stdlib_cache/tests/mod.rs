// audited: 2026-10-06
// The stdlib cache's tests: what the serialized form carries, how the file
// behaves, and what a later compile reads back.
//
// One test file holds each of those three subjects.
// docs/impl/stdlib-cache.md

mod file;
mod format;
mod fragment;
