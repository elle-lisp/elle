// audited: 2026-11-24
//! Two closure-cycle merges whose bodies end in one tail call: the site carries both arenas.
//!
//! docs/impl/region/cycletail.md

use super::*;

/// The merged root of the forward cell the binding `name` holds.
fn cell_root(info: &RegionInfo, arena: &BindingArena, name: &str) -> Region {
    let cell = info
        .begin_cell_regions
        .values()
        .flatten()
        .find(|(b, _)| arena.get(*b).name == crate::value::SymbolId::of(name))
        .map(|&(_, r)| r)
        .unwrap_or_else(|| panic!("`{name}` holds a forward cell"));
    info.merged_root(cell)
}

/// Two admitted cycles whose bodies end in ONE non-member tail call, `(g …)`. The
/// call strands both arenas, so its site must carry both roots: where one scope
/// binds both cycles, and where the second `letrec` sits in the first's tail.
///
/// The counter-factual is a site that keeps one root. The other arena then has no
/// deferred channel, and its binding-scope release falls to the frame-exit hoist.
#[test]
fn merge_records_every_arena_a_shared_tail_site_strands() {
    use crate::hir::testkit::{TWO_CYCLES_NESTED, TWO_CYCLES_ONE_SCOPE};
    for (shape, source) in [
        ("one scope", TWO_CYCLES_ONE_SCOPE),
        ("nested scopes", TWO_CYCLES_NESTED),
    ] {
        let mut symbols = SymbolTable::new();
        let (hir, arena) = compile_fhir(&format!("{source} (f 3)"), &mut symbols);
        let info = analyze_regions(&hir, &arena);
        let a = cell_root(&info, &arena, "a1");
        let b = cell_root(&info, &arena, "b1");
        assert!(
            a == cell_root(&info, &arena, "a2") && b == cell_root(&info, &arena, "b2") && a != b,
            "precondition ({shape}): each cycle merges onto an arena of its own; \
             merged_parent={:?}",
            info.merged_parent,
        );
        assert!(
            info.cycle_tail_release
                .values()
                .any(|roots| roots.contains(&a) && roots.contains(&b)),
            "{shape}: the (g …) tail site must carry both arenas, r{} and r{}; got {:?}",
            a.0,
            b.0,
            info.cycle_tail_release,
        );
    }
}
