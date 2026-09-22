// audited: 2026-09-22
//! What a whole-value read of a 1-slot container takes, through a branch, a begin wrapper, and an uncelled cell.
//!
//! docs/impl/region/bindings.md

use super::*;

/// A whole-value read of an UNCELLED 1-slot container takes a counted reference,
/// exactly as a read of the celled realization does (docs/impl/region/reads.md
/// § "A whole-value read of a 1-slot container takes a counted reference"). The
/// container releases what it held at every overwrite — here the compiler's own
/// drop-on-overwrite rather than `capture_store_with_rebind` — so `keep` borrows
/// a reference that dies at the first `(assign last …)`, and the release the
/// borrow needs is one no other name can supply.
///
/// The reference `keep` takes is its own, so `keep` is no longer a holder of the
/// init region and the cell donates its init as an unaliased one would. That is
/// what makes the shape reclaimable at all: the counted-init route would leave
/// the producer's reference to be released through the slot recorded for the init
/// region, which here is the CELL's own — a mutated slot, and no release route.
///
/// The discriminator is the ordering. `reassign_gate_counts_an_aliased_init`
/// above is the same program with the alias bound FIRST, so the alias allocates,
/// its own untainted slot carries the release, and the counted-init route runs
/// instead.
#[test]
fn reassign_gate_counts_a_read_of_an_uncelled_cell() {
    let (hir, arena, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@last (array 0 0)]\n\
             (let [keep last]\n\
               (var i 0)\n\
               (while (%lt i n)\n\
                 (begin (assign last (array i 7))\n\
                        (assign i (%add i 1))))\n\
               (%length keep)))))\n\
         (h 3)",
    );
    let (last, last_sites) = heap_carrying_reassign(&hir, &info);
    assert!(
        !arena.get(last).is_restorable_capture_cell(),
        "precondition: the container must be UNCELLED — no closure captures it, \
         so its content is re-stored by the compiler's drop-on-overwrite"
    );
    assert!(
        !info.counted_cell_read_sites.is_empty(),
        "a whole-value read of a 1-slot container takes a counted reference \
         whether or not the container is celled"
    );
    // Every counted read is value-resolved: the placeholder rides
    // `call_result_regions`, so the reader releases through its OWN slot rather
    // than inheriting the container's static source region.
    for site in &info.counted_cell_read_sites {
        let r = info
            .alloc_region
            .get(site)
            .expect("counted read site mints a placeholder region");
        assert!(
            info.call_result_regions.contains(r),
            "counted-read placeholder {r:?} must be a call-result region",
        );
    }
    assert!(
        info.cell_containers.contains_key(&last),
        "the container model still runs — the counted read changes who holds the \
         init, not whether the cell is a container"
    );
    assert!(
        last_sites
            .iter()
            .any(|site| info.drop_on_overwrite_sites.contains(site)),
        "drop-on-overwrite is the release of the reference the cell was donated"
    );
    assert!(
        info.counted_cell_init_sites.is_empty(),
        "with the reader counted the cell is the init's sole holder, so the \
         donation is available and no init retain is needed (got {:?})",
        info.counted_cell_init_sites,
    );
    let regs = &info.binding_source_regions[&last];
    assert!(
        regs.iter()
            .any(|r| info.suppressed_decref_regions.contains(r)),
        "the donated init region's ordinary decref is suppressed — its release is \
         the cell's drop-on-overwrite (regs={regs:?})",
    );
}

/// A BRANCH whose every arm is a whole-value read of a 1-slot container is a
/// whole-value read (docs/impl/region/reads.md § "A branch is a read of
/// whichever arms read"). What obliges the reader is the value it holds, not
/// the syntax that selected it: `keep` names, on every path, a borrow out of a
/// container that re-stores, and one `IncrefValueRegion` at the binder covers
/// every arm because it names the runtime value.
///
/// The discriminator against
/// `reassign_gate_counts_a_read_of_an_uncelled_cell` is the branch alone — the
/// same program with the alias's init wrapped in an `if` both of whose arms read
/// the container. Reading the branch as an ordinary alias instead leaves `keep` a
/// holder of the init region, so the container falls back to the counted-init
/// route, whose release routes through the CELL's own slot — a mutated slot, and
/// no release route, so the init strands per call
/// (`tests/elle/region-cell-alias-branch.lisp`).
#[test]
fn reassign_gate_counts_a_branch_read_of_a_container() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@last (array 0 0)]\n\
             (let [keep (if (%lt n 0) last last)]\n\
               (var i 0)\n\
               (while (%lt i n)\n\
                 (begin (assign last (array i 7))\n\
                        (assign i (%add i 1))))\n\
               (%length keep)))))\n\
         (h 3)",
    );
    let (last, last_sites) = heap_carrying_reassign(&hir, &info);
    assert!(
        !info.counted_cell_read_sites.is_empty(),
        "a branch every arm of which reads the container is a whole-value read"
    );
    // Value-resolved exactly as a bare read is: the placeholder rides
    // `call_result_regions`, so the reader releases through its OWN slot.
    for site in &info.counted_cell_read_sites {
        let r = info
            .alloc_region
            .get(site)
            .expect("counted read site mints a placeholder region");
        assert!(
            info.call_result_regions.contains(r),
            "counted-read placeholder {r:?} must be a call-result region",
        );
    }
    assert!(
        last_sites
            .iter()
            .any(|site| info.drop_on_overwrite_sites.contains(site)),
        "the container keeps the model — drop-on-overwrite releases the \
         reference it was donated"
    );
    assert!(
        info.counted_cell_init_sites.is_empty(),
        "with the branch counted the container is its init's sole holder, so the \
         donation is available and no init retain is needed (got {:?})",
        info.counted_cell_init_sites,
    );
    let regs = &info.binding_source_regions[&last];
    assert!(
        regs.iter()
            .any(|r| info.suppressed_decref_regions.contains(r)),
        "the donated init region's ordinary decref is suppressed (regs={regs:?})",
    );
}

