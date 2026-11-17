// audited: 2026-11-17
//! The walk that finds each `def` form a body binds: its own forms, and the forms of each `begin` among them.
//!
//! docs/bindings.md
//!
//! The walk mirrors the placement rule: a `def` binds only directly in a body
//! or in a `begin` that stands directly in one. A `def` anywhere else is a
//! compile error at its site, so the walk does not look for it.

use crate::syntax::Syntax;

/// A `def` form the walk reached.
pub(in crate::hir::analyze) struct Reached<'s> {
    pub(in crate::hir::analyze) items: &'s [Syntax],
}

/// Every `def` form `syntax`, a form of a body, binds in that body, in the
/// order the analyzer reaches them.
pub(in crate::hir::analyze) fn reach<'s>(syntax: &'s Syntax, out: &mut Vec<Reached<'s>>) {
    let Some(items) = syntax.as_list() else {
        return;
    };
    match items.first().and_then(Syntax::as_symbol) {
        Some("begin" | "do") => {
            for item in &items[1..] {
                reach(item, out);
            }
        }
        Some("def" | "var") if items.len() == 3 => out.push(Reached { items }),
        _ => {}
    }
}
