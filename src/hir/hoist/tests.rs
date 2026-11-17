// audited: 2026-11-16
//! The fixture the hoist tests share: a compile to functional HIR, the pass, and the walks that find what moved and what stayed.
//!
//! docs/impl/hoist.md

use super::hoist_constants;
use crate::hir::binding::{Binding, CaptureKind};
use crate::hir::expr::{Hir, HirId, HirKind, Lambda};
use crate::hir::testkit::{HirFixture, Stage};
use crate::hir::BindingArena;
use crate::value::SymbolId;

mod captures;
mod merge;
mod moves;
mod stays;

/// `source` compiled to functional HIR, every `while` a `loop`, then hoisted.
/// The fixture wraps the source in a `letrec` that binds the nil-returning
/// stubs `f` and `g`, and binds the primitives and nothing from the stdlib,
/// so a loop counts with `%lt` and `%add`.
fn hoisted(source: &str) -> (Hir, BindingArena) {
    let (mut hir, mut arena, _) = HirFixture::new().stage(Stage::Functional).build(source);
    hoist_constants(&mut hir, &mut arena);
    (hir, arena)
}

/// A function of `xs` and `n` whose `while` runs `body` on every pass.
fn in_loop(body: &str) -> String {
    format!("(fn [xs n] (var i 0) (while (%lt i n) {body} (assign i (%add i 1))) i)")
}

/// The `let` the pass built around the root: the fixture's root is its
/// `letrec`, so a root `let` whose body is that `letrec` is the pass's.
fn root_let(hir: &Hir) -> Option<&Hir> {
    match &hir.kind {
        HirKind::Let { body, .. } if matches!(body.kind, HirKind::Letrec { .. }) => Some(hir),
        _ => None,
    }
}

/// The bindings of the root `let`, each with the constant it moved.
fn root_bindings(hir: &Hir) -> &[(Binding, Hir)] {
    match root_let(hir).map(|l| &l.kind) {
        Some(HirKind::Let { bindings, .. }) => bindings,
        _ => &[],
    }
}

/// The inits of the root `let`: the constants the pass moved.
fn moved(hir: &Hir) -> Vec<&Hir> {
    root_bindings(hir).iter().map(|(_, init)| init).collect()
}

/// The binding the root `let` binds to the string `s`, if it moved.
fn moved_string(hir: &Hir, s: &str) -> Option<Binding> {
    root_bindings(hir)
        .iter()
        .find(|(_, init)| matches!(&init.kind, HirKind::String(x) if x == s))
        .map(|(b, _)| *b)
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

/// Every string literal at or below a node of the kind `at` picks: the ones
/// still built on every call or pass.
fn strings_under(hir: &Hir, at: fn(&HirKind) -> bool) -> Vec<String> {
    fn walk(h: &Hir, at: fn(&HirKind) -> bool, out: &mut Vec<String>) {
        if at(&h.kind) {
            out.extend(strings(h));
            return;
        }
        h.for_each_child(|c| walk(c, at, out));
    }
    let mut out = Vec::new();
    walk(hir, at, &mut out);
    out
}

/// Every string literal a function still builds on each call.
fn strings_in_fns(hir: &Hir) -> Vec<String> {
    strings_under(hir, |k| matches!(k, HirKind::Lambda(_)))
}

/// Every string literal a loop still builds on each pass.
fn strings_in_loops(hir: &Hir) -> Vec<String> {
    strings_under(hir, |k| matches!(k, HirKind::Loop { .. }))
}

/// Every lambda in `hir`, in pre-order: an enclosing lambda before the ones
/// inside it.
fn lambdas(hir: &Hir) -> Vec<&Lambda> {
    fn walk<'a>(h: &'a Hir, out: &mut Vec<&'a Lambda>) {
        if let Some(l) = h.kind.as_lambda() {
            out.push(l);
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = Vec::new();
    walk(hir, &mut out);
    out
}

/// The kind of `lambda`'s capture of `b`, if it captures `b`.
fn capture_of(lambda: &Lambda, b: Binding) -> Option<CaptureKind> {
    lambda
        .captures
        .iter()
        .find(|c| c.binding == b)
        .map(|c| c.kind)
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

/// How many calls at or below a lambda node call the binding spelled `name`.
fn calls_in_fns(hir: &Hir, arena: &BindingArena, name: &str) -> usize {
    fn count(h: &Hir, arena: &BindingArena, name: SymbolId) -> usize {
        let mut n = usize::from(callee(h, arena) == Some(name));
        h.for_each_child(|c| n += count(c, arena, name));
        n
    }
    fn walk(h: &Hir, arena: &BindingArena, name: SymbolId) -> usize {
        if matches!(h.kind, HirKind::Lambda(_)) {
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
