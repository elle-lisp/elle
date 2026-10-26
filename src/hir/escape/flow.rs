// audited: 2026-10-21
//! The escape value-flow engine that `analyze_escape` drives to a fixpoint, split by concern.
//!
//! docs/impl/escape.md
//!
//! This root re-exports what the parent reads:
//!   - `atom`    — the `Atom` flow value and the interprocedural `TailCtx`.
//!   - `summary` — the arg-return fixpoint (`compute_arg_return`).
//!   - `sources` — the source-collection walks (`tail_sources`, `return_atoms`,
//!     `record_frontier_sites`, `tail_pair_operands`).
//!   - `collect` — the edge and seed collection (`collect_flow`,
//!     `collect_container_contents`).

mod atom;
mod collect;
mod sources;
mod summary;

pub(super) use atom::{Atom, TailCtx};
pub(super) use collect::{collect_container_contents, collect_flow, Flow};
pub(super) use sources::{record_frontier_sites, return_atoms};
pub(super) use summary::compute_arg_return;
