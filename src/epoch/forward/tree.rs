// audited: 2026-11-20
//! The epoch 18 rule applied to a syntax tree: begins splice, bodies take their new order, then binders take the rest of their body.
//!
//! docs/epochs/bodies.md
//!
//! The last begin, the deepest body and the deepest binder go first, and of
//! two binders in one body the later one. Each changes only the forms of its
//! own body, so every path and index still to come leads where the walk found
//! it.

use super::{Binder, Plan, Reorder};
use crate::epoch::placement::tree::{binding_form, node_mut};
use crate::epoch::placement::{is_begin, Def};
use crate::syntax::{Syntax, SyntaxArena};

/// Splice each begin [`super::splices`] names in `forms` into the forms
/// around it, and each begin directly in it too. Answers the number of begins
/// spliced.
pub(crate) fn splice(arena: &SyntaxArena, forms: &mut Vec<Syntax>) -> usize {
    let mut paths = super::splices(forms);
    paths.sort();
    let mut spliced = 0;
    for path in paths.iter().rev() {
        let (&at, parent) = path.split_last().expect("a begin's path is never empty");
        let around = |items: &[Syntax], spliced: &mut usize| -> Vec<Syntax> {
            let mut next = items[..at].to_vec();
            next.extend(begin_forms(&items[at], spliced));
            next.extend_from_slice(&items[at + 1..]);
            next
        };
        if parent.is_empty() {
            *forms = around(forms, &mut spliced);
        } else {
            let node = node_mut(forms, parent);
            let next = around(node.kind.children(), &mut spliced);
            node.kind = node.kind.rebuild(arena, &next);
        }
    }
    spliced
}

/// The forms of `begin`, through each begin directly in it, counting each
/// begin in `spliced`.
fn begin_forms(begin: &Syntax, spliced: &mut usize) -> Vec<Syntax> {
    *spliced += 1;
    let mut forms = Vec::new();
    for child in &begin.kind.children()[1..] {
        if is_begin(child) {
            forms.extend(begin_forms(child, spliced));
        } else {
            forms.push(*child);
        }
    }
    forms
}

/// Put each body `reorders` names in its new order. `reorders` come from
/// [`super::moves`] over `forms`, a body inside another first. Answers the
/// number of defs that moved.
pub(crate) fn reorder(arena: &SyntaxArena, forms: &mut Vec<Syntax>, reorders: &[Reorder]) -> usize {
    for r in reorders {
        let ordered = |items: &[Syntax]| -> Vec<Syntax> {
            items[..r.start]
                .iter()
                .copied()
                .chain(r.order.iter().map(|&k| items[k]))
                .collect()
        };
        if r.path.is_empty() {
            *forms = ordered(forms);
        } else {
            let parent = node_mut(forms, &r.path);
            let next = ordered(parent.kind.children());
            parent.kind = parent.kind.rebuild(arena, &next);
        }
    }
    reorders.iter().map(|r| r.moved.len()).sum()
}

/// Apply `plan` to `forms`, the forms it was made over. Answers the number of
/// defs the binders bind.
pub(crate) fn apply(arena: &SyntaxArena, forms: &mut Vec<Syntax>, plan: &Plan) -> usize {
    let mut order: Vec<&Binder> = plan.binders.iter().collect();
    order.sort_by_key(|b| std::cmp::Reverse((b.path.len(), b.first)));
    let mut bound = 0;
    for binder in order {
        if binder.path.is_empty() {
            let rest = forms.split_off(binder.first);
            forms.push(form(arena, &rest, binder));
        } else {
            let parent = node_mut(forms, &binder.path);
            let items = parent.kind.children().to_vec();
            let mut next = items[..binder.first].to_vec();
            next.push(form(arena, &items[binder.first..], binder));
            parent.kind = parent.kind.rebuild(arena, &next);
        }
        bound += binder.count;
    }
    bound
}

/// The form `binder` makes of `rest`, the forms of its body from its first
/// def on.
fn form(arena: &SyntaxArena, rest: &[Syntax], binder: &Binder) -> Syntax {
    let defs: Vec<Def<'_>> = rest[..binder.count]
        .iter()
        .map(|n| Def::of(n).expect("a binder binds defs"))
        .collect();
    binding_form(arena, binder.head, &defs, &rest[binder.count..])
}
