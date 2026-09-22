// audited: 2026-09-22
//! A returned fn-local cell takes the same model an unreturned one takes, because the Return's mint is a reference of its own.
//!
//! docs/impl/region/bindings.md

use super::*;

/// A reassigned binding a conditional `assign` gives a PHI takes the model on
/// the counted-init route: the phi copies the store's regions onto the binding's
/// own source set, so the aliased overlap withholds the donation, the cell
/// counts its heap init at the binder, and nothing is suppressed. Being read at
/// the tail decides nothing, since the `Return`'s mint is a reference the
/// callee did not hold a moment earlier (docs/impl/region/bindings.md
/// § "Returned fn-local reassigned mutables"). The phi itself stays UNCOUNTED —
/// a version hands the one reference along rather than claiming a second
/// (docs/impl/region/reads.md § "A version of the container is not an alias of
/// it").
#[test]
fn reassign_gate_counts_a_phi_carried_returned_value() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (c)\n\
           (begin (var x (%pair 1 2))\n\
                  (if c (assign x (%pair 3 4)) nil)\n\
                  x)))\n\
         (h 1)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.drop_on_overwrite_sites.contains(site)),
        "the phi-carried binding takes the model — its drop-on-overwrite \
         releases the cell's own counted reference"
    );
    assert!(
        info.suppressed_decref_regions.is_empty(),
        "the counted-init route suppresses nothing (got {:?})",
        info.suppressed_decref_regions
    );
    assert!(
        !info.counted_cell_init_sites.is_empty(),
        "a heap init under an aliased overlap is counted at the binder",
    );
    assert!(
        info.counted_cell_read_sites.is_empty(),
        "the phi is a version, not a whole-value read — counting it would claim \
         a second reference for a single holding (got {:?})",
        info.counted_cell_read_sites,
    );
}

/// A sole-held fn-local mutable accumulated in a LOOP and read at the tail
/// (returned) must have its own loop-carried region SUPPRESSED while its
/// assign-value region is KEPT. The loop gives the binding a loop-carried
/// region distinct from the per-iteration assign-value region, but both alias
/// the one returned value; the unsuppressed baseline would emit a value-route
/// decref for EACH at the `Return`, double-freeing the callee's single
/// reference — the second frees the caller's minted reference before the
/// caller's read (`region_capture_cell_string_accum_uaf`). Suppressing the
/// binding's own region keeps the single assign-value decref (the callee's one
/// release) and lets the `Return` mint carry ownership to the caller. Contrast
/// `reassign_gate_counts_a_phi_carried_returned_value` (an `if`-shaped reassign
/// is phi-aliased ⇒ the counted-init route, nothing suppressed) and a
/// single-assign cell, whose binding and assign-value regions coalesce so there
/// is nothing to suppress.
#[test]
fn reassign_gate_splits_returned_loop_carried_region() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@acc (%pair 0 0)]\n\
             (var i 0)\n\
             (while (%lt i n)\n\
               (begin (assign acc (%pair i 7))\n\
                      (assign i (%add i 1))))\n\
             acc)))\n\
         (h 3)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of acc");
    // `acc` is the heap-carrying reassigned mutable: it has ≥2 source regions
    // (the loop-carried binding region plus the per-iteration assign-value
    // region); the immediate `i` counter carries none.
    let acc = sites
        .iter()
        .map(|(_, b)| *b)
        .find(|b| {
            info.binding_source_regions
                .get(b)
                .is_some_and(|rs| rs.len() >= 2)
        })
        .expect("acc: a returned heap mutable with a loop-carried + assign-value region");
    let acc_regs = &info.binding_source_regions[&acc];
    let suppressed = acc_regs
        .iter()
        .filter(|r| info.suppressed_decref_regions.contains(r))
        .count();
    let kept = acc_regs.len() - suppressed;
    // The split: the loop-carried binding region is suppressed (else it
    // double-frees the returned value at the Return)…
    assert!(
        suppressed >= 1,
        "returned loop-reassigned mutable must suppress its loop-carried binding \
         region (regs={:?}, suppressed={:?})",
        acc_regs,
        info.suppressed_decref_regions
    );
    // …while at least one assign-value region is KEPT (the callee's one release,
    // which the return mint balances against the caller).
    assert!(
        kept >= 1,
        "returned loop-reassigned mutable must KEEP its assign-value region decref \
         (got all {} regions suppressed: {:?})",
        acc_regs.len(),
        acc_regs
    );
}

