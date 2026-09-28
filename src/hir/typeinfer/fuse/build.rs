// audited: 2026-09-28
//! The node factory a fused loop is built with, and the one-step counter advance
//! every loop it builds shares.
//!
//! docs/impl/dissolution.md
//! docs/impl/dissolution/inline.md
//! docs/intrinsics.md

use super::*;

mod element;
mod scaffold;

use element::*;
pub(super) use scaffold::build_loop;

/// Mint a fresh reassigned local: the walk's induction variable, a scalar
/// accumulator, a survivor count, or a stage's `true`-seeded flag (a `take-while`'s
/// run sentinel, a `drop-while`'s dropping flag). Each is `define`d before the loop
/// and `assign`ed inside it, so it must read as mutated. A free function because
/// `take_chain` mints a stage's flag before any [`Build`] exists, and `build_loop`
/// mints the induction variable before it constructs one.
pub(super) fn fresh_mutable(arena: &mut BindingArena) -> Binding {
    let b = arena.gensym();
    arena.get_mut(b).is_mutated = true;
    b
}

/// Node factory for the synthesized loop. Bundles the span and signal every
/// synthesized node carries and the arena for minting locals, so the fixed
/// `(get`/`push`/`freeze)` scaffold and the per-element transform/guard pipeline
/// build nodes uniformly. The synthesized helper calls and `if`/`let` scaffolding
/// carry `sig` — the original call's signal, a sound upper bound over every op in
/// the stdlib op's body — while spliced lambda bodies keep their own signals (they
/// are moved in whole). Bottom-up re-propagation (`hir/narrow.rs`) then rebuilds
/// the fused form's signal from these leaves without under-reporting.
pub(super) struct Build<'a> {
    pub(super) arena: &'a mut BindingArena,
    pub(super) ops: &'a Ops,
    pub(super) span: crate::syntax::Span,
    pub(super) sig: Signal,
    /// The walk's induction variable — the current element's index into the BASE,
    /// which is also its position in a [`Stage::Enumerate`]'s input (every stage
    /// inner to one preserves the walk's length).
    pub(super) index: Binding,
}

impl Build<'_> {
    pub(super) fn node(&self, kind: HirKind) -> Hir {
        Hir::new(kind, self.span, self.sig)
    }
    pub(super) fn var(&self, b: Binding) -> Hir {
        Hir::new(HirKind::Var(b), self.span, Signal::silent())
    }
    pub(super) fn int(&self, n: i64) -> Hir {
        Hir::new(HirKind::Int(n), self.span, Signal::silent())
    }
    pub(super) fn nil(&self) -> Hir {
        Hir::new(HirKind::Nil, self.span, Signal::silent())
    }
    pub(super) fn bool(&self, b: bool) -> Hir {
        Hir::new(HirKind::Bool(b), self.span, Signal::silent())
    }
    pub(super) fn empty_list(&self) -> Hir {
        Hir::new(HirKind::EmptyList, self.span, Signal::silent())
    }
    pub(super) fn call(&self, f: Binding, args: Vec<Hir>) -> Hir {
        self.node(HirKind::Call {
            func: Box::new(self.var(f)),
            args: args
                .into_iter()
                .map(|expr| CallArg {
                    expr,
                    spliced: false,
                })
                .collect(),
            is_tail: false,
        })
    }
    /// `(assign counter (%add counter 1))` — the one-step advance every counter the
    /// scaffold owns takes: the base walk's induction variable, a `mapcat`'s inner
    /// one, a `find-index`'s survivor count, a `count`'s tally.
    ///
    /// The raw `%add` is the opcode the stdlib walks this loop stands in for advance
    /// by (`src/prelude.lisp`'s `each` macro). Reaching the stdlib `+` instead would put
    /// a variadic call in the loop: `+` collects a rest-list and folds it with a
    /// `letrec` walker, so every element would mint that list, the walker's closure
    /// and its cell — re-creating per element exactly the closure this pass exists to
    /// dissolve. The intrinsic's operand contract discharges from the counter's own
    /// type: the scaffold seeds each at the literal `0` and advances it by this site
    /// alone, so nothing but a number ever reaches it.
    pub(super) fn advance(&self, counter: Binding) -> Hir {
        let step = self.node(HirKind::Intrinsic {
            op: crate::hir::expr::IntrinsicOp::Add,
            args: vec![self.var(counter), self.int(1)],
        });
        self.node(HirKind::Assign {
            target: counter,
            value: Box::new(step),
        })
    }
    pub(super) fn let_(&self, binding: Binding, value: Hir, body: Hir) -> Hir {
        self.node(HirKind::Let {
            bindings: vec![(binding, value)],
            body: Box::new(body),
        })
    }
    /// A fresh immutable local (accumulator, length, bound element, …).
    pub(super) fn local(&mut self) -> Binding {
        let b = self.arena.gensym();
        self.arena.get_mut(b).is_immutable = true;
        b
    }
    /// A fresh reassigned local — a scalar accumulator, an early-exit sentinel, a
    /// survivor count (`fresh_mutable`).
    pub(super) fn mutable_local(&mut self) -> Binding {
        fresh_mutable(self.arena)
    }
    /// Retype a consumed lambda parameter to a plain immutable local: the lambda is
    /// gone, so the lowerer must give the parameter a local slot, not an argument
    /// slot.
    pub(super) fn localize_param(&mut self, param: Binding) {
        let pi = self.arena.get_mut(param);
        pi.scope = BindingScope::Local;
        pi.is_immutable = true;
    }
}
