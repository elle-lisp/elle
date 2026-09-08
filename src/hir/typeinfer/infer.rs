// audited: 2026-09-08
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! The inference pass and the environment it carries: one context owns every
//! map a pass reads and rewrites, so each per-node rule names only what it uses.
//!
//! - `fixpoint` — the ascent: passes over the whole tree until the environment
//!   stops moving (docs/impl/typeinfer.md).
//! - `collect`  — the pre-passes that build the program facts `Infer::new`
//!   reads (lambda params, mutated bindings, value-position uses, `type-of`
//!   aliases).
//! - `node`     — the per-node transfer function that is the heart of the pass,
//!   with the binding forms in `node::binder`, the branching forms in
//!   `node::branch`, and the call rules in `node::call`.
//! - `subject`  — the `(type-of x)` scrutinee discrimination and the small
//!   `Var`/ANF/keyword helpers that feed match-arm narrowing.
//! - `facts`    — apply/restore of guard-derived narrowing facts on the binding
//!   environment (save/restore discipline shared by `If`/`Cond`/`Begin`).
//!
//! The per-op operand contracts and the prove-or-reject walk live in
//! `contract.rs` (`check_intrinsic_operand_proofs`) — the generalization of the
//! monomorphic-container obligation to every %-intrinsic in call position.

use super::*;
use std::collections::HashSet;

mod collect;
mod facts;
pub(in crate::hir::typeinfer) mod fixpoint;
mod node;
mod subject;

// Re-export at `pub(super)` so every path that resolved as
// `crate::hir::typeinfer::infer::<Item>` before the split still resolves: the
// parent's `use infer::*` and prune.rs's `super::infer::{…}` both depend on it.
pub(super) use collect::*;
pub(super) use facts::*;
pub(super) use subject::*;

/// The inference pass: the program facts one compilation unit yields, and the
/// type environment the ascent rewrites over them.
pub(super) struct Infer<'a> {
    /// The lattice. Owned, so a per-node rule joins without being handed one.
    interner: TypeInterner,
    arena: &'a BindingArena,

    // ── the program's facts: collected once, never rewritten ──
    /// Which bindings are lambdas, and what their parameters are.
    lambda_params: HashMap<Binding, Vec<Binding>>,
    /// Bindings written by `Assign`/`SetCell`. A parameter in this set has flow
    /// the per-pass recomputation cannot see, so it never receives call-site
    /// proofs (guards only).
    mutated_params: HashSet<Binding>,
    /// Bindings with at least one value-position use
    /// (`collect_value_position_uses`): their callers are not all visible, so
    /// call-site joins must not prove their parameters.
    value_used: HashSet<Binding>,
    /// Immutable let-bound aliases of `(type-of a)`, mapped to their subject
    /// `a` (`collect_typeof_aliases`), so a match on such an alias narrows `a`.
    pub(super) typeof_aliases: HashMap<Binding, Binding>,

    // ── the environment: what a pass reads, and what it leaves behind ──
    binding_types: HashMap<Binding, TyId>,
    pub(super) hir_types: HashMap<HirId, TyId>,
    /// Each lambda binding's body type, as the last pass computed it. Every
    /// call reads its result here, a self-recursive call included.
    lambda_body_type: HashMap<Binding, TyId>,
    pub(super) binding_min_length: HashMap<Binding, usize>,
    /// This pass's call-site contributions to parameter types. RECOMPUTED per
    /// pass (the ascent replaces each contributed parameter's binding type
    /// wholesale at pass end): a join that only ever accumulates can never come
    /// back down, so unknown-typed call sites would have to be skipped — and a
    /// skipped unknown is exactly the unsound "typed callers alone prove the
    /// parameter" hole. Here Top contributes honestly.
    param_joins: HashMap<Binding, TyId>,
    /// The entries the ascent gave up on: an estimate still moving when the
    /// pass budget ran out is held at Top for the rest of the run, so nothing
    /// reads a proof out of it (`fixpoint.rs`).
    widened: HashSet<Binding>,
}

impl<'a> Infer<'a> {
    /// Collect one unit's program facts and seed the ascent's start.
    pub(super) fn new(hir: &Hir, arena: &'a BindingArena) -> Self {
        let mut lambda_params: HashMap<Binding, Vec<Binding>> = HashMap::new();
        collect_lambda_info(hir, arena, &mut lambda_params);
        let mutated_params = collect_mutated_bindings(hir);
        let mut value_used = HashSet::new();
        collect_value_position_uses(hir, &mut value_used);
        let mut typeof_aliases: HashMap<Binding, Binding> = HashMap::new();
        collect_typeof_aliases(hir, arena, &mut typeof_aliases);

        // Kleene start: every parameter that CAN be proven by complete
        // call-site enumeration (callee-only binding, unmutated param) begins
        // at BOTTOM, so an identity-passed argument in a self/mutual recursion
        // contributes nothing on the way up instead of reading the Top default
        // and pinning itself there. Parameters of value-used bindings stay
        // ABSENT (read as Top): their callers are not enumerable, so optimism
        // there would let the checker pass on ⊥. A never-called callee-only
        // function's params stay ⊥ — its %-sites can never execute, so nothing
        // unsound compiles.
        let mut binding_types: HashMap<Binding, TyId> = HashMap::new();
        for (b, params) in &lambda_params {
            if value_used.contains(b) {
                continue;
            }
            for p in params {
                if !mutated_params.contains(p) {
                    binding_types.insert(*p, TypeInterner::BOTTOM);
                }
            }
        }

        Infer {
            interner: TypeInterner::new(),
            arena,
            lambda_params,
            mutated_params,
            value_used,
            typeof_aliases,
            binding_types,
            hir_types: HashMap::new(),
            lambda_body_type: HashMap::new(),
            binding_min_length: HashMap::new(),
            param_joins: HashMap::new(),
            widened: HashSet::new(),
        }
    }
}
