// audited: 2026-11-11
//! The fixture the hoist tests share: a compile to functional HIR, the pass, and the walks that find what moved and what stayed.
//!
//! docs/impl/hoist.md

use super::hoist_loop_constants;
use crate::hir::expr::{Hir, HirId, HirKind};
use crate::hir::testkit::{HirFixture, Stage};
use crate::hir::BindingArena;
use crate::value::SymbolId;

mod moves;
mod stays;

/// `source` compiled to functional HIR, every `while` a `loop`, then hoisted.
/// The fixture binds the primitives and the nil-returning stubs `f` and `g`,
/// and nothing from the stdlib, so a loop counts with `%lt` and `%add`.
fn hoisted(source: &str) -> (Hir, BindingArena) {
    let (mut hir, mut arena, _) = HirFixture::new().stage(Stage::Functional).build(source);
    hoist_loop_constants(&mut hir, &mut arena);
    (hir, arena)
}

/// A function of `xs` and `n` whose `while` runs `body` on every pass.
fn in_loop(body: &str) -> String {
    format!("(fn [xs n] (var i 0) (while (%lt i n) {body} (assign i (%add i 1))) i)")
}

/// Each `let` the pass built: one whose body is a loop. Functionalization
/// wraps every `while` in its `block`, so no `let` it leaves holds a loop
/// directly.
fn hoist_lets(hir: &Hir) -> Vec<&Hir> {
    fn walk<'a>(h: &'a Hir, out: &mut Vec<&'a Hir>) {
        if let HirKind::Let { body, .. } = &h.kind {
            if matches!(body.kind, HirKind::Loop { .. }) {
                out.push(h);
            }
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = Vec::new();
    walk(hir, &mut out);
    out
}

/// The inits of every `let` the pass built: the constants it moved.
fn moved(hir: &Hir) -> Vec<&Hir> {
    hoist_lets(hir)
        .into_iter()
        .flat_map(|l| match &l.kind {
            HirKind::Let { bindings, .. } => bindings.iter().map(|(_, init)| init),
            _ => unreachable!(),
        })
        .collect()
}

/// Every string literal at or below `hir`.
fn strings(hir: &Hir) -> Vec<String> {
    fn walk(h: &Hir, out: &mut Vec<String>) {
        if let HirKind::String(s) = &h.kind {
            out.push(s.clone());
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = Vec::new();
    walk(hir, &mut out);
    out
}

/// Every string literal the pass moved, the strings inside a moved collection
/// literal included.
fn moved_strings(hir: &Hir) -> Vec<String> {
    moved(hir).into_iter().flat_map(strings).collect()
}

/// Every string literal at or below a loop node: the ones still built on
/// every pass, a nested function's included.
fn strings_in_loops(hir: &Hir) -> Vec<String> {
    fn walk(h: &Hir, out: &mut Vec<String>) {
        if matches!(h.kind, HirKind::Loop { .. }) {
            out.extend(strings(h));
            return;
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = Vec::new();
    walk(hir, &mut out);
    out
}

/// The name of the binding a call node calls, when its callee is a `Var`.
fn callee(hir: &Hir, arena: &BindingArena) -> Option<SymbolId> {
    match &hir.kind {
        HirKind::Call { func, .. } => match &func.kind {
            HirKind::Var(b) => Some(arena.get(*b).name),
            _ => None,
        },
        _ => None,
    }
}

/// How many calls at or below a loop node call the binding spelled `name`.
fn calls_in_loops(hir: &Hir, arena: &BindingArena, name: &str) -> usize {
    fn count(h: &Hir, arena: &BindingArena, name: SymbolId) -> usize {
        let own = usize::from(callee(h, arena) == Some(name));
        let mut n = own;
        h.for_each_child(|c| n += count(c, arena, name));
        n
    }
    fn walk(h: &Hir, arena: &BindingArena, name: SymbolId) -> usize {
        if matches!(h.kind, HirKind::Loop { .. }) {
            return count(h, arena, name);
        }
        let mut n = 0;
        h.for_each_child(|c| n += walk(c, arena, name));
        n
    }
    walk(hir, arena, SymbolId::of(name))
}

/// Every node id in `hir`, in walk order.
fn ids(hir: &Hir) -> Vec<HirId> {
    fn walk(h: &Hir, out: &mut Vec<HirId>) {
        out.push(h.id);
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = Vec::new();
    walk(hir, &mut out);
    out
}
