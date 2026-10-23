# Adoption and subtree drop (the ownership forest)

<!-- audited: 2026-10-21 -->

Adoption links regions into a parent→child tree, so a whole subtree frees as a
unit when its root frees.

Merging *collapses* two regions onto one physical id; **adoption** keeps them
distinct but **links** them. Merging is the tight single-edge case: one child
stored once into one coincident-lifetime parent. Adoption is the general case, a
multi-region externally-unique component. That includes a mutable retaining
container and the values funnelled into it, a closure and its captures, and the
interior reference cycles the per-region RC cascade cannot collect
([rules.md](rules.md)).

`region::infer::ownership` classifies a region **Owned** (adopted, freed by
subtree drop) or **Shared** (the per-region RC baseline). The lowerer emits
`AdoptRegion{parent, child}` for each interior edge, and `FreeRegionGroup` for a
rootless co-owned cycle. Both ops run on the **interpreter and the JIT**. The
`elle_jit_adopt_region` / `elle_jit_free_region_group` helpers
([src/jit/dispatch/region.rs](../../../src/jit/dispatch/region.rs)) mirror the
interpreter's `handle_adopt_region` / `handle_free_region_group`, so a program
reclaims identically on either tier. The MLIR tier compiles no function that
carries them, and the WASM tier treats them as no-ops, since its arena boundary
reclaims. This document is the **runtime substrate** those emit modes drive: the
`RegionStore` primitives, pinned by `regionstore::tests::adopt` and the
cross-tier `runtime::tests::ownership` `*_under_jit` pins.

## The runtime: a reclamation typestate and `owned_children`

Each `RegionEntry` carries, besides its pool, a **reclamation mode** and an
`owned_children: Vec<RuntimeRegion>`. The mode is a typestate, not an
`(rc, owner)` pair: a region is either `Counted(u32)` — a Shared/baseline region
reclaimed by its own reference count — **or** `Owned(owner)` — reclaimed *only* by
`owner`'s subtree drop. The two variants are mutually exclusive. A region with a
live count *and* an owner, whose count could free what the owner will also
subtree-drop, is therefore *unrepresentable*: an `Owned` region has no `u32` to
decrement. Adoption *moves* the region from `Counted` into `Owned`,
**consuming** its count, and freeing *consumes* the entry. So a region is
reclaimed exactly one way, decided once.

- **`adopt_region(parent, child)`** transitions `child` from `Counted` to
  `Owned(parent)` — consuming its count — and pushes `child` into
  `parent.owned_children`. No incref: an interior ownership edge is **not**
  reference-counted (the subtree frees as a unit), the runtime twin of the lowerer
  suppressing the interior store edge's `IncrefRegion` ([merging.md](merging.md)).
  A region is adopted **at most once** — a second adoption
  would mean two owners and is a debug-asserted bug (the inference adopts each member
  once).
- A **`decref` of an `Owned` region is a no-op**. There is no count to
  decrement, so the no-op is *structural*, not a guard the decref path must
  remember. The interior containment edge is therefore safe to leave
  un-suppressed. When the owner frees, its cascade decrefs the interior child,
  finds an `Owned` region and no-ops, and the subtree drop frees the child. A
  missed interior-edge suppression costs nothing. A **store-adopted** member
  keeps its **own** compiler-emitted decref, unlike a capture-adopted member,
  whose decref is suppressed. That decref is a structural no-op **provided it
  fires while the member is still `Owned`**, **before** the root's subtree drop.
  The emit guarantees that order even when member and root share a
  `decref_point` node ([adopt.md](adopt.md)). The member's release is sorted
  ahead of the root's, so the root's later drop reclaims the member exactly once.
- **`reparent_owned_children(from, to)`** hands `from`'s whole direct
  `owned_children` set to `to`. Each child is re-stamped `Owned { owner: to }`,
  and the set is appended to `to`'s children. It is a **move**, never a copy, so
  the forest's forward/back edges stay consistent (the subtree-drop walk
  debug-asserts them) and no child gains a second owner. Neither endpoint's own reclaim mode changes and no count
  is created or consumed: the children were `Owned` and stay `Owned`; only the owner
  whose demise reclaims them changes. The cross-fiber cuts ride this
  ownership-**transfer** primitive. A parked activation's node passes to the
  fiber's at teardown, and a completing fiber's node to its consumer's, so one
  set-drop at the new owner's demise reclaims them all.
  A self-reparent, an absent `from`, or an empty child set is a no-op. Pinned by
  `regionstore::tests::reparent`.
