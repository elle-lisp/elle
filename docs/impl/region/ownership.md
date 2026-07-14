# Adoption and subtree drop (the ownership forest)

Merging *collapses* two regions onto one physical id; **adoption** keeps them
distinct but **links** them into a parent→child ownership tree, so a whole subtree
frees as a unit when its root frees. Where merging is the tight single-edge case
(one child stored once into one coincident-lifetime parent), adoption is the
general case — a multi-region externally-unique component, including a mutable
retaining container and the values funnelled into it, a closure and its captures,
and the interior reference cycles that the per-region RC cascade cannot collect
(rules.md Rule 8). The compile-time analysis that classifies a region
**Owned** (adopted, freed by subtree drop) vs **Shared** (the per-region RC
baseline) is `regions::ownership`; the lowerer emits `AdoptRegion{parent, child}`
for each interior edge (and `FreeRegionGroup` for a rootless co-owned cycle)
behind `--region-ownership`. Both ops are realized on the **interpreter and the
JIT** — the `elle_jit_adopt_region` / `elle_jit_free_region_group` helpers
(`src/jit/dispatch/region.rs`) mirror the interpreter's `handle_adopt_region` /
`handle_free_region_group` line-for-line, so the same program reclaims identically
on either tier; only the MLIR/WASM realization trails (the flag forces those tiers
off until their structural-arena handling lands). This section is the **runtime
substrate** those emit modes drive — the `RegionStore` primitives, pinned by the
`regionstore::tests` adoption tests and the cross-tier `runtime::tests::ownership`
`*_under_jit` pins.

## The runtime: a reclamation typestate and `owned_children`

Each `RegionEntry` carries, besides its pool, a **reclamation mode** and an
`owned_children: Vec<RuntimeRegion>`. The mode is a typestate, not an
`(rc, owner)` pair: a region is either `Counted(u32)` — a Shared/baseline region
reclaimed by its own reference count — **or** `Owned(owner)` — reclaimed *only* by
`owner`'s subtree drop. The two are mutually exclusive variants, so **owned-and-RC'd**
— a region carrying both a live count *and* an owner, where the count could
independently free a region the owner will also subtree-drop (a double-free) — is
*unrepresentable by construction*: an `Owned` region has no `u32` to decrement at
all. This is the move-only encoding of the ownership right — adoption *moves* the
region from `Counted` into `Owned`, **consuming** its count, and freeing *consumes*
the entry — so a region is reclaimed exactly one way, decided once and thereafter
not expressible the other way.

- **`adopt_region(parent, child)`** transitions `child` from `Counted` to
  `Owned(parent)` — consuming its count — and pushes `child` into
  `parent.owned_children`. No incref: an interior ownership edge is **not**
  reference-counted (the subtree frees as a unit), the runtime twin of the lowerer
  suppressing the interior store edge's `IncrefRegion` (the self-edge elimination of
  merging.md § Merging, generalized). A region is adopted **at most once** — a second adoption
  would mean two owners and is a debug-asserted bug (the inference adopts each member
  once).
- A **`decref` of an `Owned` region is a no-op** — there is no count to decrement, so
  the no-op is *structural*, not a guard the decref path must remember to apply. This
  is what makes the interior containment edge safe to leave un-suppressed at runtime:
  when the owner frees, its free-time cascade scans its contents and decrefs the
  interior child, but that decref finds an `Owned` region and no-ops, and the subtree
  drop below frees it explicitly. (So, as with mint-or-reuse, a missed interior-edge
  suppression costs nothing — the `Owned` mode absorbs it.) A **store-adopted** member
  keeps its **own** compiler-emitted decref too (unlike a capture-adopted member, whose
  decref is suppressed); that decref is likewise a structural no-op **provided it fires
  while the member is still `Owned`** — i.e. **before** the root's subtree drop. The
  emit guarantees that ordering even when member and root share a `decref_point` node
  (adopt.md § "The lifetime obligation the root carries"): the member's release is sorted ahead
  of the root's, so it lands on the frozen `Owned` region and no-ops, and the root's
  later drop reclaims the member exactly once.
