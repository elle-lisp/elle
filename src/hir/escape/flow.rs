//! The escape value-flow engine: the flow atoms, the interprocedural arg-return
//! summary, the tail/return seed collectors, and the backward-edge collection
//! that `analyze_escape` drives to a fixpoint. The analysis as a whole — its
//! facets, consumers, and precision characteristics — is documented in the parent
//! module (`super`, escape.rs).
//!
//! Split by concern into submodules; this root re-exports every item so the
//! `crate::hir::escape::flow::<Item>` paths the parent uses resolve unchanged:
//!   - `atom`    — the `Atom` flow value and the interprocedural `TailCtx`.
//!   - `summary` — the arg-return fixpoint (`compute_arg_return`).
//!   - `sources` — the source-collection walks (`tail_sources`, `return_atoms`,
//!     `record_frontier_sites`).
//!   - `collect` — the edge/seed collection (`collect_flow`).

mod atom;
mod collect;
mod sources;
mod summary;

pub(super) use atom::{Atom, TailCtx};
pub(super) use collect::collect_flow;
pub(super) use sources::{record_frontier_sites, return_atoms};
pub(super) use summary::compute_arg_return;
