// audited: 2026-10-20
//! Seq protocol: centralized dispatch for ordered, indexable sequences.
//!
//! Seq extends Collection — these operations apply only to types with a
//! defined element order: list, (), array, @array, string, @string,
//! bytes, @bytes.  Not sets or structs (unordered).
//!
//! Every operation that may allocate takes the call's `ctx`
//! (docs/impl/region/ctx.md): results are born in the caller's region
//! (Rule 3), through the allocation capability the primitive wrapper hands
//! down — the region is named and passed down explicitly.
//!
//! Container dispatch/build helpers live in `helpers`; read-only accessors in
//! `query`; `slice` in `slice`; in-place mutation in `mutate`. The seq ops are
//! re-exported here so external callers name `crate::primitives::seq::seq_*`.
use crate::primitives::ctx::NativeCtx;
use crate::value::Value;

mod helpers;
mod mutate;
mod query;
mod slice;

use helpers::*;
pub use mutate::*;
pub use query::*;
pub use slice::*;
