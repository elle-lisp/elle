// audited: 2026-09-09
//! What one `letrec` BODY looks like at its tail, as the closure-cycle merge's two
//! tail gates read it.
//!
//! docs/impl/region/letrec.md
//!
//! Two gates ask two questions of the same body. The RELEASE-CHANNEL gate asks what
//! each tail call is — its callee, and whether a cycle member rides in as an argument
//! — because a frame-replacing `TailCall` strands the merged arena's binding-scope
//! drop. The RETURN-FUNDED gate asks whether every tail exit LEAVES the frame, which
//! is where the mint that funds the caller lands. A body that leaves neither way
//! yields a bare value, and the bindings it yields say whether the letrec hands a
//! member out.

// `super::super::super` is `region::infer`, whose private imports every merge
// submodule shares (`cycle` reaches them one level up).
use super::super::super::*;
use rustc_hash::FxHashMap;

/// One tail call in a `letrec` body: what the release-channel gate needs of it — the
/// callee, and every binding an argument carries in by-move.
pub(super) struct TailCallSite {
    /// The tail-call `Call` node's HirId — the key the lowerer sets
    /// `deferred_release_slot` at for a non-member callee.
    pub(super) hir_id: HirId,
    /// The callee, unwrapped through `functionalize`'s `DerefCell`: `Some(b)` for a
    /// binding reference (a member, a native, a redefined operator, a foreign fn),
    /// `None` for a callee the gate cannot resolve (which refuses — no site to key
    /// the deferred release at).
    pub(super) callee: Option<crate::hir::Binding>,
    /// Every binding referenced in the tail call's ARGUMENT subtrees (Var reads,
    /// including a cell read's inner `Var`; nested lambdas are not descended). A
    /// member passed by-move is exactly a binding here whose source region is in the
    /// SCC. After ANF a nested aggregate/call argument is a fresh temp whose source
    /// region is the aggregate/call-result region — never the member's — so the
    /// RC-safe stored-then-passed case is not caught here (correctly admitted).
    pub(super) arg_bindings: Vec<crate::hir::Binding>,
}

/// What one `Letrec`'s BODY looks like at its tail, as the merge's two tail gates
/// read it.
pub(super) struct LetrecTail {
    /// One [`TailCallSite`] per tail call in the body — the release-channel gate's
    /// input. A body tail call replaces the frame, stranding the binding-scope drop,
    /// so each must supply a release channel (a member callee's stranded-cycle
    /// deferral, or a non-member callee's explicit arena adopt) and must not pass a
    /// cycle member in by-move.
    pub(super) sites: Vec<TailCallSite>,
    /// Does EVERY tail exit of the body leave the frame itself — a `Return`, or a
    /// tail `Call`? The **return-funded** admission needs this reading rather than
    /// `sites`: it decides whether the value the frame hands its caller is minted
    /// INSIDE the body, ahead of the binding-scope drop the lowerer emits at the
    /// `Letrec` node (docs/impl/region/letrec.md § The frontier gate).
    pub(super) exits_frame: bool,
    /// The bindings the body's tail region-transparently evaluates TO — the value the
    /// `Letrec` node itself yields. Read only where `exits_frame` is false, i.e. where
    /// that value goes to an ENCLOSING consumer: a cycle member among them leaves the
    /// binding scope on an uncounted read, so the arena's release follows it instead of
    /// staying at the scope (docs/impl/region/letrec.md § "Drop site — following a
    /// handed-out member").
    pub(super) value_bindings: Vec<crate::hir::Binding>,
}

/// For every `Letrec` node, how its BODY exits at the tail ([`LetrecTail`]).
pub(super) fn collect_letrec_tail_callees(hir: &Hir, out: &mut FxHashMap<HirId, LetrecTail>) {
    if let HirKind::Letrec { body, .. } = &hir.kind {
        let mut sites = Vec::new();
        body_tail_callees(body, &mut sites);
        let mut value_bindings = Vec::new();
        value_flow_bindings(body, &mut value_bindings);
        out.insert(
            hir.id,
            LetrecTail {
                sites,
                exits_frame: body_tail_exits_frame(body),
                value_bindings,
            },
        );
    }
    hir.for_each_child(|c| collect_letrec_tail_callees(c, out));
}

/// Does every tail EXIT of a letrec body leave the frame — is the value the frame
/// hands its caller produced (and minted) INSIDE this body?
///
/// The return-funded admission reads this to know that the merge's binding-scope
/// release runs after that mint (docs/impl/region/letrec.md § The frontier gate).
/// Descends only the tail position of the pure control forms, and deliberately answers
/// `false` for everything it does not recognise: a body it cannot read may hand its
/// value to an enclosing consumer, whose mint is outside the letrec node and therefore
/// after the release. A bare value tail out of tail position
/// (`(let [c (letrec [ev …] ev)] … c)`), a loop, a short-circuit `And`/`Or`, and a
/// `Cond` with no else arm all read that way.
fn body_tail_exits_frame(hir: &Hir) -> bool {
    match &hir.kind {
        // The two nodes that hand a value to the CALLER from inside this body: the
        // `Return` mints for it here, and a tail `Call` mints either at the callee's
        // own `Return` (a closure, which replaces the frame) or at the post-`TailCall`
        // fall-through retain emitted right here (a native, which does not).
        HirKind::Return { .. } | HirKind::Call { is_tail: true, .. } => true,
        HirKind::Let { body, .. }
        | HirKind::Letrec { body, .. }
        | HirKind::Parameterize { body, .. } => body_tail_exits_frame(body),
        HirKind::Begin(exprs) | HirKind::Block { body: exprs, .. } => {
            exprs.last().is_some_and(body_tail_exits_frame)
        }
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => body_tail_exits_frame(then_branch) && body_tail_exits_frame(else_branch),
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            // No else arm means an implicit nil fall-through — an exit that does not
            // leave the frame, so the whole `Cond` does not.
            else_branch
                .as_ref()
                .is_some_and(|e| body_tail_exits_frame(e))
                && clauses.iter().all(|(_, b)| body_tail_exits_frame(b))
        }
        // `Match` is absent deliberately, where branch compensation admits it exactly as
        // an `If`: this reading needs the arms to be EXHAUSTIVE (a fall-through with no
        // arm taken is an exit that does not leave the frame), which an `If` and a
        // `Cond` with an else arm have and a pattern match does not.
        _ => false,
    }
}

