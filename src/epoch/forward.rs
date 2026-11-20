// audited: 2026-11-19
//! The epoch 18 rule's reading of a tree: which defs move above the forward reads that reach them, and which become a letrec or a let.
//!
//! docs/epochs.md
//! docs/bindings.md
//!
//! The rule reads a file twice. The first walk answers each body's new
//! order, a [`Reorder`]: a def a forward read reaches moves above the read,
//! where no code that runs can tell ([`order`]). The second walk reads the
//! moved tree and answers a [`Plan`]. A [`Binder`] names the defs one
//! `letrec` or `let` binds, by the path to the form whose forms hold them,
//! and a [`Left`] names a read or a def the rule leaves. The tree migration
//! applies both through [`tree`], and `elle rewrite` turns the same answers
//! into edits of the text, as the epoch 17 rule's plan is shared
//! (`super::placement`).
//!
//! [`bodies`] finds each body, [`walk`] says which runs each body holds, and
//! [`scan`] where a form reads a name.

use super::placement::node_at;
use super::rules::{migrations_in_range, MigrationRule};
use crate::syntax::{Span, Syntax};

mod bodies;
mod order;
mod scan;
pub(crate) mod tree;
mod walk;

/// One body's forms in the order the moves leave them.
pub(crate) struct Reorder {
    /// The child index at each step from a top-level form down to the form
    /// whose forms hold the body. Empty for the file's own forms.
    pub path: Vec<usize>,
    /// The index of the body's first form among those forms.
    pub start: usize,
    /// The body's forms in their new order, each by its index among those
    /// forms.
    pub order: Vec<usize>,
    /// The forms that moved, by the same index: those whose order to the
    /// forms that stayed changed.
    pub moved: Vec<usize>,
}

/// Defs the rule binds over the forms after them in their body.
pub(crate) struct Binder {
    /// The child index at each step from a top-level form down to the form
    /// whose forms hold the defs. Empty for the file's own forms.
    pub path: Vec<usize>,
    /// The index of the first def among those forms.
    pub first: usize,
    /// How many defs it binds.
    pub count: usize,
    /// The form that binds them: `letrec` for a run, `let` for one def whose
    /// value reads its own name.
    pub head: &'static str,
}

/// A form the rule leaves as written.
pub(crate) struct Left {
    /// Where the note points: the forward read, or the def.
    pub span: Span,
    /// What the note says.
    pub message: String,
    /// Whether the compiler refuses the file: at epoch 18 the read would find
    /// a binding outside the body, or none.
    pub refused: bool,
}

/// Every binder and every left form over one file's forms. A binder comes
/// before each binder inside it, and before each later one in its body.
pub(crate) struct Plan {
    pub binders: Vec<Binder>,
    pub left: Vec<Left>,
}

/// Whether migrating from epoch `from` to epoch `to` crosses the epoch that
/// adds the rule.
pub fn applies(from: u64, to: u64) -> bool {
    migrations_in_range(from, to)
        .flat_map(|m| m.rules.iter())
        .any(|rule| matches!(rule, MigrationRule::DefsBeforeReads))
}

/// Read `forms`, a file's top-level forms, and answer each body whose order
/// the moves change, a body inside another first.
pub(crate) fn moves(forms: &[Syntax]) -> Vec<Reorder> {
    let mut reorders = Vec::new();
    bodies::each(forms, &mut |path, items, start| {
        reorders.extend(order::of(path, items, start));
    });
    reorders.sort_by_key(|r| std::cmp::Reverse(r.path.len()));
    reorders
}

/// Read `forms`, a file's top-level forms after the moves, and plan the
/// letrecs and lets over them.
pub(crate) fn plan(forms: &[Syntax]) -> Plan {
    walk::over(forms)
}

/// The forms a path names: the file's own, or the children of the node at
/// the path.
pub(crate) fn body_items<'t>(forms: &'t [Syntax], path: &[usize]) -> &'t [Syntax] {
    if path.is_empty() {
        forms
    } else {
        node_at(forms, path).kind.children()
    }
}