- **Subtree drop, in phases.** Freeing a region (`free_runtime_region_pages`)
  runs five phases:
  1. collect the whole owned subtree, the region plus every transitive
     `owned_children`, walked Rust-side with no heap deref;
  2. **rescue** any member the recorded edge tables prove is still externally
     referenced (the rescue, below);
  3. read every remaining member's recorded `outgoing` table. A target *in* the
     freed set is interior and dropped, never cascaded. A target *outside* is a
     **Shared** frontier ref to cascade;
  4. return every member's pages, bumping each generation, so a stale pointer
     detonates at the next debug `region_of` ([generations.md](generations.md));
  5. cascade the collected Shared-frontier refs once.

  **No phase dereferences a heap page to discover an edge** — discovery is the recorded
  table, not a content walk; pages are touched only to tear them down. Interior cycles
  reclaim with the pages: the drop walks `owned_children`, not the reference graph, so a
  `(push a b)(push b a)` knot interior to one owned subtree frees with the subtree and
  never strands. Pinned by
  `regionstore::tests::forest::subtree_drop_cascades_shared_frontier_not_interior_cycle`.

## The outgoing edge table — reclamation without a heap scan

A dying region must release its references *into other regions*: the Shared
frontier the subtree or RC-zero free cascades. Walking the dying region's
contents to find them would be a heap scan. Instead, **every cross-region
reference is recorded at creation** into the source region's
`outgoing: FxHashMap<RuntimeRegion, u32>`, with per-reference counts. Every
`RegionEntry` carries one, `Owned` and `Counted` alike. Reclamation walks
`outgoing` (O(edges)), never the contents.

**What it records: content edges only.** The table mirrors exactly what the free-time
content scan (`find_object_cross_refs`) would find — a `Value` stored *into* another
region's heap object — and nothing else. A region's *incoming* RC count also
covers owner, return, argument, parameter and borrow references. That is a
separate ledger: compiler-emitted `DecrefValueRegion`/`DecrefRegion` balance
those increfs, so they are **not** edges. `outgoing` is the cascade's worklist, the
incoming count is the RC-zero trigger; the two are different sizes by design (the
`rc - in_degree` residual the `cross_ref_edges` diagnostic reports).

**Where it is recorded — the same sites that incref the containment edge:**

- **At allocation**, the creation funnel `incref_cross_region_refs`
  ([refcount.rs](../../../src/value/fiberheap/regionstore/refcount.rs)) scans a
  fresh object via `find_object_cross_refs` and increfs each cross-region
  referent. It records the matching `outgoing` edge in the *same loop*, so the
  alloc-path table is scan-equivalent **by construction**. This covers every object variant the scan
  covers — pair/array/struct/set contents, a closure's env + backing + template, a fiber's
  env + template, the `traits` side-field.
- **At a post-alloc mutable store**, the mutable-store seam
  ([mutate.rs](../../../src/value/arena/mutate.rs)) records the edge beside the
  RC incref/decref. It is the sole path a `Value` enters or leaves a live
  container on every tier. Push, insert, extend and add record; pop, remove,
  drain and del un-record. A replace
  (`set_at_with_rebind`/`struct_put_with_rebind`/`lbox_store_with_rebind`/`capture_store_with_rebind`)
  un-records the old target and records the new, mirroring the RC rebind.
- **At a fiber's terminal completion**, `incref_signal_region`
  ([refcount.rs](../../../src/vm/fiber/refcount.rs)) pins the result
  held in `fiber.signal`; that result is a content edge the scan's Fiber arm reads, so the
  same site records `outgoing[fiber-region] → result-region`. It is removed by the free-time
  walk when the fiber frees (a terminal fiber is read, never resumed, so there is no
  explicit un-record), matching the scan's asymmetric park-retain.

**Filter parity.** A recorded edge applies the filter `find_object_cross_refs`
applies. It skips a target of 0 or 1 (reserved, not real regions) and a
self-edge, so a same-region store records nothing, as the scan skips `own_id`. The scan's
region resolution is **ownership-verified**: a candidate id (read from the pointer's masked
page header) counts only when a live region of THIS store genuinely owns the pointer's
address (`RegionPool::owns`). A pointer the store does not manage reads whatever
bytes sit at the masked base, which can spell a local id. Such a pointer is a
foreign-heap value or a shared/Rc allocation. A compile-time-env constant in a
template's constant pool lives on the CompileCtx VM's heap, and a worker can
read a parent-heap value. Bare liveness is time-dependent
(dead at record time, live at scan time splits the two sides); ownership is time-invariant
for a foreign pointer, so record and scan agree by construction
(`regionstore::tests::edges::foreign_store_value_records_no_edge_and_frees_clean`).

