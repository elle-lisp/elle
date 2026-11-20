// audited: 2026-11-19
//! Every body the epoch 18 rule reads in a file: the file's own forms, and each body a form holds.
//!
//! docs/epochs.md
//!
//! The moves and the letrec read the same bodies, so one walk finds them for
//! both. A body is named by the path to the form whose children hold it and
//! the index of its first form among them.

use crate::epoch::placement::reads::each_parts;
use crate::syntax::{Syntax, SyntaxKind};

/// What [`each`] calls with each body.
type Visit<'t, 'v> = dyn FnMut(&[usize], &'t [Syntax], usize) + 'v;

/// Call `visit` with each body in `forms`, an outer body before the bodies
/// inside it. `visit` takes the path to the form that holds the body (empty
/// for the file), that form's children (the file's forms), and the index of
/// the body's first form among them.
pub(super) fn each<'t>(forms: &'t [Syntax], visit: &mut Visit<'t, '_>) {
    visit(&[], forms, 0);
    let mut path = Vec::new();
    for (i, form) in forms.iter().enumerate() {
        path.push(i);
        node(form, &mut path, visit);
        path.pop();
    }
}

/// Visit each body in `node`, which stands at `path`.
fn node<'t>(node: &'t Syntax, path: &mut Vec<usize>, visit: &mut Visit<'t, '_>) {
    if matches!(node.kind, SyntaxKind::Quote(_)) {
        return;
    }
    if let Some(items) = node.as_list() {
        match items.first().and_then(Syntax::as_symbol) {
            Some("quote") => return,
            Some("fn") => visit(path, items, 2),
            Some("defn" | "defmacro") => visit(path, items, 3),
            Some("let" | "let*" | "letrec" | "parameterize") => visit(path, items, 2),
            Some("block") => visit(path, items, 1),
            Some("each") => {
                if let Some((_, coll, _)) = each_parts(items) {
                    visit(path, items, coll + 1);
                }
            }
            _ => {}
        }
    }
    for (i, child) in node.kind.children().iter().enumerate() {
        path.push(i);
        self::node(child, path, visit);
        path.pop();
    }
}
