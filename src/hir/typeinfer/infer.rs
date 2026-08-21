//! Forward type-inference pass, split by concern:
//!
//! - `collect`  — the pre-passes that build the info maps `infer_node` reads
//!   (lambda params, mutated bindings, value-position uses, `type-of` aliases).
//! - `node`     — the fixpoint driver `infer_types` and the per-node `infer_node`
//!   transfer function that is the heart of the pass.
//! - `subject`  — the `(type-of x)` scrutinee discrimination and the small
//!   `Var`/ANF/keyword helpers that feed match-arm narrowing.
//! - `facts`    — apply/restore of guard-derived narrowing facts on the binding
//!   environment (save/restore discipline shared by `If`/`Cond`/`Begin`).
//!
//! The per-op operand contracts and the prove-or-reject walk live in
//! `contract.rs` (`check_intrinsic_operand_proofs`) — the generalization of the
//! monomorphic-container obligation to every %-intrinsic in call position.

mod collect;
mod facts;
mod node;
mod subject;

// Re-export at `pub(super)` so every path that resolved as
// `crate::hir::typeinfer::infer::<Item>` before the split still resolves: the
// parent's `use infer::*` and prune.rs's `super::infer::{…}` both depend on it.
pub(super) use collect::*;
pub(super) use facts::*;
pub(super) use node::*;
pub(super) use subject::*;
