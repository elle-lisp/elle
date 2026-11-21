// audited: 2026-11-20
//! The epoch 17 rule applied to a syntax tree: each def that assigns becomes an assign, then each planned run of defs becomes a let.
//!
//! docs/epochs/bodies.md
//!
//! The deepest placement goes first. A placement changes only the form that
//! holds its run, and only from the run on, so every path a later placement
//! names still leads where the walk found it. The epoch 18 rule builds its
//! forms with [`binding_form`] too.

use super::{Assigns, Decl, Def, Kind, Placement, Plan};
use crate::syntax::{Syntax, SyntaxArena, SyntaxKind};

/// Apply the rule to `forms`, a file's top-level forms: the defs that assign,
/// then the plan over the tree they leave. Answers the number of defs that
/// assign or move.
pub(crate) fn migrate(arena: &SyntaxArena, forms: &mut Vec<Syntax>) -> usize {
    let assigned = assign(arena, forms, &super::assigns(forms));
    let plan = super::plan(forms);
    assigned + apply(arena, forms, &plan)
}

/// Apply `assigns` to `forms`, the forms they were read from. Each def becomes
/// an `assign` where it stands, so every path still leads where the walk found
/// it. Then each binding goes before its home, the last home first, so a path
/// still to come leads where the walk found it too. Answers the number of
/// defs that assign.
fn assign(arena: &SyntaxArena, forms: &mut Vec<Syntax>, assigns: &Assigns) -> usize {
    for path in &assigns.defs {
        let node = node_mut(forms, path);
        let def = Def::of(node).expect("an assign names a def");
        let form = [
            Syntax::symbol(arena, "assign", def.items[0].span),
            Syntax::symbol(arena, def.plain_name(), def.name().span),
            value(arena, &def),
        ];
        *node = Syntax::list(arena, &form, node.span);
    }
    let mut decls: Vec<&Decl> = assigns.decls.iter().collect();
    decls.sort_by(|a, b| a.home.cmp(&b.home));
    for decl in decls.into_iter().rev() {
        let (&at, parent) = decl.home.split_last().expect("a home path is never empty");
        let form = decl_form(arena, decl);
        if parent.is_empty() {
            forms.insert(at, form);
        } else {
            let node = node_mut(forms, parent);
            let mut items = node.kind.children().to_vec();
            items.insert(at, form);
            node.kind = node.kind.rebuild(arena, &items);
        }
    }
    assigns.defs.len()
}

/// The `def` that makes `decl`'s binding, spanned at the first def that
/// assigns it.
fn decl_form(arena: &SyntaxArena, decl: &Decl) -> Syntax {
    let span = decl.span;
    let value = if decl.seeded {
        Syntax::symbol(arena, &decl.name, span)
    } else {
        Syntax::new(SyntaxKind::Nil, span)
    };
    let form = [
        Syntax::symbol(arena, "def", span),
        Syntax::symbol(arena, &format!("@{}", decl.name), span),
        value,
    ];
    Syntax::list(arena, &form, span)
}

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
pub(crate) fn node_mut<'f>(forms: &'f mut [Syntax], path: &[usize]) -> &'f mut Syntax {
    let (first, rest) = path
        .split_first()
        .expect("a placement's path is never empty");
    rest.iter()
        .fold(&mut forms[*first], |node, &i| &mut node.children_mut()[i])
}

/// The let a placement makes from the forms of its parent.
fn let_form(arena: &SyntaxArena, items: &[Syntax], placement: &Placement) -> Syntax {
    let run = placement.first..placement.first + placement.count;
    let defs: Vec<Def<'_>> = items[run.clone()]
        .iter()
        .map(|n| Def::of(n).expect("a planned run holds defs"))
        .collect();
    binding_form(arena, "let", &defs, &items[run.end..placement.end])
}

/// The `head` form, a `let` or a `letrec`, that binds each of `defs` over
/// `body`, spanned at the first def, so an error in it points where the author
/// wrote that def. A `var` binds its name with `@`. With no body, the form
/// reads the last name.
pub(crate) fn binding_form(
    arena: &SyntaxArena,
    head: &str,
    defs: &[Def<'_>],
    body: &[Syntax],
) -> Syntax {
    let span = defs[0].node.span;
    let mut bindings = Vec::with_capacity(defs.len() * 2);
    for def in defs {
        let name = def.name();
        bindings.push(if def.needs_at() {
            Syntax::symbol(arena, &format!("@{}", def.plain_name()), name.span)
        } else {
            *name
        });
        bindings.push(value(arena, def));
    }
    let mut form = vec![
        Syntax::symbol(arena, head, span),
        Syntax::array(arena, &bindings, span),
    ];
    if body.is_empty() {
        let last = defs.last().expect("a binding form binds a def");
        form.push(Syntax::symbol(arena, last.plain_name(), last.name().span));
    } else {
        form.extend_from_slice(body);
    }
    Syntax::list(arena, &form, span)
}

/// The value `def` binds: its own, or for a `defn` the `fn` its parameters
/// and body make.
fn value(arena: &SyntaxArena, def: &Def<'_>) -> Syntax {
    match def.kind {
        Kind::Defn => {
            let mut function = vec![Syntax::symbol(arena, "fn", def.node.span)];
            function.extend_from_slice(&def.items[2..]);
            Syntax::list(arena, &function, def.node.span)
        }
        Kind::Def | Kind::Var => def.items[2],
    }
}