/// A MIXED branch — one arm reading the container, one allocating — takes the
/// counted read too, and pays for the allocating arm by KEEPING that arm's source
/// regions (docs/impl/region/reads.md § "A branch is a read of whichever arms
/// read"). The replacement is per-arm: the reader stops holding the container's
/// regions, so the donation runs, while the allocating arm's region stays in the
/// reader's set — it is the only thing extending that value's last use out to the
/// binder's retain.
#[test]
fn reassign_gate_counts_a_mixed_branch_init() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@last (array 0 0)]\n\
             (let [keep (if (%lt n 0) last (array 5 5))]\n\
               (var i 0)\n\
               (while (%lt i n)\n\
                 (begin (assign last (array i 7))\n\
                        (assign i (%add i 1))))\n\
               (%length keep)))))\n\
         (h 3)",
    );
    let (last, last_sites) = heap_carrying_reassign(&hir, &info);
    assert_eq!(
        info.counted_cell_read_sites.len(),
        1,
        "the arm that reads the container makes the branch a counted read \
         (got {:?})",
        info.counted_cell_read_sites,
    );
    let site = *info.counted_cell_read_sites.iter().next().unwrap();
    let placeholder = *info
        .alloc_region
        .get(&site)
        .expect("counted read site mints a placeholder region");
    assert!(
        info.call_result_regions.contains(&placeholder),
        "counted-read placeholder {placeholder:?} must be a call-result region",
    );
    // The reader: the one binding the placeholder was minted for.
    let reader = *info
        .binding_source_regions
        .iter()
        .find(|(_, rs)| rs.contains(&placeholder))
        .expect("the placeholder is the reader's source region")
        .0;
    let reader_regs = &info.binding_source_regions[&reader];
    let last_regs = &info.binding_source_regions[&last];
    assert!(
        reader_regs.iter().any(|r| *r != placeholder),
        "the allocating arm's region stays in the reader's source set — cutting \
         it would put that arm's own release ahead of the binder's retain \
         (regs={reader_regs:?})",
    );
    assert!(
        !reader_regs.iter().any(|r| last_regs.contains(r)),
        "the container's regions are withdrawn from the reader, which is what \
         hands the donation back (reader={reader_regs:?}, container={last_regs:?})",
    );
    assert!(
        last_sites
            .iter()
            .any(|site| info.drop_on_overwrite_sites.contains(site)),
        "the container keeps the model — drop-on-overwrite releases the \
         reference it was donated"
    );
    assert!(
        info.counted_cell_init_sites.is_empty(),
        "with the reading arm counted the container is its init's sole holder, \
         so the donation is available and no init retain is needed (got {:?})",
        info.counted_cell_init_sites,
    );
    assert!(
        last_regs
            .iter()
            .any(|r| info.suppressed_decref_regions.contains(r)),
        "the donated init region's ordinary decref is suppressed \
         (regs={last_regs:?})",
    );
}

/// A statement wrapper around the read is descended too: a `Begin`'s value is
/// its tail's, so the reader ends up holding what the tail read and takes the
/// same counted reference (docs/impl/region/reads.md § "A branch is a read of
/// whichever arms read").
#[test]
fn reassign_gate_counts_a_begin_wrapped_read() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@last (array 0 0)]\n\
             (let [keep (begin 1 last)]\n\
               (var i 0)\n\
               (while (%lt i n)\n\
                 (begin (assign last (array i 7))\n\
                        (assign i (%add i 1))))\n\
               (%length keep)))))\n\
         (h 3)",
    );
    let (last, _) = heap_carrying_reassign(&hir, &info);
    assert_eq!(
        info.counted_cell_read_sites.len(),
        1,
        "the begin's tail is the read the binder counts (got {:?})",
        info.counted_cell_read_sites,
    );
    assert!(
        info.counted_cell_init_sites.is_empty(),
        "with the read counted the container is its init's sole holder, so the \
         donation is available (got {:?})",
        info.counted_cell_init_sites,
    );
    let last_regs = &info.binding_source_regions[&last];
    assert!(
        last_regs
            .iter()
            .any(|r| info.suppressed_decref_regions.contains(r)),
        "the donated init region's ordinary decref is suppressed \
         (regs={last_regs:?})",
    );
}

/// Counterfactual against over-admission: a branch NO arm of which reads a
/// container is a read of nothing, so the reader keeps every source region it
/// had and the container falls back to counting its init. Nothing here obliges a
/// retain — neither arm's value can be freed by the container's next overwrite.
#[test]
fn reassign_gate_declines_a_branch_reading_no_container() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@last (array 0 0)]\n\
             (let [other (array 1 1)]\n\
               (let [keep (if (%lt n 0) other (array 5 5))]\n\
                 (var i 0)\n\
                 (while (%lt i n)\n\
                   (begin (assign last (array i 7))\n\
                          (assign i (%add i 1))))\n\
                 (%add (%length keep) (%length last)))))))\n\
         (h 3)",
    );
    let (_, last_sites) = heap_carrying_reassign(&hir, &info);
    assert!(
        info.counted_cell_read_sites.is_empty(),
        "neither arm reads a 1-slot container, so there is nothing to count \
         (got {:?})",
        info.counted_cell_read_sites,
    );
    assert!(
        last_sites
            .iter()
            .any(|site| info.drop_on_overwrite_sites.contains(site)),
        "the container still takes the model — what the branch decides is who \
         holds the init, not whether the container is a container"
    );
}
