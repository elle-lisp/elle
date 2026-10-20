// audited: 2026-10-20
//! Pins a `Fresh` native's embed declaration: the embedded argument stays out of every Owned subtree, and the result stays a member.
//!
//! docs/impl/region/adopt.md
//! docs/impl/region/effects.md
//!
//! A `Fresh` native whose result EMBEDS an argument declares which args it embeds
//! (`PrimitiveDef::embeds`). The walk's `Fresh` arm then records `result ⊇ arg` in
//! `embed_edges` — the compile-time analog of the runtime alloc-scan
//! (`find_object_cross_refs`) that counts the same embedding. The forest counts the
//! edge against external uniqueness and builds no subtree from it. Without the edge
//! the forest cannot see a captured value flow OUT through an escaping result, so it
//! folds the value into the capturing closure's Owned subtree. With the edge read as a
//! containment edge, the argument joins the subtree of every container holding the
//! result, and the single-owner filter refuses the whole subtree. `with-traits` embeds
//! arg 1, the `traits` side-field (`embeds: &[1]`); `string` and `slice` embed arg 0,
//! the source their view lies over (`embeds: &[0]`).

use super::*;

/// The `(argument, result)` pairs of every embed edge whose argument is `arg`.
fn embeds_of(info: &RegionInfo, arg: Region) -> Vec<(Region, Region)> {
    info.embed_edges
        .iter()
        .filter(|&&(_, src, _)| src == arg)
        .map(|&(_, src, dst)| (src, dst))
        .collect()
}

/// Every `(member, owner)` adopt the store map emits.
fn store_adopts(edges: &ownership::AdoptEdges) -> Vec<(Region, Region)> {
    edges.store.values().flatten().copied().collect()
}

/// The single region captured by the lambda whose `alloc_region` is `closure` — its
/// sole capture's binding's sole source region. The embed-declaration probe's shape
/// has exactly one capture (the trait table); panics otherwise.
fn sole_captured_region(hir: &Hir, info: &RegionInfo, closure: Region) -> Region {
    fn walk(h: &Hir, info: &RegionInfo, closure: Region, out: &mut Vec<Region>) {
        if let HirKind::Lambda(lambda) = &h.kind {
            if info.alloc_region.get(&h.id) == Some(&closure) {
                for c in &lambda.captures {
                    if let Some(rs) = info.binding_source_regions.get(&c.binding) {
                        out.extend(rs.iter().copied().filter(|&r| r != closure));
                    }
                }
            }
        }
        h.for_each_child(|c| walk(c, info, closure, out));
    }
    let mut out = Vec::new();
    walk(hir, info, closure, &mut out);
    out.sort_by_key(|r| r.0);
    out.dedup();
    assert_eq!(
        out.len(),
        1,
        "shape must have exactly one captured region; got {out:?}",
    );
    out[0]
}

#[test]
fn with_traits_embed_refuses_adopt_of_captured_escaping_table() {
    // Fixture shape (tests/impl/region-traits-capture-adopt-uaf.lisp):
    // a closure `make` CAPTURES a struct `shared-tbl` and, in its body, attaches it as a
    // trait table with `with-traits`. The traited RESULT escapes `make` (returned from
    // its body) with its `traits` side-field still referencing the captured table.
    //
    // `with-traits` is a `Fresh` NATIVE that embeds arg 1 (the table) into the fresh
    // result's `traits` side-field — declared by `PrimitiveDef::embeds = &[1]`. The walk
    // records the embed edge `result ⊇ table`, so external uniqueness sees the
    // table referenced from OUTSIDE make's subtree (by the escaping result region) and
    // REFUSES to fold it in: the table stays Shared (per-region RC), reclaimed under the
    // live result's reference.
    //
    // The counter-factual: with no `result ⊇ table` edge the forest judges the captured
    // table externally unique to `make` and capture-adopts it, and make's subtree drop
    // frees it under the escaped result's `traits` field — a use-after-free, an
    // `UpdateCapture` fault under `--trace=guardfree`.
    let src = "(begin (let [shared-tbl {:type :my-type}] \
                        (let [make (fn (data) (with-traits [data] shared-tbl))] \
                          (make 1))) \
                      nil)";
    let (hir, info, edges) = adopt_edges(src);
    let make_r = sole_closure_region(&hir, &info);
    let tbl = sole_captured_region(&hir, &info, make_r);
    // Precondition: `make` genuinely captures the table (so absent the embed edge the
    // forest would fold it into make's Owned subtree — the counter-factual's premise).
    assert!(
        closure_captures_region(&hir, &info, tbl, make_r),
        "precondition: the closure r{} must capture the table r{}",
        make_r.0,
        tbl.0,
    );
    // The invariant: the captured table, embedded into an escaping result, is adopted by
    // NOBODY — it stays Shared (per-region RC). Asserted FIRST, so the counter-factual
    // (the table capture-adopted) fails here rather than on a downstream symptom.
    let adopts: Vec<(Region, Region)> = edges
        .store
        .values()
        .chain(edges.capture.values())
        .flatten()
        .copied()
        .collect();
    assert!(
        !adopts.iter().any(|&(m, _)| m == tbl),
        "the captured table r{} embedded into an escaping result must NOT be adopted \
         (it stays Shared) — got adopts {:?}",
        tbl.0,
        adopts,
    );
    let (_, _, owned) = owned_subtrees_with_effects(src);
    assert!(
        !in_some_owned_subtree(&owned, tbl),
        "the captured-and-embedded table r{} must be in no Owned subtree; owned={:?}",
        tbl.0,
        owned,
    );
    // The mechanism: the walk records the with-traits FRESH result ⊇ the table region
    // as an embed edge, and as nothing else.
    let &(embed_src, result) = embeds_of(&info, tbl).first().unwrap_or_else(|| {
        panic!(
            "with-traits (Fresh, embeds arg 1) must record `result ⊇ table` for the \
             captured table r{} in embed_edges; got {:?}",
            tbl.0, info.embed_edges,
        )
    });
    assert_eq!(embed_src, tbl, "the embed's contained member is the table");
    assert_ne!(
        result, tbl,
        "the embed's container is the with-traits result"
    );
    assert!(
        info.fresh_result_regions.contains(&result),
        "the embed container r{} is the with-traits FRESH result",
        result.0,
    );
    assert!(
        !info.containment_edges.iter().any(|&(_, src, _)| src == tbl),
        "an embed is not a containment edge; containment={:?}",
        info.containment_edges,
    );
}

