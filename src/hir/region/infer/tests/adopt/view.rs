// audited: 2026-10-20
//! Pins a view's embed: a container adopts the `string` or `slice` result it holds, and the source stays out of its subtree.
//!
//! docs/impl/region/adopt.md
//! docs/impl/region/effects.md

use super::*;

/// Every `(member, owner)` adopt the store map emits.
fn store_adopts(edges: &ownership::AdoptEdges) -> Vec<(Region, Region)> {
    edges.store.values().flatten().copied().collect()
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
    // tests/impl/region-view-owner-uaf.lisp is where the absent edge shows, as a
    // drop-time rescue of the wrongly adopted source. With the embed read as a
    // containment edge, `s` joins `c`'s subtree and the whole subtree refuses: the
    // first assertion fails.
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
