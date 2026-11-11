// audited: 2026-11-10
//! What a `def`-bound whole-value read of a reassigned captured cell takes.
//!
//! docs/impl/region/reads.md

use super::*;

/// Every `Define` whose value is a whole-value read of a re-storable capture
/// cell, a bare `Var` or the `DerefCell` around one: the reader and its value.
fn def_bound_cell_reads<'h>(hir: &'h Hir, arena: &BindingArena) -> Vec<(Binding, &'h Hir)> {
    fn read_of(h: &Hir) -> Option<Binding> {
        match &h.kind {
            HirKind::Var(b) => Some(*b),
            HirKind::DerefCell { cell } => read_of(cell),
            _ => None,
        }
    }
    fn walk<'h>(h: &'h Hir, arena: &BindingArena, out: &mut Vec<(Binding, &'h Hir)>) {
        if let HirKind::Define { binding, value } = &h.kind {
            if read_of(value).is_some_and(|b| arena.get(b).is_restorable_capture_cell()) {
                out.push((*binding, &**value));
            }
        }
        h.for_each_child(|c| walk(c, arena, out));
    }
    let mut out = Vec::new();
    walk(hir, arena, &mut out);
    out
}

/// A `def` in a function body reads a cell a sibling closure captures and an
/// `assign` repoints. The cell's binder drops its allocating init's producer
/// reference after the store, and the overwrite releases the cell's own, so
/// `batch` holds the array only by a counted read of its own. Counterfactual:
/// a `Define` that records no read site leaves `batch` a holder of the
/// container's regions and no reference, and the overwrite frees the array
/// under it (tests/impl/region-reassign-captured-cell-reader.lisp).
#[test]
fn a_def_bound_read_of_a_captured_cell_is_counted() {
    let (hir, arena, info) = pipeline(
        "(def @h (fn ()\n\
           (var cell (array 1 2))\n\
           (def touch (fn () cell))\n\
           (def batch cell)\n\
           (assign cell (array 3))\n\
           (touch)\n\
           (%length batch)))\n\
         (h)",
    );
    let reads = def_bound_cell_reads(&hir, &arena);
    assert!(
        !reads.is_empty(),
        "precondition: `batch` is a def-bound whole-value read of a captured cell",
    );
    for (reader, value) in reads {
        assert!(
            info.counted_cell_read_sites.contains(&value.id),
            "a def-bound whole-value read of a captured cell takes a counted reference",
        );
        let placeholder = info
            .alloc_region
            .get(&value.id)
            .expect("a counted read mints a placeholder region");
        assert!(
            info.call_result_regions.contains(placeholder),
            "the placeholder is a call-result region, released by value at the reader's last use",
        );
        assert_eq!(
            info.binding_source_regions.get(&reader).map(Vec::as_slice),
            Some(std::slice::from_ref(placeholder)),
            "the reader holds its placeholder alone, and none of the container's regions",
        );
    }
}
