// audited: 2026-09-23
// The cache has three subjects and one test file each: what the serialized form
// carries across, how the file on disk behaves, and what a later compile reads
// back out of the functions it restored.
// docs/impl/stdlib-cache.md

mod file;
mod format;
mod fragment;
