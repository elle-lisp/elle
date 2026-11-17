// audited: 2026-11-17
//! The epoch 17 rule applied to a syntax tree: each planned run of defs becomes a let.
//!
//! docs/epochs.md
//!
//! The deepest placement goes first. A placement changes only the form that
//! holds its run, and only from the run on, so every path a later placement
//! names still leads where the walk found it.

use super::{Def, Kind, Placement, Plan};
use crate::syntax::{Syntax, SyntaxArena};

/// Apply `plan` to `forms`, the forms it was made over. Answers the number of
/// defs that moved.
pub(crate) fn apply(arena: &SyntaxArena, forms: &mut [Syntax], plan: &Plan) -> usize {
    let mut order: Vec<&Placement> = plan.placements.iter().collect();
    order.sort_by_key(|p| std::cmp::Reverse(p.path.len()));
    let mut moved = 0;
    for placement in order {
        let parent = node_mut(forms, &placement.path);
        let items = parent.kind.children().to_vec();
        let let_form = let_form(arena, &items, placement);
        if placement.replaces {
            *parent = let_form;
        } else {
            let mut next = items[..placement.first].to_vec();
            next.push(let_form);
            next.extend_from_slice(&items[placement.end..]);
            parent.kind = parent.kind.rebuild(arena, &next);
        }
        moved += placement.count;
    }
    moved
}

/// The node `path` names, to write through. The forms come from the reader
/// and nothing else holds them, so the walk owns them (`transform::rewrite_node`).
fn node_mut<'f>(forms: &'f mut [Syntax], path: &[usize]) -> &'f mut Syntax {
    let (first, rest) = path
        .split_first()
        .expect("a placement's path is never empty");
    rest.iter()
        .fold(&mut forms[*first], |node, &i| &mut node.children_mut()[i])
}

/// The let a placement makes from the forms of its parent, spanned at its
/// first def, so an error in it points where the author wrote the def.
fn let_form(arena: &SyntaxArena, items: &[Syntax], placement: &Placement) -> Syntax {
    let run = placement.first..placement.first + placement.count;
    let defs: Vec<Def<'_>> = items[run.clone()]
        .iter()
        .map(|n| Def::of(n).expect("a planned run holds defs"))
        .collect();
    let span = defs[0].node.span;
    let mut bindings = Vec::with_capacity(defs.len() * 2);
    for def in &defs {
        let name = def.name();
        bindings.push(if def.needs_at() {
            Syntax::symbol(arena, &format!("@{}", def.plain_name()), name.span)
        } else {
            *name
        });
        bindings.push(match def.kind {
            Kind::Defn => {
                let mut function = vec![Syntax::symbol(arena, "fn", def.node.span)];
                function.extend_from_slice(&def.items[2..]);
                Syntax::list(arena, &function, def.node.span)
            }
            Kind::Def | Kind::Var => def.items[2],
        });
    }
    let mut form = vec![
        Syntax::symbol(arena, "let", span),
        Syntax::array(arena, &bindings, span),
    ];
    let body = &items[run.end..placement.end];
    if body.is_empty() {
        let last = defs.last().expect("a run holds a def");
        form.push(Syntax::symbol(arena, last.plain_name(), last.name().span));
    } else {
        form.extend_from_slice(body);
    }
    Syntax::list(arena, &form, span)
}
