// audited: 2026-10-22
//! The mutable stores: each mutable container's contents, read through a guard and written by the funnels.
//!
//! docs/impl/mutable.md
//! docs/impl/region/rules.md
//!
//! A store's read is public and a store's write is visible only inside
//! `value/`. The funnels in `value/arena/mutate.rs` call the writes, each
//! beside the region tracking its store needs, and `value/build.rs` calls the
//! constructors. A write that overlaps a live read guard panics with "already
//! borrowed".

mod array;
mod bytes;
mod cell;
mod set;
mod structs;

pub use array::{ArrayRead, ArrayStore};
pub use bytes::{ByteRead, ByteStore};
pub use cell::CellStore;
pub use set::{SetRead, SetStore};
pub use structs::{StructRead, StructStore};