**The debug equivalence oracle.** The content scan survives as a
`#[cfg(debug_assertions)]` oracle. At each free, before teardown, the drop
asserts that the recorded `outgoing` table, filtered to valid referents, is
multiset-equal to a one-time content scan. The scan dereferences target pages,
so it runs while every member's pages are still mapped. A missed mutation funnel
or a double-record becomes a deterministic panic at the free site. The panic
names the region and both edge sets, where release would leak (a missing edge)
or UAF (an extra edge).
In release builds only the table walk runs — O(edges), no scan. The diagnostic
`cross_ref_edges` (the `arena/region-edges` leak-graph) stays scan-based, so it remains an
*independent* check of the table rather than a mirror of it.

Classic Tofte–Talpin regions need no cascade, because a region points only at
longer-lived outer regions. Elle's hybrid adds one scan-needing edge class,
Owned/Shared → Shared, and records it eagerly.

## The incoming edge table and the external-reference rescue

Adoption's soundness condition is **external uniqueness**: an `Owned` member is
referenced only through its owning subtree, so the root's demise strands no live
reference. The compile-time walk (`region::infer::ownership::compute_owned_subtrees`)
proves that condition over the edges the solver can name; the runtime holds the
ground truth — every recorded content edge — and enforces the same condition **at
the drop itself**:

- **The incoming edge table.** Each `RegionEntry` mirrors its `outgoing` table with
  `incoming: FxHashMap<RuntimeRegion, u32>`: for every recorded content edge
  `src → dst`, `dst.incoming[src]` carries the same count. The sites that
  maintain `outgoing` maintain it too: `record_outgoing`, `unrecord_outgoing`,
  and the subtree drop's frontier walk, which removes a dying source's footprint
  from each live target. The two ledgers move in lockstep, and an unbalanced
  un-record debug-asserts on either side. Like `outgoing`, it records content edges only: the transfer/borrow
  references in the incoming RC count are balanced by compiler-emitted decrefs and
  are not edges.

- **The rescue.** A subtree drop first collects its member set read-only. Take a
  non-root member whose `incoming` table names a source that **survives the
  drop**, in neither the dying set nor the member's own subtree. That member is
  **rescued** instead of torn down: it leaves the forest (`Owned → Counted`) with a count
  rebuilt from its recorded incoming edges, and its own subtree stays intact
  beneath it. Dying sources are counted in (their frontier decrefs arrive in the
  same drop); the member's own subtree's back-edges are excluded (they release only
  at the member's own drop, so counting them would self-sustain the count). Every
  remaining referencer then releases the member through the ordinary cascade — the
  last release frees it. Rescue iterates to a fixpoint: a rescued member's
  surviving edges can make a sibling member externally referenced, which rescues
  the sibling too.

The rescue is the runtime's **refusal-to-Shared**, the always-legal baseline, at
the last moment the forest can choose it. A region whose external uniqueness
fails at the drop falls back to per-region RC instead of being freed under a
live reference, whatever adopt kind claimed it. It fires only when a live external edge exists
at the drop; the externally-unique common case pays one empty-map check per
member. Pinned by `regionstore::tests::rescue` and, end to end under guardfree, by
[region-capture-cell-member-cascade-uaf.lisp](../../../tests/impl/region-capture-cell-member-cascade-uaf.lisp):
a struct member stored into a module-level capture cell survives its parent's
subtree drop and frees at the cell's release.

## A rootless cycle frees at its scope

A store-only cycle with no container parent, such as `(push a b) (push b a)`,
has no member to root a subtree at. The co-owned group cut
(`region::infer::ownership::compute_owned_region_groups`) frees its members
together, with one `FreeRegionGroup` at the innermost structural scope that
encloses every member's allocation. The end of that scope post-dominates each
member's release inside it, and the release of each store's pass-through
result.

The end of the scope does not post-dominate a release outside it. When the
scope's value is a member, a consumer outside the scope reads that member after
the scope ends. That is the `let` that builds the cycle and answers `a`, handed
to `length`. The cut refuses such a group, because its free would run before
the read. The members stay on per-region RC, which strands the cycle and never
frees it under a reader. The drop site does not follow the value out, because
the free loads each member from its binding's slot, and the binding's scope is
the one that ended.

Pinned by
`region::infer::tests::owned::owned_region_groups_refuses_a_cycle_the_scope_value_carries_out`
and, under guardfree, by
[region-cycle-scope-value-uaf.lisp](../../../tests/impl/region-cycle-scope-value-uaf.lisp).
