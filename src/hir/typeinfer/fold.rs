// audited: 2026-10-28
//! The fold: after inference, delete the branches and dispatch arms the inferred types decide.
//!
//! A decided `if` becomes its taken branch. A `(match (type-of x) …)` whose
//! live arm is decided becomes that arm's body. A `cond` loses the clauses a
//! decided test rules out, and an `and` or `or` stops at the operand that
//! decides it. A sequence drops the literal a decided guard left in it. A
//! deletion that would take a binding something outside it still reads is
//! declined. No statement after a raise goes: a resume reaches it.
//!
//! docs/impl/graft.md

use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirKind};
use crate::hir::types::NodeTypes;
use rustc_hash::FxHashMap;

mod decide;

use decide::{Decided, Decider};

/// Fold `hir` against one round's inferred types. Answers whether anything was
/// deleted, which is when inference has to run again.
pub(super) fn fold(hir: &mut Hir, types: &NodeTypes, arena: &BindingArena) -> bool {
    let mut reads = FxHashMap::default();
    count_reads(hir, &mut reads);
    let mut folder = Folder {
        decider: Decider::new(types, arena),
        reads,
        changed: false,
    };
    folder.walk(hir);
    folder.changed
}

struct Folder<'a> {
    decider: Decider<'a>,
    /// How many times the whole tree reads each binding, as it stood before
    /// this fold. A deletion only lowers the true counts, so a stale count
    /// can only decline a deletion, never admit one.
    reads: FxHashMap<Binding, u32>,
    changed: bool,
}

impl Folder<'_> {
    fn walk(&mut self, h: &mut Hir) {
        h.for_each_child_mut(|c| self.walk(c));
        match &mut h.kind {
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let (kept, dropped) = match self.decider.condition(cond) {
                    Decided::True => (then_branch, else_branch),
                    Decided::False => (else_branch, then_branch),
                    _ => return,
                };
                if !self.deletable(&[cond, dropped]) {
                    return;
                }
                let kept = std::mem::replace(&mut **kept, Hir::silent(HirKind::Nil, h.span));
                *h = kept;
                self.changed = true;
            }
            HirKind::Match { value, arms } => {
                let Some(live) = self.decider.match_arm(value, arms) else {
                    return;
                };
                let mut dropped: Vec<&Hir> = vec![value];
                for (i, (_, guard, body)) in arms.iter().enumerate() {
                    if i != live {
                        dropped.extend(guard.iter());
                        dropped.push(body);
                    }
                }
                let patterns_bind = arms
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != live)
                    .flat_map(|(_, (p, _, _))| p.bindings().bindings)
                    .any(|b| self.reads.get(&b).copied().unwrap_or(0) > 0);
                if patterns_bind || !self.deletable(&dropped) {
                    return;
                }
                let body = std::mem::replace(&mut arms[live].2, Hir::silent(HirKind::Nil, h.span));
                *h = body;
                self.changed = true;
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                self.fold_cond(clauses, else_branch);
                if clauses.is_empty() {
                    let only = else_branch
                        .take()
                        .map_or_else(|| Hir::silent(HirKind::Nil, h.span), |e| *e);
                    *h = only;
                }
            }
            HirKind::And(_) => self.fold_shortcircuit(h, Decided::False, Decided::True),
            HirKind::Or(_) => self.fold_shortcircuit(h, Decided::True, Decided::False),
            HirKind::Begin(_) => drop_literal_statements(h),
            _ => {}
        }
    }

    /// Drop each clause whose test decides false. A clause whose test decides
    /// true is the last that can run: its body becomes the else, and the
    /// clauses after it go.
    fn fold_cond(&mut self, clauses: &mut Vec<(Hir, Hir)>, else_branch: &mut Option<Box<Hir>>) {
        let mut i = 0;
        while i < clauses.len() {
            match self.decider.condition(&clauses[i].0) {
                Decided::False => {
                    let (test, body) = &clauses[i];
                    if self.deletable(&[test, body]) {
                        clauses.remove(i);
                        self.changed = true;
                        continue;
                    }
                }
                Decided::True => {
                    let mut dropped: Vec<&Hir> = vec![&clauses[i].0];
                    for (test, body) in &clauses[i + 1..] {
                        dropped.push(test);
                        dropped.push(body);
                    }
                    dropped.extend(else_branch.as_deref());
                    if self.deletable(&dropped) {
                        let (_, body) = clauses.remove(i);
                        clauses.truncate(i);
                        *else_branch = Some(Box::new(body));
                        self.changed = true;
                    }
                    return;
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// Stop an `and` (`stop` False) or an `or` (`stop` True) at the operand
    /// that decides it, and drop an operand before the last that decides the
    /// other way (`pass`), whose value is never the result. One operand left is
    /// the whole form.
    fn fold_shortcircuit(&mut self, h: &mut Hir, stop: Decided, pass: Decided) {
        let (HirKind::And(xs) | HirKind::Or(xs)) = &mut h.kind else {
            return;
        };
        let mut i = 0;
        while i < xs.len() {
            let d = self.decider.condition(&xs[i]);
            if d == stop {
                if i + 1 < xs.len() && self.deletable(&xs[i + 1..].iter().collect::<Vec<_>>()) {
                    xs.truncate(i + 1);
                    self.changed = true;
                }
                break;
            }
            if d == pass && i + 1 < xs.len() && self.deletable(&[&xs[i]]) {
                xs.remove(i);
                self.changed = true;
                continue;
            }
            i += 1;
        }
        if xs.len() == 1 {
            let only = xs.pop().expect("one operand is left");
            *h = only;
        }
    }

    /// May these subtrees go? Not when one introduces a binding something
    /// outside them reads: functionalization can move a binding's cell out of
    /// the arm that introduced it, and deleting the arm would orphan the cell.
    fn deletable(&self, parts: &[&Hir]) -> bool {
        let mut introduced = Vec::new();
        let mut inside = FxHashMap::default();
        for p in parts {
            p.introduced_bindings(&mut introduced);
            count_reads(p, &mut inside);
        }
        introduced
            .iter()
            .all(|b| self.reads.get(b).copied().unwrap_or(0) <= inside.get(b).copied().unwrap_or(0))
    }
}

/// Drop each immediate literal before the sequence's tail: a decided guard
/// leaves its `nil` where it stood, and such a statement has no effect and no
/// reader. A sequence left with its tail alone becomes the tail. Inference
/// need not run again for this, because the sequence's type is its tail's.
fn drop_literal_statements(h: &mut Hir) {
    let HirKind::Begin(items) = &mut h.kind else {
        return;
    };
    let tail = items.len().saturating_sub(1);
    let mut i = 0;
    items.retain(|item| {
        let keep = i >= tail || !is_immediate(item);
        i += 1;
        keep
    });
    if items.len() == 1 {
        let only = items.pop().expect("one statement is left");
        *h = only;
    }
}

/// An immediate literal, which evaluates to itself with no effect.
fn is_immediate(h: &Hir) -> bool {
    matches!(
        h.kind,
        HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Keyword(_)
    )
}

/// Count every read of a binding: a `Var`, a write's target, a capture.
fn count_reads(h: &Hir, out: &mut FxHashMap<Binding, u32>) {
    match &h.kind {
        HirKind::Var(b) | HirKind::Assign { target: b, .. } => *out.entry(*b).or_default() += 1,
        HirKind::Lambda(lambda) => {
            for c in &lambda.captures {
                *out.entry(c.binding).or_default() += 1;
            }
        }
        _ => {}
    }
    h.for_each_child(|c| count_reads(c, out));
}
