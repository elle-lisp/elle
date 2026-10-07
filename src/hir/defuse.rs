// audited: 2026-10-06
//! Def-use chains over HIR, and the origin of the value each node produces.
//!
//! One forward walk over the tree builds three tables:
//! - `def_site`: the node that defines each binding
//! - `uses`: the nodes that read each binding
//! - `value_origin`: what each node's value is made from
//!
//! docs/impl/hir.md

use super::binding::Binding;
use super::expr::{Hir, HirId, HirKind};

use std::collections::HashMap;

/// What a result-position expression produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueOrigin {
    /// An immediate value, or no value at this position (`break`, `recur`)
    Immediate,
    /// Reference to another binding
    Binding(Binding),
    /// Opaque result of a call or an `eval`
    CallResult,
    /// A lambda, string literal, compound or heap quote, `MakeCell`, or
    /// allocating intrinsic
    Allocation,
    /// deref-cell (value unknown statically)
    CellDeref,
    /// Control-flow merge with different origins
    Mixed,
}

impl ValueOrigin {
    fn merge(a: &ValueOrigin, b: &ValueOrigin) -> ValueOrigin {
        if a == b {
            a.clone()
        } else {
            ValueOrigin::Mixed
        }
    }

    /// Fold an iterator of origins into a single merged origin.
    fn fold(origins: impl Iterator<Item = ValueOrigin>) -> ValueOrigin {
        let mut result: Option<ValueOrigin> = None;
        for o in origins {
            result = Some(match result {
                None => o,
                Some(prev) => Self::merge(&prev, &o),
            });
        }
        result.unwrap_or(ValueOrigin::Immediate)
    }
}

/// Def-use chain builder. Accumulates results during the walk.
pub(crate) struct DefUseBuilder {
    pub def_site: HashMap<Binding, HirId>,
    pub uses: HashMap<Binding, Vec<HirId>>,
    pub value_origin: HashMap<HirId, ValueOrigin>,
}

impl DefUseBuilder {
    pub fn new() -> Self {
        DefUseBuilder {
            def_site: HashMap::new(),
            uses: HashMap::new(),
            value_origin: HashMap::new(),
        }
    }

    fn record_def(&mut self, binding: Binding, hir_id: HirId) {
        self.def_site.insert(binding, hir_id);
    }

    fn record_use(&mut self, binding: Binding, hir_id: HirId) {
        self.uses.entry(binding).or_default().push(hir_id);
    }

    fn record_origin(&mut self, hir_id: HirId, origin: ValueOrigin) {
        self.value_origin.insert(hir_id, origin);
    }

    fn origin_of(&self, id: HirId) -> ValueOrigin {
        self.value_origin
            .get(&id)
            .cloned()
            .unwrap_or(ValueOrigin::Mixed)
    }

    /// Walk a HIR node, building def-use chains and value origins.
    pub fn walk(&mut self, hir: &Hir) {
        let origin = self.compute(hir);
        self.record_origin(hir.id, origin);
    }