- **`reparent_owned_children(from, to)`** hands `from`'s whole direct
  `owned_children` set to `to`: each child is re-stamped `Owned { owner: to }` and the
  set is appended to `to`'s children — a **move**, never a copy, so the forest's
  forward/back edges stay consistent (the subtree-drop walk debug-asserts them) and no
  child gains a second owner. Neither endpoint's own reclaim mode changes and no count
  is created or consumed: the children were `Owned` and stay `Owned`; only the owner
  whose demise reclaims them changes. This is the ownership-**transfer** primitive the
  cross-fiber cuts ride — a set of regions owned by one node is handed wholesale to
  another (a parked activation's node to the fiber's at teardown; a completing fiber's
  node to its consumer's) so one set-drop at the new owner's demise reclaims them all.
  A self-reparent, an absent `from`, or an empty child set is a no-op. Pinned by
  `regionstore::tests::forest::reparent_*`.
- **Subtree drop, in phases.** Freeing a region (`free_runtime_region_pages`)
  collects the whole owned subtree — the region plus every transitive
  `owned_children`, walked Rust-side with no heap deref — **rescues** any member the
  recorded edge tables prove is still externally referenced (§ The incoming edge
  table and the external-reference rescue), then **reads every remaining member's
  recorded `outgoing` edge table** (§ The outgoing edge table) and partitions its
  targets: a target *in* the freed set is interior and dropped (reclaimed by this drop,
  never cascaded), a target *outside* is a genuinely-**Shared** frontier ref to cascade;
  only then returns every member's pages, bumping each generation (a stale pointer into
  them detonates at the next debug `region_of`, exactly as for an ordinary free;
  generations.md); and finally cascades the collected Shared-frontier refs once.
  **No phase dereferences a heap page to discover an edge** — discovery is the recorded
  table, not a content walk; pages are touched only to tear them down. Interior cycles
  reclaim with the pages: the drop walks `owned_children`, not the reference graph, so a
  `(push a b)(push b a)` knot interior to one owned subtree frees with the subtree and
  never strands. Pinned by
  `regionstore::tests::subtree_drop_cascades_shared_frontier_not_interior_cycle`.

## The outgoing edge table — reclamation without a heap scan

A dying region must release its references *into other regions* (the Shared frontier the
subtree / RC-zero free cascades). Discovering those by walking the dying region's contents
at free is a heap scan; instead **every cross-region reference is recorded at creation**
into the source region's `outgoing: FxHashMap<RuntimeRegion, u32>` — per-reference counts,
universal (on every `RegionEntry`, `Owned` and `Counted` alike: an Owned region carries
`outgoing` for its cascade-on-drop but no count). Reclamation walks `outgoing` (O(edges)),
never the contents — what makes "no heap walk to reclaim" literally true.

**What it records: content edges only.** The table mirrors exactly what the free-time
content scan (`find_object_cross_refs`) would find — a `Value` stored *into* another
region's heap object — and nothing else. The larger *incoming* RC count a region carries
(owner, return / argument / parameter transfer, borrow references) is a separate ledger;
those increfs are balanced by compiler-emitted `DecrefValueRegion`/`DecrefRegion`, not by
the free-time cascade, so they are **not** edges. `outgoing` is the cascade's worklist, the
incoming count is the RC-zero trigger; the two are different sizes by design (the
`rc - in_degree` residual the `cross_ref_edges` diagnostic reports).

**Where it is recorded — the same sites that incref the containment edge:**

- **At allocation**, the creation funnel `incref_cross_region_refs`
  (regionstore/refcount.rs) already scans a freshly-allocated object via
  `find_object_cross_refs` and increfs each cross-region referent; it records the matching
  `outgoing` edge in the *same loop*, so the alloc-path table is scan-equivalent **by
  construction** (one function feeds both). This covers every object variant the scan
  covers — pair/array/struct/set contents, a closure's env + backing + template, a fiber's
  env + template, the `traits` side-field.
- **At a post-alloc mutable store**, the mutable-store seam (`value/arena/mutate.rs`, the
  sole path a `Value` enters or leaves a live container on every tier — interpreter, JIT,
  WASM) records the edge co-located with the RC incref/decref: push / insert / extend / add
  record, pop / remove / drain / del un-record, a replace
  (`set_at`/`struct_put`/`lbox_store`/`capture_store`) un-records the old target and records
  the new — exactly mirroring the RC rebind.
- **At a fiber's terminal completion**, `incref_signal_region` (vm/fiber.rs) pins the result
  held in `fiber.signal`; that result is a content edge the scan's Fiber arm reads, so the
  same site records `outgoing[fiber-region] → result-region`. It is removed by the free-time
  walk when the fiber frees (a terminal fiber is read, never resumed, so there is no
  explicit un-record), matching the scan's asymmetric park-retain.

