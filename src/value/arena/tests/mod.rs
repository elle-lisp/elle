// audited: 2026-09-20
// Re-exports what the arena's own scope holds, so each themed file below reads
// the same names an inline test module would.
//
// docs/impl/region/ownership.md

pub(crate) use super::*;
pub(crate) use crate::value::heap::{HeapObject, Pair};

mod deref;
mod edges;
mod keys;
mod macroscope;
mod refcount;
mod root;