/// The [`TailCallSite`]s within one letrec body. Never descends into a `Lambda`
/// — a nested closure's tail calls run in that closure's own activation, not the
/// letrec's, so they neither strand nor may adopt the merged arena (mirrors the
/// lowerer's `collect_body_tail_callees`, `lir/lower/binding.rs`). The callee is
/// unwrapped through the `DerefCell` `functionalize` adds around a needs-capture
/// binding read; each argument subtree contributes its referenced bindings.
fn body_tail_callees(hir: &Hir, out: &mut Vec<TailCallSite>) {
    if matches!(hir.kind, HirKind::Lambda { .. }) {
        return;
    }
    if let HirKind::Call {
        func,
        args,
        is_tail: true,
        ..
    } = &hir.kind
    {
        let callee_node = match &func.kind {
            HirKind::DerefCell { cell } => cell,
            _ => func,
        };
        let callee = match &callee_node.kind {
            HirKind::Var(b) => Some(*b),
            _ => None,
        };
        let mut arg_bindings = Vec::new();
        for a in args {
            value_flow_bindings(&a.expr, &mut arg_bindings);
        }
        out.push(TailCallSite {
            hir_id: hir.id,
            callee,
            arg_bindings,
        });
    }
    hir.for_each_child(|c| body_tail_callees(c, out));
}

/// The bindings an expression region-transparently evaluates **to** — the values that
/// flow out of it with no incref of their own. This mirrors escape's `tail_sources`
/// descent (`hir/escape/flow.rs`): pass through the pure control / select / deref /
/// bind wrappers, but STOP at a `Call`, an `Intrinsic`, and a `Lambda`. A member
/// reached only past a stopped node did not flow uncounted:
///
///  - a nested `Call` — `(g (ev k))` — has `ev` as its callee, so `ev`'s RESULT (a
///    value) flows, not `ev` itself; a member passed as a nested-call argument is
///    incref-balanced (a non-tail call owns its params), not moved;
///  - an `Intrinsic` — `(g (%pair od 1))` — stores the member into a fresh aggregate,
///    an RC-counted reference the aggregate's cascade releases;
///  - a `Lambda` — a closure argument's captures are RC-counted.
///
/// Two readers, asking the same question of different expressions. Over a **tail-call
/// argument** it names what flows BY-MOVE into the call: only a bare member value in a
/// direct argument (`(g od)`, or through an `If`/`Begin`/`DerefCell` that selects one)
/// is moved with no incref, so a callee that replaces the frame owns it as a parameter
/// and its release would double-free the arena — which the tail gate reads this to
/// refuse. Over a **letrec body** it names the value
/// the `Letrec` node yields, so a member there is one the cycle hands to an enclosing
/// consumer — read only where the body does not leave the frame, hence never through
/// the `Return` arm below.
fn value_flow_bindings(hir: &Hir, out: &mut Vec<crate::hir::Binding>) {
    match &hir.kind {
        HirKind::Var(b) => out.push(*b),
        HirKind::DerefCell { cell } => value_flow_bindings(cell, out),
        HirKind::Let { body, .. }
        | HirKind::Letrec { body, .. }
        | HirKind::Loop { body, .. }
        | HirKind::Parameterize { body, .. } => value_flow_bindings(body, out),
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            value_flow_bindings(then_branch, out);
            value_flow_bindings(else_branch, out);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (_, b) in clauses {
                value_flow_bindings(b, out);
            }
            if let Some(eb) = else_branch {
                value_flow_bindings(eb, out);
            }
        }
        HirKind::Begin(exprs) | HirKind::Block { body: exprs, .. } => {
            if let Some(last) = exprs.last() {
                value_flow_bindings(last, out);
            }
        }
        HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                value_flow_bindings(e, out);
            }
        }
        HirKind::Match { arms, .. } => {
            for (_, _, body) in arms {
                value_flow_bindings(body, out);
            }
        }
        HirKind::Return { value }
        | HirKind::MakeCell { value }
        | HirKind::Assign { value, .. }
        | HirKind::Define { value, .. }
        | HirKind::Destructure { value, .. }
        | HirKind::SetCell { value, .. } => value_flow_bindings(value, out),
        // A Call / Intrinsic / Lambda / immediate: a fresh, incref-balanced, or
        // RC-counted result — no member flows by-move. Stop.
        _ => {}
    }
}
