// ── Region merging ────────────────────────────────────────────────────
//
// When two regions collapse into one, split by what forces the collapse:
//
// - `seed` — the builder-idiom child→parent merge, where the merge starts.
// - `selfedge` — the self-edge elimination predicate (transform 2).
// - `recursion` — the letrec closure-cycle merge: which recursion shapes
//   collapse, and which tail callees the merge still admits.
// - `escape` — where the merge stops: a returned cycle, a handed-out member,
//   and a fiber crossing.

// Re-glob the parent's test imports so each submodule can `use super::*;`.
use super::*;

mod escape;
mod recursion;
mod seed;
mod selfedge;

/// The `Letrec` node binding a name (`loop`/`ping`) — the cycle's binding scope,
/// whose scope-exit is the tight, RC-safe drop site for the merged arena.
fn letrec_binding_node(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    name: &str,
) -> Option<HirId> {
    fn walk(
        h: &Hir,
        arena: &BindingArena,
        symbols: &SymbolTable,
        name: &str,
        out: &mut Option<HirId>,
    ) {
        if let HirKind::Letrec { bindings, .. } = &h.kind {
            if bindings
                .iter()
                .any(|(b, _)| symbols.name(arena.get(*b).name) == Some(name))
            {
                *out = Some(h.id);
            }
        }
        h.for_each_child(|c| walk(c, arena, symbols, name, out));
    }
    let mut out = None;
    walk(hir, arena, symbols, name, &mut out);
    out
}

/// The closure (`Lambda` `alloc_region`) regions and the pre-allocated capture-cell
/// (`begin_cell_regions`) regions in `hir` — the two member kinds of a letrec
/// closure-cycle merge.
fn letrec_cycle_members(hir: &Hir, info: &RegionInfo) -> (Vec<Region>, Vec<Region>) {
    fn walk(h: &Hir, info: &RegionInfo, out: &mut Vec<Region>) {
        if matches!(h.kind, HirKind::Lambda { .. }) {
            if let Some(&r) = info.alloc_region.get(&h.id) {
                out.push(r);
            }
        }
        h.for_each_child(|c| walk(c, info, out));
    }
    let mut closures = Vec::new();
    walk(hir, info, &mut closures);
    let cells: Vec<Region> = info
        .begin_cell_regions
        .values()
        .flat_map(|v| v.iter().map(|(_, r)| *r))
        .collect();
    (closures, cells)
}

/// Analyze `source` under the REAL primitive classification, returning the arena
/// so `letrec_binding_node` can locate the cycle. A storing/copying `%`-op compiles
/// as a native funnel `Call`, so a body tail like `(%freeze …)` is a frame-replacing
/// `TailCall` — the shape the non-member body-tail release slot exists for.
fn analyze_cycle_with_effects(
    source: &str,
    symbols: &mut SymbolTable,
) -> (Hir, BindingArena, RegionInfo) {
    let meta = crate::primitives::build_primitive_meta(symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&meta);
    let cc = pc.call_classification;
    let (hir, arena) = compile_fhir(source, symbols);
    let info = analyze_regions_with(&hir, &arena, cc);
    (hir, arena, info)
}

/// The forward-cell regions of the in-lambda `ev`/`od` letrec, and the merged root
/// they collapse onto (the SCC closure of least program order).
fn ev_od_cells(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    info: &RegionInfo,
) -> Vec<Region> {
    let letrec_id =
        letrec_binding_node(hir, arena, symbols, "ev").expect("the in-lambda letrec binding `ev`");
    info.begin_cell_regions
        .get(&letrec_id)
        .map(|v| v.iter().map(|&(_, r)| r).collect())
        .unwrap_or_default()
}