**Filter parity.** A recorded edge applies the same filter `find_object_cross_refs` applies
— skip a target of 0 or 1 (reserved, not real regions) and a self-edge (`target == source`)
— so a same-region store records nothing, exactly as the scan skips `own_id`. The scan's
region resolution is **ownership-verified**: a candidate id (read from the pointer's masked
page header) counts only when a live region of THIS store genuinely owns the pointer's
address (`RegionPool::owns`). A pointer the store does not manage — a foreign-heap value
(a compile-time-env constant baked into a template's constant pool lives on the CompileCtx
VM's heap; a worker reading a parent-heap value) or a shared/Rc allocation — reads whatever
bytes sit at the masked base, which can spell a local id. Bare liveness is time-dependent
(dead at record time, live at scan time splits the two sides); ownership is time-invariant
for a foreign pointer, so record and scan agree by construction
(`regionstore::tests::edges::foreign_store_value_records_no_edge_and_frees_clean`).

**The debug equivalence oracle.** The content scan is *not* deleted: it is demoted to a
`#[cfg(debug_assertions)]` oracle. At each free, before teardown — while every member's
pages are still mapped (the load-bearing ordering: the scan dereferences target pages to
classify them) — the drop asserts the recorded `outgoing` table (filtered to currently-valid
referents) is multiset-equal to a one-time content scan. Any accounting drift — a missed
mutation funnel, a double-record — becomes a deterministic panic at the free site, naming the
region and both edge sets, instead of a silent leak (a missing edge) or UAF (an extra edge).
In release builds only the table walk runs — O(edges), no scan. The diagnostic
`cross_ref_edges` (the `arena/region-edges` leak-graph) stays scan-based, so it remains an
*independent* check of the table rather than a mirror of it.

Research footing: classic Tofte–Talpin regions need no cascade at all (a region only points
at longer-lived outer regions, so freeing it dereferences nothing). Elle's hybrid adds exactly
one scan-needing edge class — Owned/Shared → Shared — and recording it eagerly eliminates the
lazy free-time discovery.

## The incoming edge table and the external-reference rescue

Adoption's soundness condition is **external uniqueness**: an `Owned` member is
referenced only through its owning subtree, so the root's demise strands no live
reference. The compile-time walk (`regions::ownership::compute_owned_subtrees`)
proves that condition over the edges the solver can name; the runtime holds the
ground truth — every recorded content edge — and enforces the same condition **at
the drop itself**:

- **The incoming edge table.** Each `RegionEntry` mirrors its `outgoing` table with
  `incoming: FxHashMap<RuntimeRegion, u32>`: for every recorded content edge
  `src → dst`, `dst.incoming[src]` carries the same count. It is maintained by
  exactly the sites that maintain `outgoing` — `record_outgoing` /
  `unrecord_outgoing`, plus the subtree drop's frontier walk (which removes a dying
  source's footprint from each live target it referenced) — so the two ledgers move
  in lockstep; an unbalanced un-record debug-asserts exactly as the outgoing side
  does. Like `outgoing`, it records content edges only: the transfer/borrow
  references in the incoming RC count are balanced by compiler-emitted decrefs and
  are not edges.

- **The rescue.** A subtree drop first collects its member set read-only. A
  non-root member whose `incoming` table names a source that **survives the drop**
  — neither in the dying set nor inside the member's own subtree — is **rescued**
  instead of torn down: it leaves the forest (`Owned → Counted`) with a count
  rebuilt from its recorded incoming edges, and its own subtree stays intact
  beneath it. Dying sources are counted in (their frontier decrefs arrive in the
  same drop); the member's own subtree's back-edges are excluded (they release only
  at the member's own drop, so counting them would self-sustain the count). Every
  remaining referencer then releases the member through the ordinary cascade — the
  last release frees it. Rescue iterates to a fixpoint: a rescued member's
  surviving edges can make a sibling member externally referenced, which rescues
  the sibling too.

The rescue is the runtime's **refusal-to-Shared** (the always-legal baseline),
applied at the last moment the forest can still choose it: a region whose external
uniqueness does not hold at the drop — however the external reference arose, and
whichever adopt kind claimed the region — falls back to per-region RC instead of
being freed under a live reference. It fires only when a live external edge exists
at the drop; the externally-unique common case pays one empty-map check per
member. Pinned by `regionstore::tests::forest` (the rescue unit family) and, end
to end, by the guardfree fixture pin `region_capture_cell_member_cascade_uaf`
(tests/integration/elle_scripts.rs): a struct member stored into a module-level
capture cell survives its parent's subtree drop and frees at the cell's release.

