// audited: 2026-10-11
//! The arena's tests, one themed file each, reading the names an inline test module would.
//!
//! docs/impl/region/ownership.md

pub(crate) use super::*;
pub(crate) use crate::value::heap::{HeapObject, Pair};

mod deref;
mod edges;
mod keys;
mod macroscope;
mod refcount;
mod root;
mod selfedge;
