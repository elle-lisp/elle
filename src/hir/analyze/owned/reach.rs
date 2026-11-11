// audited: 2026-11-09
//! The walk that finds each `def` form binding in the scope a form is analyzed in.
//!
//! docs/bindings.md
//!
//! The walk mirrors the analyzer: it enters each form the analyzer analyzes in
//! the scope it is in, and stops at each form that pushes a scope of its own.
//! A `def` the walk misses binds at its site, as a later `def` of a name does.

use crate::syntax::{Syntax, SyntaxKind};

/// A `def` the walk reached, and whether it reached it through `begin` alone.
pub(in crate::hir::analyze) struct Reached<'s> {
    pub(in crate::hir::analyze) items: &'s [Syntax],
    pub(in crate::hir::analyze) straight: bool,
}

/// The forms whose operands the analyzer analyzes in a scope of their own, or
/// not as code at all. The walk does not enter them.
const OPENS_SCOPE: &[&str] = &[
    "fn",
    "let",
    "letrec",
    "block",
    "%file-body",
    "quote",
    "environment",
    "bound",
    "attune!",
    "silent!",
    "numeric!",
    "immutable!",
    "unicode!",
    "signal",
    "splice",
];

/// Every `def` form `syntax` holds that binds in the scope `syntax` is
/// analyzed in, in the order the analyzer reaches them.
pub(in crate::hir::analyze) fn reach<'s>(
    syntax: &'s Syntax,
    straight: bool,
    out: &mut Vec<Reached<'s>>,
) {
    match &syntax.kind {
        SyntaxKind::List(items) => reach_list(items, straight, out),
        SyntaxKind::Array(items)
        | SyntaxKind::ArrayMut(items)
        | SyntaxKind::Struct(items)
        | SyntaxKind::StructMut(items)
        | SyntaxKind::Set(items)
        | SyntaxKind::SetMut(items)
        | SyntaxKind::Bytes(items)
        | SyntaxKind::BytesMut(items) => {
            for item in items.iter() {
                reach(item, false, out);
            }
        }
        SyntaxKind::Splice(inner) => reach(inner, false, out),
        _ => {}
    }
}

fn reach_list<'s>(items: &'s [Syntax], straight: bool, out: &mut Vec<Reached<'s>>) {
    let rest = items.get(1..).unwrap_or(&[]);
    match items.first().and_then(Syntax::as_symbol) {
        Some("begin" | "do") => {
            for item in rest {
                reach(item, straight, out);
            }
        }
        Some("def" | "var") if items.len() == 3 => {
            reach(&items[2], false, out);
            out.push(Reached { items, straight });
        }
        // The scrutinee is analyzed where the match is; each arm opens a scope.
        Some("match") => {
            if let Some(scrutinee) = rest.first() {
                reach(scrutinee, false, out);
            }
        }
        Some(head) if OPENS_SCOPE.contains(&head) => {}
        _ => {
            for item in items {
                reach(item, false, out);
            }
        }
    }
}