#[test]
fn container_owning_an_embedding_result_keeps_the_adopt() {
    // `(string "s")` is the canonical fresh heap value of the forest tests, and `string`
    // embeds its arg 0 (the view it answers over immutable bytes). Pushed into `c`, the
    // result is an ordinary member of `c`'s subtree: the funnel edge gives it an owner,
    // and the literal it embeds is nobody's member, because an embed builds no subtree.
    //
    // The counter-factual: with the embed read as a containment edge, the literal joins
    // `c`'s subtree through the result, has no emittable owner edge, and refuses the
    // whole subtree to Shared — `c` loses its subtree drop over a value it plainly owns.
    // This assertion was RED while the walk pushed the embed into `containment_edges`
    // (`read_bounded_by_the_container_keeps_the_adopt` in `tests::borrow` fell with it).
    let src = "(let [c (@array) r (string \"s\")] (begin (%array-push c r) (length c)))";
    let (hir, info, edges) = adopt_edges(src);
    let lit = string_literal_region(&hir, &info, "s");
    let &(_, result) = embeds_of(&info, lit).first().unwrap_or_else(|| {
        panic!(
            "string (Fresh, embeds arg 0) must record `result ⊇ literal` for r{} in \
             embed_edges; got {:?}",
            lit.0, info.embed_edges,
        )
    });
    let adopts = store_adopts(&edges);
    assert!(
        adopts.iter().any(|&(m, _)| m == result),
        "the container must adopt the string result r{} it holds; got adopts {:?}",
        result.0,
        adopts,
    );
    assert!(
        !adopts.iter().any(|&(m, _)| m == lit),
        "the embedded literal r{} is adopted by nobody; got adopts {:?}",
        lit.0,
        adopts,
    );
    let (_, _, owned) = owned_subtrees_with_effects(src);
    assert!(
        !in_some_owned_subtree(&owned, lit),
        "the embedded literal r{} is in no Owned subtree; owned={:?}",
        lit.0,
        owned,
    );
}

#[test]
fn view_source_outside_the_container_stays_shared() {
    // The view shape itself: `v` is a `slice` over `s`, `c` owns `v`, and `s` outlives
    // `c`. The forest adopts `v` into `c` and leaves `s` Shared: `s` is its own
    // candidate root, and the embed edge `v ⊇ s` leaves its singleton set, so external
    // uniqueness refuses it. At `c`'s drop the cascade over `v`'s recorded edge
    // releases `s` once, and `s`'s own binding release frees it after the later read.
    //
    // The counter-factual: with the embed edge absent, `s` is an externally-unique
    // singleton on RC either way, so nothing changes here — the escaping twin in
    // tests/impl/region-view-owner-uaf.lisp is where the absent edge faults. With the
    // embed read as a containment edge, `s` joins `c`'s subtree and the whole subtree
    // refuses: the first assertion fails.
    let src = "(let [s \"abcdefgh\" c (@array) v (slice s 2 6)] \
                 (begin (%array-push c v) (length c) (length s)))";
    let (hir, info, edges) = adopt_edges(src);
    let source = string_literal_region(&hir, &info, "abcdefgh");
    let &(_, view) = embeds_of(&info, source).first().unwrap_or_else(|| {
        panic!(
            "slice (Fresh, embeds arg 0) must record `view ⊇ source` for r{} in \
             embed_edges; got {:?}",
            source.0, info.embed_edges,
        )
    });
    let adopts = store_adopts(&edges);
    assert!(
        adopts.iter().any(|&(m, _)| m == view),
        "the container must adopt the view r{} it holds; got adopts {:?}",
        view.0,
        adopts,
    );
    let (_, _, owned) = owned_subtrees_with_effects(src);
    assert!(
        !in_some_owned_subtree(&owned, source),
        "the view's source r{} is in no Owned subtree; owned={:?}",
        source.0,
        owned,
    );
}
