//! What counts as a branch **arm**.
//!
//! An arm is a program region at most one of which runs per execution — not a
//! syntactic arm body. For an `If` and a `Match` the two coincide. For the
//! short-circuiting forms they do not: a `cond`'s clause TESTS are conditional
//! positions exactly as its bodies are (test *k* runs only where tests 0..*k*-1
//! all failed), and an `and`/`or` tail is a conditional position with no sibling
//! body at all. Reading those forms syntactically leaves a release whose last use
//! is a later test outside every arm — which is exactly where the polymorphic
//! entry point puts it (docs/impl/region/mechanism.md § "An arm is a conditional
//! position, not a syntactic arm body").
//!
//! The arms are read off the nested-`If` each form is equivalent to:
//!
//! ```text
//! (cond t0 b0 t1 b1 … e)  ≡  (if t0 b0 (if t1 b1 … e))
//! (and e0 e1 … en)        ≡  (if e0 (and e1 … en) false)
//! (or  e0 e1 … en)        ≡  (if e0 true (or e1 … en))
//! ```
//!
//! so one `Cond` yields one [`ArmSet`] per clause — the clause BODY, and the rest
//! of the chain from the next test through the `else` — while `And`/`Or` yield a
//! single one-armed set, their tail. Every such span is contiguous in post-order,
//! the walk visiting a form's parts in source order, so each is one interval and
//! neither consumer learns a new shape. All the sets of one form carry the form's
//! own node and interval, so they share one anchor and one live-in premise.
//!
//! Two consumers, deliberately sharing one reading: the branch-arm release window
//! (`super::analyze::decref`) and per-arm branch compensation
//! ([`super::compensate`]).

use std::collections::HashMap;

use crate::hir::expr::{Hir, HirId, HirKind};

/// One arm: the node whose head hosts a compensating release, and the post-order
/// interval of the span it covers. For a syntactic arm the interval is that
/// node's own subtree; for the tail of a short-circuiting chain it spans from
/// that node through the form's last part, and `id` is the first node the tail
/// evaluates — the point control reaches when the previous test failed.
pub(super) struct Arm {
    pub(super) id: HirId,
    pub(super) lo: u32,
    pub(super) hi: u32,
}

/// One branch: the form's own node and interval, plus the arms of one level of
/// its nested-`If` equivalent. A form with N conditional levels contributes N of
/// these, all naming the same node.
pub(super) struct ArmSet {
    pub(super) id: HirId,
    pub(super) node_lo: u32,
    pub(super) node_hi: u32,
    pub(super) arms: Vec<Arm>,
}

/// The branches `hir` itself contributes — none for a node that is not a branch.
/// Callers walk the tree themselves, since each collects different scopes
/// alongside.
pub(super) fn branch_arms(
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    low: &HashMap<HirId, u32>,
) -> Vec<ArmSet> {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let lo = |id: HirId| low.get(&id).copied().unwrap_or(0);
    let arm = |h: &Hir| Arm {
        id: h.id,
        lo: lo(h.id),
        hi: ord(h.id),
    };
    let whole = |arms: Vec<Arm>| ArmSet {
        id: hir.id,
        node_lo: lo(hir.id),
        node_hi: ord(hir.id),
        arms,
    };
    match &hir.kind {
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => vec![whole(vec![arm(then_branch), arm(else_branch)])],
        HirKind::Match { arms, .. } => {
            vec![whole(arms.iter().map(|(_p, _g, body)| arm(body)).collect())]
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            // The rest of the chain past clause `k` ends at the form's last part
            // — the `else` branch, or the last clause's body where the form has
            // none. A `cond` that matches no clause evaluates to `nil` having run
            // no body, so at the last level the sibling arm is the `else` or
            // nothing at all: there is no node to host a release, and the pass
            // simply does not fire on that path (the leak-preserving direction a
            // `Match` with no matching arm already takes).
            let Some(last) = else_branch
                .as_deref()
                .or_else(|| clauses.last().map(|(_t, b)| b))
            else {
                return Vec::new();
            };
            let tail_hi = ord(last.id);
            clauses
                .iter()
                .enumerate()
                .map(|(k, (_test, body))| {
                    let rest = clauses
                        .get(k + 1)
                        .map(|(next_test, _)| next_test)
                        .or(else_branch.as_deref());
                    let mut arms = vec![arm(body)];
                    if let Some(head) = rest {
                        arms.push(Arm {
                            id: head.id,
                            lo: lo(head.id),
                            hi: tail_hi,
                        });
                    }
                    whole(arms)
                })
                .collect()
        }
        HirKind::And(exprs) | HirKind::Or(exprs) => {
            // The head always runs; everything after it is the one conditional
            // position, and the short-circuit path evaluates no node — so this is
            // a one-armed branch. That is enough for the window, whose anchor is
            // the point every path reaches, and inert for compensation, which has
            // no sibling arm to hang a release on.
            let (Some(tail), Some(last)) = (exprs.get(1), exprs.last()) else {
                return Vec::new();
            };
            vec![whole(vec![Arm {
                id: tail.id,
                lo: lo(tail.id),
                hi: ord(last.id),
            }])]
        }
        _ => Vec::new(),
    }
}
