// audited: 2026-10-28
//! Pins for the callee side of the inline: once a call site re-walks a callee, its bindings and scopes name what they named before.
//!
//! docs/impl/region/mechanism.md

use super::*;

/// A callee whose reassigned local starts as its parameter, called from two
/// sites with two different literals. The re-walk at each site binds `s` to
/// that site's literal, so a naming the inline keeps names the LAST site's.
const TWO_SITES: &str = "(defn rs [s n]
                           (let [@out s]
                             (while (%lt (length out) n) (assign out (string out out)))
                             (slice out 0 n)))
                         (def x (rs \"abcdefghij\" 50))
                         (rs \"zz\" 30)";

/// The file-scope pipeline `analyze_full` runs, keeping the arena so a test can
/// read binding names.
fn analyze_named(source: &str) -> (Hir, BindingArena, RegionInfo) {
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir(source, &mut symbols);
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&meta);
    let info = analyze_regions_with(&hir, &arena, pc.call_classification);
    (hir, arena, info)
}

/// The lambda whose first parameter is named `param`.
fn callee<'a>(hir: &'a Hir, param: &str, arena: &BindingArena) -> &'a Hir {
    fn walk<'a>(h: &'a Hir, want: SymbolId, arena: &BindingArena, out: &mut Option<&'a Hir>) {
        if let HirKind::Lambda(l) = &h.kind {
            if l.params.first().is_some_and(|p| arena.get(*p).name == want) {
                out.get_or_insert(h);
            }
        }
        h.for_each_child(|c| walk(c, want, arena, out));
    }
    let mut out = None;
    walk(hir, SymbolId::of(param), arena, &mut out);
    out.unwrap_or_else(|| panic!("no lambda whose first parameter is {param}"))
}

/// Every node of `h`'s subtree, `h` included.
fn nodes(h: &Hir) -> Vec<&Hir> {
    fn walk<'a>(h: &'a Hir, out: &mut Vec<&'a Hir>) {
        out.push(h);
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = Vec::new();
    walk(h, &mut out);
    out
}

/// The bindings a binder form in `h`'s subtree introduces, the lambda's own
/// parameters excluded.
fn body_binders(h: &Hir) -> Vec<Binding> {
    let mut out = Vec::new();
    for n in nodes(h) {
        match &n.kind {
            HirKind::Let { bindings, .. }
            | HirKind::Letrec { bindings, .. }
            | HirKind::Loop { bindings, .. } => out.extend(bindings.iter().map(|(b, _)| *b)),
            HirKind::Define { binding, .. } => out.push(*binding),
            _ => {}
        }
    }
    out
}

fn params(h: &Hir) -> Vec<Binding> {
    match &h.kind {
        HirKind::Lambda(l) => l.params.clone(),
        _ => panic!("not a lambda"),
    }
}

#[test]
fn an_inlined_callee_names_its_bindings_by_its_own_regions() {
    // Each region a body binding names is one the callee mints: a parameter's
    // placeholder, or an allocation inside the body. The counter-factual is
    // `out` naming the "zz" literal of the second call site — a region of the
    // CALLER, which the reassign gate then reads as the init it donates.
    let (hir, arena, info) = analyze_named(TWO_SITES);
    let rs = callee(&hir, "s", &arena);
    let mut own: rustc_hash::FxHashSet<Region> = nodes(rs)
        .iter()
        .filter_map(|n| info.alloc_region.get(&n.id).copied())
        .collect();
    for p in params(rs) {
        own.extend(info.binding_source_regions.get(&p).into_iter().flatten());
    }
    for b in body_binders(rs) {
        for r in info.binding_source_regions.get(&b).into_iter().flatten() {
            assert!(
                own.contains(r),
                "{b:?} names {r:?}, which the callee neither allocates nor receives",
            );
        }
    }
}

#[test]
fn an_inlined_callee_suppresses_no_release_of_its_caller() {
    // The caller owns each literal and releases it after the call. Suppressing
    // that release in place of the parameter's leaves the callee releasing its
    // argument twice, and the caller's own release then frees a region a later
    // allocation reuses: the corrupted `x` the end-to-end pin reads.
    let (hir, _arena, info) = analyze_named(TWO_SITES);
    for lit in ["abcdefghij", "zz"] {
        let r = string_literal_region(&hir, &info, lit);
        assert!(
            !info.suppressed_decref_regions.contains(&r),
            "the caller's literal {lit:?} must keep its release"
        );
    }
}

#[test]
fn an_inlined_callee_keeps_its_scopes() {
    // A parameter sits in the lambda body's scope, and a body binding in a scope
    // the body opens. Each scope `rs` opens holds an allocation, so each reads
    // live. A scope the re-walk mints in place of the structural one holds none
    // of the body's allocations, which keep their structural regions, so it
    // reads empty — and a parameter left in the caller's scope names no scope
    // of the callee at all.
    let (hir, arena, info) = analyze_named(TWO_SITES);
    let rs = callee(&hir, "s", &arena);
    let body_scope = info.scope_region.get(&rs.id).copied();
    assert!(body_scope.is_some(), "the lambda body has a scope");
    for p in params(rs) {
        assert_eq!(
            info.binding_region.get(&p).copied(),
            body_scope,
            "a parameter sits in the callee's body scope"
        );
    }
    let scopes: Vec<HirId> = nodes(rs)
        .iter()
        .map(|n| n.id)
        .filter(|id| info.scope_region.contains_key(id))
        .collect();
    let scope_regions: Vec<Region> = scopes.iter().map(|id| info.scope_region[id]).collect();
    for b in body_binders(rs) {
        let r = info.binding_region.get(&b).copied();
        assert!(
            r.is_some_and(|r| scope_regions.contains(&r)),
            "{b:?} sits in a scope the callee opens",
        );
    }
    for id in scopes {
        assert!(
            info.scope_has_local_allocs(id),
            "the scope at {id:?} holds an allocation of the callee"
        );
    }
}
