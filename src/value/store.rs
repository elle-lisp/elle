// audited: 2026-10-23
//! The mutable stores: each mutable container's contents, read through a guard and written by the funnels.
//!
//! docs/impl/mutable.md
//! docs/impl/region/rules.md
//!
//! A store's read is public and a store's write is visible only inside
//! `value/`. The funnels in `value/arena/mutate.rs` call the writes, each
//! beside the region tracking its store needs, and `value/build/mutable.rs`
//! calls the constructors. A write that overlaps a live read guard panics with
//! "already borrowed". The `@array`, `@string` and `@bytes` stores are one
//! extent type, generic over what it holds, and the `@set` store wraps it to
//! keep its members in order. A box's or capture cell's store is its one value,
//! inline in the shell.

mod array;
mod bytes;
mod cell;
mod extent;
mod set;
mod structs;

pub use array::{ArrayRead, ArrayStore};
pub use bytes::{ByteRead, ByteStore};
pub use cell::CellStore;
pub use extent::{Extent, ExtentRead};
pub use set::{SetRead, SetStore};
pub use structs::{StructRead, StructStore};
