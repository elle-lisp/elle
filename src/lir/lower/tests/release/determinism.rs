// audited: 2026-09-29
//! The region analysis and the release order it drives are a pure function of the source.
//!
//! docs/impl/region/rules.md

use super::*;

#[test]
fn region_analysis_is_deterministic_across_compiles() {
    // The region analysis (decref points, buckets, memberships) must be a
    // pure function of the source, modulo the process-global HirId counter.
    // The counterfactual: a single-pass binding-chain
    // override (hash-ordered, read-while-write) would resolve a random prefix of each
    // binding chain per compile, yielding randomly-too-early decref points and a
    // cell release that lands before a read through it. The fixpoint
    // iteration makes the result unique; this pins it.
    fn snapshot(src: &str) -> String {
        let (lowerer, _hir) = make_lowerer(src);
        let info = &lowerer.region_info;
        // HirIds come from a process-global counter shared across threads, so
        // absolute ids — and even gaps between them — jitter run to run under
        // the parallel test harness. Normalize each id to its RANK among the
        // ids this snapshot mentions: structure survives, jitter doesn't.
        let mut ids: Vec<u32> = info
            .region_data
            .values()
            .map(|d| d.decref_point.0)
            .chain(info.alloc_region.keys().map(|h| h.0))
            .chain(lowerer.decrefs_by_decref_point.keys().map(|h| h.0))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let rank = |id: u32| ids.binary_search(&id).expect("id collected above") as u32;
        let mut rd: Vec<(u32, u32)> = info
            .region_data
            .iter()
            .map(|(r, d)| (r.0, rank(d.decref_point.0)))
            .collect();
        rd.sort();
        let mut ar: Vec<(u32, u32)> = info
            .alloc_region
            .iter()
            .map(|(h, r)| (rank(h.0), r.0))
            .collect();
        ar.sort();
        let mut cr: Vec<u32> = info.call_result_regions.iter().map(|r| r.0).collect();
        cr.sort();
        let mut buckets: Vec<(u32, Vec<u32>)> = lowerer
            .decrefs_by_decref_point
            .iter()
            .map(|(h, rs)| (rank(h.0), rs.iter().map(|r| r.0).collect()))
            .collect();
        buckets.sort();
        format!(
            "region_data: {rd:?}\nalloc_region: {ar:?}\ncall_result: {cr:?}\nbuckets: {buckets:?}\nxrefs: {:?}",
            info.cross_region_refs
        )
    }
    let first = snapshot(CAPTURE_CELL_SHAPE);
    for round in 0..8 {
        let again = snapshot(CAPTURE_CELL_SHAPE);
        assert_eq!(
            first, again,
            "round {round}: region analysis produced different results for \
             the same source — a hash-iteration order dependence",
        );
    }
}

#[test]
fn release_order_is_deterministic_across_compiles() {
    // Release order may never depend on hash-map iteration: the same source
    // must lower to the identical instruction stream on every compile
    // (docs/impl/region/rules.md Rule 4), up to the process-global static-region
    // counter (canonicalized away above). Two regions sharing a decref_point
    // are enough to expose a hash-ordered emission as a cross-compile diff.
    let first = canonicalize_static_regions(&format!("{:?}", compile_to_lir(CAPTURE_CELL_SHAPE)));
    for round in 0..8 {
        let again =
            canonicalize_static_regions(&format!("{:?}", compile_to_lir(CAPTURE_CELL_SHAPE)));
        assert_eq!(
            first, again,
            "round {round}: lowering the same source produced different \
             instruction streams — release order depends on hash iteration",
        );
    }
}
