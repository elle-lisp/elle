// ── ownership inference: adopt-edge emission (compute_adopt_edges, step 4) ──────
//
// `region::infer::ownership::compute_adopt_edges` is the map the lowerer consumes: for each
// externally-unique Owned subtree (the lifetime obligation + no merge overlap), it
// emits one `AdoptRegion(owner, member)` per non-root member. A `%`-store is an opaque
// `Funnel` native call recording NO `cross_region_refs` edge — its containment reaches
// the walk as site-keyed funnel-recovered `containment_edges`, and the store adopt is
// keyed at that funnel CALL site (region/adopt.md § The funnel adopt); a capture member
// is keyed at its closure's Lambda. Each member is adopted by its **actual parent**:
// the root when a direct `member → root` edge exists (a flat star, the common case — an
// interior member↔member cycle among root's direct children rides along, reclaimed by
// the root's subtree drop with no adopt of its own), else the single interior container
// that holds it (multi-level nesting `root ⊇ a ⊇ b`: `a` adopts `b`, the root adopts
// `a`, and the root's recursive subtree drop frees the whole chain). A member with no
// containment edge naming an owner, or with two-or-more non-root containers and no root
// edge (an ambiguous single owner), refuses the whole subtree to Shared (the
// always-legal baseline). These pins are written from that definition.
//
// One submodule per question the map has to answer:
//
// - `structural` — the lifetime obligation, decided by post-dominance over the scope
//   tree rather than by counting.
// - `subtrees` — which members a subtree contains: a `Fresh` native's embed
//   declaration, and store + capture + deep nesting together.
// - `captures` — what a closure may adopt: the suppress ⊆ adopt contract, the
//   capture-cell clique, and the re-storable-cell gate.
// - `cuts` — where a subtree stops: the activation-owner cut and the
//   transferred-returned-subtree cut.

// Re-glob the parent's test imports so each submodule can `use super::*;` and
// so `super::ownership` — the only `super::IDENT` a test body names — resolves
// one level down (see the note in `tests/mod.rs`).
use super::*;

mod captures;
mod cuts;
mod structural;
mod subtrees;

/// The region of the first (outermost) `Lambda` node in `hir` — the closure region the
/// combined-shape probes capture into. (Each probe has exactly one closure.)
fn sole_closure_region(hir: &Hir, info: &RegionInfo) -> Region {
    fn first_lambda(h: &Hir, out: &mut Option<HirId>) {
        if matches!(&h.kind, HirKind::Lambda { .. }) && out.is_none() {
            *out = Some(h.id);
        }
        h.for_each_child(|c| first_lambda(c, out));
    }
    let mut lam = None;
    first_lambda(hir, &mut lam);
    *info
        .alloc_region
        .get(&lam.expect("a Lambda node"))
        .expect("the closure has an alloc region")
}

/// Does the lambda whose `alloc_region` is `closure` capture a binding whose
/// source regions include `member` — i.e. can `lower_lambda_expr` reload the
/// member's value (from a slot or the env) for the value-resolved adopt?
fn closure_captures_region(hir: &Hir, info: &RegionInfo, member: Region, closure: Region) -> bool {
    fn walk(h: &Hir, info: &RegionInfo, member: Region, closure: Region, found: &mut bool) {
        if let HirKind::Lambda { captures, .. } = &h.kind {
            if info.alloc_region.get(&h.id) == Some(&closure)
                && captures.iter().any(|c| {
                    info.binding_source_regions
                        .get(&c.binding)
                        .is_some_and(|rs| rs.contains(&member))
                })
            {
                *found = true;
            }
        }
        h.for_each_child(|c| walk(c, info, member, closure, found));
    }
    let mut found = false;
    walk(hir, info, member, closure, &mut found);
    found
}