/// A RETURNED fn-local reassigned mutable takes the same container model an
/// unreturned one takes. The return claims the reference the `Return`'s mint
/// creates, which the callee did not hold a moment earlier; the cell's own
/// reference is the counted store's and claims nothing from anyone. So being
/// returned decides nothing about the container half — and withholding it
/// strands every value the loop displaces, one region per trip
/// (docs/impl/region/bindings.md § "Returned fn-local reassigned mutables — the
/// return claims the MINT's reference, not the cell's";
/// `tests/elle/region-loop-acc-return.lisp` measures the strand).
#[test]
fn reassign_gate_counts_a_returned_loop_accumulator() {
    let (hir, info) = returned_loop_accumulator();
    let (acc, cell) = returned_accumulator_cell(&hir, &info);
    assert!(
        !cell.forwards_content,
        "nothing forwards on from the returned link, so it keeps the content \
         drop — the release of the cell's own reference, which the `Return` mint \
         has already replaced for the caller"
    );
    assert!(
        cell.stores.value_regions().next().is_some(),
        "precondition: the loop's assign records the value it stores ({acc:?})"
    );
    for (site, b) in find_reassign_sites(&hir) {
        if b != acc {
            continue;
        }
        assert!(
            info.drop_on_overwrite_sites.contains(&site),
            "a returned cell keeps drop-on-overwrite at @{} — the channel that \
             releases each value the loop displaces",
            site.0
        );
        assert!(
            !info.donated_overwrite_sites.contains(&site),
            "a fn-local cell's store is COUNTED, whatever the tail does with the \
             content: donating at @{} would leave the producer's reference with \
             no release",
            site.0
        );
    }
}

/// A `Return` is a reader of the cell's content, and the cell's reference
/// protects it: the stored value's producer release stays pinned to its STORE
/// site rather than riding the returned-region extension out to the `Return`.
///
/// The extension exists so a returned region's release orders after the mint.
/// A cell-stored value needs nothing from it — the cell holds a counted
/// reference of its own from the store onward, and drops it at the content drop
/// the same `Return` node carries, after the mint. Left extended, the one
/// release names whatever the producer's ANF slot holds LAST, so every earlier
/// value of a loop is stranded (docs/impl/region/bindings.md § "A `Return` is a
/// reader of the cell's content").
#[test]
fn reassign_return_does_not_extend_a_cell_stored_value() {
    let (hir, info) = returned_loop_accumulator();
    let (acc, cell) = returned_accumulator_cell(&hir, &info);
    let stores: Vec<HirId> = cell.stores.sites().collect();
    assert_eq!(
        stores.len(),
        1,
        "precondition: the loop body assigns {acc:?} exactly once (got {stores:?})"
    );
    let returns = find_returns(&hir);
    assert!(
        !returns.is_empty(),
        "precondition: the shape reads the accumulator at the tail"
    );
    for r in cell.stores.value_regions() {
        let dp = info
            .region_data
            .get(&r)
            .unwrap_or_else(|| panic!("stored region {r:?} has no decref point"))
            .decref_point;
        assert!(
            !returns.contains(&dp),
            "the stored value's release rode the returned-region extension out to \
             the `Return` (@{}): one release there names only the last iteration's \
             value, stranding every earlier one",
            dp.0
        );
        assert_eq!(
            dp, stores[0],
            "a cell-stored value's producer release is pinned to the store that \
             took it (region {r:?} landed at @{})",
            dp.0
        );
    }
}

/// The returned loop accumulator: a fn-local mutable a `while` reassigns, whose
/// final content the frame hands back. Shared by the two tests above so the
/// container-model admission and the release placement are read off one shape.
fn returned_loop_accumulator() -> (Hir, RegionInfo) {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@acc (%pair 0 0)]\n\
             (var i 0)\n\
             (while (%lt i n)\n\
               (begin (assign acc (%pair i 7))\n\
                      (assign i (%add i 1))))\n\
             acc)))\n\
         (h 3)",
    );
    (hir, info)
}

/// The accumulator binding of [`returned_loop_accumulator`] and its recorded
/// container. `acc` is the heap-carrying reassigned mutable — the immediate `i`
/// counter carries no region.
fn returned_accumulator_cell<'a>(
    hir: &Hir,
    info: &'a RegionInfo,
) -> (Binding, &'a crate::hir::region::CellContainer) {
    let acc = find_reassign_sites(hir)
        .into_iter()
        .map(|(_, b)| b)
        .find(|b| {
            info.binding_source_regions
                .get(b)
                .is_some_and(|rs| !rs.is_empty())
        })
        .expect("acc: the heap-carrying reassigned mutable");
    let cell = info.cell_containers.get(&acc).unwrap_or_else(|| {
        panic!("a returned fn-local reassigned mutable must record a container ({acc:?})")
    });
    (acc, cell)
}

/// Every `Return` node in the tree — the points `lower_return` mints at.
fn find_returns(hir: &Hir) -> Vec<HirId> {
    let mut out = Vec::new();
    fn walk(hir: &Hir, out: &mut Vec<HirId>) {
        if matches!(&hir.kind, HirKind::Return { .. }) {
            out.push(hir.id);
        }
        hir.for_each_child(|c| walk(c, out));
    }
    walk(hir, &mut out);
    out
}