    /// Compute value origin for a node, recording defs/uses along the way.
    fn compute(&mut self, hir: &Hir) -> ValueOrigin {
        match &hir.kind {
            // Literals
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Keyword(_) => ValueOrigin::Immediate,

            // Allocations
            HirKind::String(_) | HirKind::QuoteConst(_) => ValueOrigin::Allocation,
            // `Quote` holds an immediate (`'5`, `'foo`) or, on the
            // macro-hygiene path alone, a pre-baked heap syntax object.
            HirKind::Quote(v) => {
                if v.is_heap() {
                    ValueOrigin::Allocation
                } else {
                    ValueOrigin::Immediate
                }
            }
            HirKind::Lambda(lambda) => {
                for p in &lambda.params {
                    self.record_def(*p, hir.id);
                }
                if let Some(rp) = lambda.rest_param {
                    self.record_def(rp, hir.id);
                }
                // Each capture is a use of the outer binding at the lambda's
                // HirId. A `Recursive` capture counts too: the self-reference
                // borrows the executing closure, so the closure's region must
                // live through the whole recursion.
                for cap in &lambda.captures {
                    self.record_use(cap.binding, hir.id);
                }
                self.walk(&lambda.body);
                ValueOrigin::Allocation
            }

            HirKind::MakeCell { value } => {
                self.walk(value);
                ValueOrigin::Allocation
            }

            // Variable reference
            HirKind::Var(b) => {
                self.record_use(*b, hir.id);
                ValueOrigin::Binding(*b)
            }

            // Cell deref
            HirKind::DerefCell { cell } => {
                self.walk(cell);
                ValueOrigin::CellDeref
            }

            // SetCell: use of cell + value; returns the written value
            HirKind::SetCell { cell, value } => {
                self.walk(cell);
                self.walk(value);
                // SetCell returns the value it writes.
                self.origin_of(value.id)
            }

            // Call
            HirKind::Call { func, args, .. } => {
                self.walk(func);
                for a in args {
                    self.walk(&a.expr);
                }
                ValueOrigin::CallResult
            }

            // Binding forms
            HirKind::Let { bindings, body } => {
                for (b, init) in bindings {
                    self.walk(init);
                    self.record_def(*b, hir.id);
                }
                self.walk(body);
                self.origin_of(body.id)
            }

            HirKind::Letrec { bindings, body } => {
                for (b, init) in bindings {
                    self.record_def(*b, hir.id);
                    self.walk(init);
                }
                self.walk(body);
                self.origin_of(body.id)
            }

            HirKind::Define { binding, value } => {
                self.walk(value);
                self.record_def(*binding, hir.id);
                self.origin_of(value.id)
            }

            // Control flow
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk(cond);
                self.walk(then_branch);
                self.walk(else_branch);
                ValueOrigin::merge(
                    &self.origin_of(then_branch.id),
                    &self.origin_of(else_branch.id),
                )
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (c, b) in clauses {
                    self.walk(c);
                    self.walk(b);
                }
                if let Some(eb) = else_branch {
                    self.walk(eb);
                }
                let clause_origins = clauses.iter().map(|(_, b)| self.origin_of(b.id));
                let all_origins =
                    clause_origins.chain(else_branch.iter().map(|eb| self.origin_of(eb.id)));
                ValueOrigin::fold(all_origins)
            }

            HirKind::Match { value, arms } => {
                self.walk(value);
                for (pat, guard, body) in arms {
                    for b in pat.bindings().bindings {
                        self.record_def(b, hir.id);
                    }
                    if let Some(g) = guard {
                        self.walk(g);
                    }
                    self.walk(body);
                }
                ValueOrigin::fold(arms.iter().map(|(_, _, body)| self.origin_of(body.id)))
            }

            HirKind::Begin(exprs) => {
                for e in exprs {
                    self.walk(e);
                }
                exprs
                    .last()
                    .map(|last| self.origin_of(last.id))
                    .unwrap_or(ValueOrigin::Immediate)
            }

            HirKind::Block { body, .. } => {
                for e in body {
                    self.walk(e);
                }
                body.last()
                    .map(|last| self.origin_of(last.id))
                    .unwrap_or(ValueOrigin::Immediate)
            }

            HirKind::Break { value, .. } => {
                self.walk(value);
                // Break doesn't produce a value at this position
                ValueOrigin::Immediate
            }

            // Loop/Recur
            HirKind::Loop { bindings, body } => {
                for (b, init) in bindings {
                    self.walk(init);
                    self.record_def(*b, hir.id);
                }
                self.walk(body);
                // The loop's value is the body's value on the path that does
                // not recur.
                self.origin_of(body.id)
            }

            HirKind::Recur { args } => {
                for a in args {
                    self.walk(a);
                }
                // Recur doesn't produce a value (jumps back)
                ValueOrigin::Immediate
            }

            // Assign defines its target. Dead-binding elimination walks the
            // tree before functionalize, where assignments are common.
            HirKind::Assign { target, value } => {
                self.walk(value);
                self.record_def(*target, hir.id);
                self.origin_of(value.id)
            }

            // Boolean short-circuit
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                for e in exprs {
                    self.walk(e);
                }
                ValueOrigin::fold(exprs.iter().map(|e| self.origin_of(e.id)))
            }

            // Emit
            HirKind::Emit { value, .. } => {
                self.walk(value);
                ValueOrigin::Immediate
            }

            // Return is region-transparent: its value origin is the
            // wrapped value's origin.
            HirKind::Return { value } => {
                self.walk(value);
                self.origin_of(value.id)
            }

            // Destructure
            HirKind::Destructure { pattern, value, .. } => {
                self.walk(value);
                for b in pattern.bindings().bindings {
                    self.record_def(b, hir.id);
                }
                ValueOrigin::Immediate
            }

            // Eval
            HirKind::Eval { expr, env } => {
                self.walk(expr);
                self.walk(env);
                ValueOrigin::CallResult
            }

            // Parameterize
            HirKind::Parameterize { bindings, body } => {
                for (k, v) in bindings {
                    self.walk(k);
                    self.walk(v);
                }
                self.walk(body);
                self.origin_of(body.id)
            }

            // While: functionalize removes it, but the walk may run earlier.
            HirKind::While { cond, body } => {
                self.walk(cond);
                self.walk(body);
                ValueOrigin::Immediate
            }

            // Intrinsic: walk args; non-allocating → Immediate, allocating → Allocation
            HirKind::Intrinsic { op, args } => {
                for a in args {
                    self.walk(a);
                }
                if op.allocates() {
                    ValueOrigin::Allocation
                } else {
                    ValueOrigin::Immediate
                }
            }

            HirKind::Error => ValueOrigin::Immediate,
        }
    }
}

#[cfg(test)]
mod tests;
