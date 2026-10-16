// audited: 2026-10-06
//! The letrec walk unions what each init contributes to a binding's regions, so a placeholder init never erases a destructure's.
//!
//! docs/impl/region/rules.md

use super::*;

/// At top level, `(begin (def (a & r) (list 1 2 3)) (length r))` lowers
/// to a letrec whose first init is the begin. The begin's destructure
/// side-effects `binding_regions[r] = [list_region]`, and a later init
/// binds `r = nil` as the placeholder for the pattern binding. The letrec
/// walk must UNION per-init contributions rather than overwrite: the
/// `r = nil` init contributes no regions, so a blind insert would replace
/// the destructure's `binding_regions[r]` with `[]`. That would leave the
/// list's region's `decref_point` at the
/// destructure's id (the post-pass that extends `decref_point` via
/// binding uses skips `r` when its regions are empty), and a consumer that
/// traverses the list reads a freed page.
///
/// This test pins the analysis invariant directly: after region
/// inference, every destructured binding's `binding_source_regions`
/// must include the source value's region(s).
#[test]
fn letrec_init_does_not_overwrite_destructure_binding_regions() {
    use crate::symbol::SymbolTable;
    let mut symbols = SymbolTable::new();
    // Multi-form source. The top-level compile-file-to-fhir letrec
    // declares placeholder `r = nil` initializers for each pattern
    // binding introduced by destructures across the whole file. The
    // letrec walk must union those inits' region contributions with
    // the destructure-assigned binding_regions[r], not overwrite it.
    let source =
        "(begin (def (a & r) (list 1 2 3)) (length r)) (def (a b & r) (list 1 2)) (length r)";
    let (hir, arena) = compile_fhir(source, &mut symbols);
    let info = analyze_regions(&hir, &arena);
    // Find the pattern binding `r` introduced by the destructure.
    // The arena names it from the source symbol, so we look it up
    // by name. The letrec also declares it; the destructure's
    // pattern shares the same Binding id.
    let r_binding = find_binding_by_name(&hir, "r", &arena).expect("expected binding `r`");
    let r_regions = info
        .binding_source_regions
        .get(&r_binding)
        .cloned()
        .unwrap_or_default();
    // Both `(list ...)` allocations feed `r` via destructure-rest
    // at the same Binding id. The letrec walk unions per-init
    // contributions so r tracks BOTH list regions. If the second
    // init's walk overwrote the first's contribution, r would track
    // only the latest list. The FIRST list's region's decref_point
    // would then stay at its destructure id, and `(length r)` inside
    // the begin, which reads r while r still points into the first
    // list, would read a freed page.
    let list_calls = find_calls_to_primitive(&hir, "list", &arena);
    assert_eq!(
        list_calls.len(),
        2,
        "expected exactly two (list ...) calls in the test source"
    );
    let list_regions: Vec<_> = list_calls
        .iter()
        .filter_map(|id| info.alloc_region.get(id).copied())
        .collect();
    let r_covers: Vec<_> = list_regions
        .iter()
        .filter(|lr| r_regions.contains(lr))
        .copied()
        .collect();
    assert_eq!(
        r_covers.len(),
        list_regions.len(),
        "r's binding_source_regions={:?} must include EVERY list region the \
             destructure could have bound it from; got list_regions={:?}, covered={:?}",
        r_regions,
        list_regions,
        r_covers,
    );
}
