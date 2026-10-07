// audited: 2026-10-06
//! The inference pass, and the one context that owns every map a pass reads and rewrites.
//!
//! - `fixpoint` — the ascent: passes over the whole tree until the environment
//!   stops moving.
//! - `collect`  — the pre-passes that build the program facts `Infer::new`
//!   reads (lambda params, mutated bindings, value-position uses, `type-of`
//!   aliases).
//! - `node`     — the per-node transfer function, with the binding forms in
//!   `node::binder`, the branching forms in `node::branch`, and the call rules
//!   in `node::call`.
//! - `subject`  — the `(type-of x)` scrutinee discrimination and the small
//!   `Var`/ANF/keyword helpers that feed match-arm narrowing.
//! - `facts`    — the declared floor, and the apply and restore of guard facts
//!   on the binding environment, shared by `If`, `Cond` and `Begin`.
//!
//! The operand contracts and the prove-or-reject walk live in `contract.rs`.
//!
//! docs/impl/typeinfer.md

use super::graft::{GraftRecord, Operand};
use super::*;
use std::collections::HashSet;

mod collect;
mod facts;
pub(in crate::hir::typeinfer) mod fixpoint;
mod node;
mod subject;

// The parent's `use infer::*`, and the fold's and prune.rs's
// `super::infer::{…}`, reach these items through this re-export.
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
    lambda_params: HashMap<Binding, LambdaSig>,
    /// What each graft bound to a parameter of this unit's functions, joined
    /// into the parameter at the end of every pass as a call site is.
    record: &'a GraftRecord,
    /// Bindings written by `Assign`/`SetCell` anywhere in the unit. A write
    /// gives a binding flow the per-pass recomputation cannot see. So a
    /// parameter in this set never receives call-site proofs, only guard
    /// facts. A lambda binding in it records Top for its result rather than
    /// its initializer's body type.
    mutated: HashSet<Binding>,
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
    /// The join of the values each open block's `break`s carry, which the
    /// block joins with its tail when it closes.
    break_types: HashMap<crate::hir::expr::BlockId, TyId>,
    /// This pass's call-site contributions to parameter types, rebuilt every
    /// pass. At pass end the ascent replaces each contributed parameter's type
    /// with its join here, so a call site's Top counts.
    param_joins: HashMap<Binding, TyId>,
    /// The entries the ascent gave up on. An estimate still moving when the
    /// pass budget ran out is held at Top for the rest of the run, so nothing
    /// reads a proof out of it.
    widened: HashSet<Binding>,
}

impl<'a> Infer<'a> {
    /// Collect one unit's program facts and seed the ascent's start.
    pub(super) fn new(hir: &Hir, arena: &'a BindingArena, record: &'a GraftRecord) -> Self {
        let mut lambda_params: HashMap<Binding, LambdaSig> = HashMap::new();
        collect_lambda_info(hir, arena, &mut lambda_params);
        let mutated = collect_mutated_bindings(hir);
        let mut value_used = HashSet::new();
        collect_value_position_uses(hir, &mut value_used);
        let mut typeof_aliases: HashMap<Binding, Binding> = HashMap::new();
        collect_typeof_aliases(hir, arena, &mut typeof_aliases);

        // The Kleene start: a parameter complete call-site enumeration can
        // prove (a callee-only binding, an unmutated parameter) begins at
        // Bottom. A value-used binding's parameters stay absent, which reads as
        // Top, because their callers are not all visible.
        //
        // The start is not a proof, and a never-called function's parameters
        // never leave it. `Infer::settle` raises every Bottom left in the node
        // map to Top before the map leaves the pass.
        let mut binding_types: HashMap<Binding, TyId> = HashMap::new();
        for (b, sig) in &lambda_params {
            if value_used.contains(b) {
                continue;
            }
            for p in &sig.all {
                if !mutated.contains(p) {
                    binding_types.insert(*p, TypeInterner::BOTTOM);
                }
            }
        }

        Infer {
            interner: TypeInterner::new(),
            arena,
            lambda_params,
            record,
            mutated,
            value_used,
            typeof_aliases,
            binding_types,
            hir_types: HashMap::new(),
            lambda_body_type: HashMap::new(),
            binding_min_length: HashMap::new(),
            break_types: HashMap::new(),
            param_joins: HashMap::new(),
            widened: HashSet::new(),
        }
    }
}
