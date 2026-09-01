//! Tests for the escape analysis (`super` = `hir::escape`).

use super::*;
use crate::hir::{Hir, HirId, HirKind};

/// Compile a source file to canonical (functionalized) HIR for the escape
/// tests, threading a fresh per-call `CompileCtx`. Every compile names its
/// instance's compile state explicitly, threading the compile context as a
/// parameter (docs/impl/region/ctx.md).
fn compile_fhir(
    src: &str,
    symbols: &mut crate::symbol::SymbolTable,
) -> (Hir, crate::hir::BindingArena) {
    let mut cctx = crate::pipeline::CompileCtx::new();
    crate::pipeline::compile_file_to_fhir(src, symbols, &mut cctx, "<test>").expect("compile")
}

/// Compile a source to canonical HIR *and* the real `CallClassification` the
/// lowerer feeds the live analysis — built from the same interned symbol table
/// via `cached_primitive_meta`, so the declared native effects (`chan/send`'s
/// `Sends`, …) reach both `analyze_escape` and the solver. Tests run under this
/// classification, never the empty default, so they exercise what actually runs.
fn compile_with_cc(
    src: &str,
) -> (
    Hir,
    crate::hir::BindingArena,
    crate::hir::region::CallClassification,
) {
    let mut symbols = crate::symbol::SymbolTable::new();
    let (hir, arena) = compile_fhir(src, &mut symbols);
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&meta);
    (hir, arena, pc.call_classification)
}

/// Compile a source under the real classification and return its
/// `EscapeInfo` alongside the HIR and arena — the fixture every spec test
/// reads. Escape is the **authority**; these tests assert its four-facet spec
/// directly (`docs/impl/escape.md`), never agreement with any other analysis.
fn escape_of(src: &str) -> (Hir, BindingArena, EscapeInfo) {
    let mut symbols = crate::symbol::SymbolTable::new();
    let (hir, arena) = compile_fhir(src, &mut symbols);
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&meta);
    let escape = analyze_escape(&hir, &arena, &pc.call_classification);
    (hir, arena, escape)
}

/// Assert the escape spec for named bindings: each `(name, escapes_activation,
/// escapes_via_return)` triple is checked against every binding that name
/// resolves to. The invariant `returned ⟹ escapes` is checked too.
fn assert_binding_escape(src: &str, expect: &[(&str, bool, bool)]) {
    let (hir, arena, escape) = escape_of(src);
    for &(name, esc, ret) in expect {
        let bs = bindings_named(&hir, &arena, &[name]);
        assert!(!bs.is_empty(), "missing `{name}` in `{src}`");
        for b in bs {
            assert_eq!(
                escape.binding_escapes_activation(b),
                esc,
                "`{name}` activation-escape in `{src}`"
            );
            assert_eq!(
                escape.binding_escapes_via_return(b),
                ret,
                "`{name}` return-escape in `{src}`"
            );
            assert!(
                !escape.binding_escapes_via_return(b) || escape.binding_escapes_activation(b),
                "`{name}` returned but not escaping — return facet must be a subset"
            );
        }
    }
}

/// Every lambda in a compiled program (post-order), paired with the bindings
/// it captures. Shared by the capture-facet tests.
fn lambdas_with_captures(hir: &Hir) -> Vec<(HirId, Vec<Binding>)> {
    fn walk(h: &Hir, out: &mut Vec<(HirId, Vec<Binding>)>) {
        if let HirKind::Lambda { captures, .. } = &h.kind {
            out.push((h.id, captures.iter().map(|c| c.binding).collect()));
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = Vec::new();
    walk(hir, &mut out);
    out
}

/// The binding(s) a `Var(name)` reference resolves to in a program, restricted
/// to those whose name matches `wanted` — a small helper to point a scrutiny
/// test at a specific source binding without depending on raw arena indices.
fn bindings_named(hir: &Hir, arena: &BindingArena, wanted: &[&str]) -> Vec<Binding> {
    fn walk(h: &Hir, out: &mut Vec<Binding>) {
        if let HirKind::Var(b) = &h.kind {
            out.push(*b);
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut all = Vec::new();
    walk(hir, &mut all);
    all.into_iter()
        .filter(|b| {
            let id = arena.get(*b).name;
            wanted.iter().any(|w| crate::value::SymbolId::of(w) == id)
        })
        .collect()
}

mod capture;
mod flow;
