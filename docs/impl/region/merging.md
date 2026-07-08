# Merging

Merging collapses two solver `Region`s onto **one** physical region: both
allocations land in the same pages, and a single `DecrefRegion` frees them
together. It is the one mechanism allowed to break "one region per value"
(rules.md § "There are exactly two measures") — sound only when the merged
values share a lifetime, which the predicate below pins.

The first merge is the **builder-idiom seed**: a freshly-built child aggregate
merged into the **parent aggregate it is stored into**. The canonical shape is a
nested `%pair` — `(%pair (%pair 1 2) 3)` — where the inner pair is the car of the
outer. It is a down-payment on the forest's owned-subtree drop
(ownership.md § "Adoption and subtree drop"): a fully-fresh nested literal collapses to one
region, every car/cdr edge becomes intra-region, and the whole structure frees as
a unit. This is **not** sibling page-amortization (two values with no edge between
them) — that is a separate, later rider.

## The seed predicate

A child region `c` merges into a parent region `p` exactly when an immutable
aggregate-store edge `(site, c, p)` in `cross_region_refs` satisfies all of:

1. **It is a `%pair` car/cdr store.** `site` is a `Pair` intrinsic node and
   `alloc_region[site] == p` — the edge's target is the aggregate freshly
   allocated *at that very site*. This is what distinguishes the immutable
   aggregate store (whose sole compile-time `IncrefRegion` is balanced by the
   aggregate's free-time cascade) from the cases that must **never** seed a merge:
   a may-store **clique** edge (`hard_edge_sites` — runtime-counted by the
   target's content scan), a `SetCell` edge (its target is the cell's binding
   region, not an alloc at the site), and a mutable/conditional `%put`/
   `%array-push` edge (its target is the collection arg, and its own
   `alloc_region[site]` is a call-result placeholder, not the target).
2. **`p` is a fresh local immutable aggregate.** `p` is `live` and is none of the
   dynamic classes — `call_result_regions`, `cell_release_regions`,
   `suppressed_decref_regions`, `mutated_binding_value_regions`.
3. **`c` is a fresh local allocation.** Same exclusions as `p`. A call-result or
   capture-cell child is a runtime fact, not statically nameable.
4. **`c` is stored only into `p`.** Every `cross_region_refs` edge whose source is
   `c` targets `p`. A child stored into a *second* aggregate is aliased and must
   keep independent accounting (the repeated `(%pair x x)` into the *same* parent
   is fine — both references live and die with `p`; two *different* targets is
   not).
5. **Neither `c` nor `p` escapes.** Neither is in the return frontier (escape's
   return facet projected to regions — not returned), and every user binding that
   holds it (`binding_source_regions`) neither escapes via return
   (`EscapeInfo::binding_escapes_via_return`) nor is
   captured by a closure (`is_captured`). A returned/captured *child* outlives the
   parent's free. A returned/captured *parent* is sound to merge too (the child
   still dies within it — the owned-subtree drop), but it is the deferred widening:
   the seed starts narrow with the parent as a genuine **local** owner (the
   discarded / together-consumed nested literal) and widens cut by cut, the way the
   ownership inference does.
6. **`p`'s free post-dominates `c`'s last use.** The merged region's single
   `DecrefRegion`, at `region_data[p].decref_point`, must not precede `c`'s own last
   *direct* use. Decided **structurally** over the scope tree
   (`regions::postdom::drop_post_dominates`, `EmitMode::Merge`), not by `compute_order`
   magnitude (adopt.md § "The lifetime obligation the root carries").
   Because `c` is `p`'s car/cdr stored *only* into it (conditions 1+4), containment pins
   `c`'s lifetime to `p`'s, so the loop-enclosure clause is **waived** — an in-loop nested
   literal still merges. ANF binds the parent's own value to a temp, so `p`'s
   `decref_point` can land one node *past* `c`'s last use (the store into `p`); the
   predicate's straight-line case admits that (`c` sequenced before `p` with no control
   node between them). A child *read after* the parent's death (an alias read past the
   build; the mutable-accumulator `(assign acc (%pair i acc))` lifetime in miniature) is
   sequenced after `p`'s free and is refused. The old `ord(c) <= ord(p)` compare survives
   only as a `#[cfg(debug_assertions)]` shadow.

The merge is recorded as a `child → parent` forest (`RegionInfo::merged_parent`;
`merged_root` follows the chain, so a three-deep nest `(%pair (%pair (%pair …)))`
collapses every level onto the outermost region — the **root**). A region has at
most one merge parent (condition 4), so the relation is a forest, never a cycle.

## Emission: one slot per merge tree, one demise at the root

Every region in a merge tree shares **one** static slot — the root's. The lowerer's
`static_slot` canonicalizes a region through `merged_root` before minting or
looking up its slot, so a child's allocation, its `IncrefRegion`s, and its
`DecrefRegion` all name the root's slot. Two consequences fall out by
construction:

- **The child carries no demise of its own.** `emit_decrefs_for` skips a non-root
  merge child (`merged_root(r) != r`); only the root region's single
  `DecrefRegion`, at the root's (the outer aggregate's) `decref_point`, survives.
  That point post-dominates every member allocation (condition 6 — a child never
  outlives its parent), so the one drop frees the whole merged region after its
  last member is built. Emitting the child's own drop — which after
  canonicalization names the same root slot — would free the shared region at the
  child's earlier `decref_point`, under the still-live parent (a use-after-free).
- **The merged `child → parent` store edge is dropped.** Once both endpoints
  resolve to one slot the edge is an intra-region `R → R` self-edge whose
  `IncrefRegion(R)` the free-time cascade never balances (it skips self-references;
  mechanism.md § "Self-edge elimination"). `emit_increfs_for` drops it. A merge
  *without* this drop leaks `R`; a child drop *without* the merge frees early —
  the two move together, which is why allocation-canonicalization, child-decref
  suppression, and self-edge elimination are one mechanism, not three.

The whole mechanism is keyed on `RegionInfo::merged_parent` being non-empty. Under
`--checked-intrinsics=on` (the CLI default) `%pair` lowers as a native call, not a
`Pair` intrinsic node, so the seed predicate finds no sites, `merged_parent` is
empty, `merged_root` is the identity, and every step above is inert — the emitted
stream is byte-identical to the one-region-per-value baseline. The merge fires only
where `%pair` survives as an intrinsic (`--checked-intrinsics=off`).

## Runtime: the per-execution slot model and mint-or-reuse

The hazard merging must resolve is the **per-execution slot model** (model.md § "The per-execution region model"): two alloc instructions (child, then parent) stamped
with one shared static slot would each `runtime_region_for_alloc_slot`-mint a
fresh physical region and overwrite the activation mapping — orphaning the child's
region (the shared-slot leak class). The seed resolves it exactly as the model's
"merging is the feature that makes a slot resolve to a *shared* physical region"
note anticipates:

- the lowerer records a per-function **`merged_slots`** set — the root slots a
  merge tree's allocations share (`record_merged_slots`, keyed through
  `merged_root`), threaded into the executing `Code`/`ClosureTemplate` and, for the
  top-level entry, through `Bytecode.merged_slots`;
- `runtime_region_for_alloc_slot_maybe_merged` **mints** for a unique slot
  (byte-identical to the unmerged baseline) and **mint-or-reuses** for a merged
  slot — the child's alloc, executing first, mints `R` and records `slot → R`; the
  parent's alloc, finding the slot already mapped, **reuses** `R`. Both land in
  `R`; the single `DecrefRegion` at the root's `decref_point` frees both. All three
  tiers honour it: the interpreter alloc handlers, the JIT alloc helper (a merged
  slot routes to `elle_jit_resolve_alloc_region_merged`, selected at compile time
  from `LirFunction.merged_slots`), and a cross-thread-sent closure (whose
  `merged_slots` rides the `SendableClosure`).

Per-iteration uniqueness in loops is preserved because that single `DecrefRegion`
clears the slot (`take_runtime_region_for_drop_slot`) each iteration, so the next
iteration's child mints fresh. The lowerer asserts, per merged slot, that exactly
one `DecrefRegion` names it (`record_merged_slots`'s decref-dominance check) — a
merge it cannot prove single-demised is never recorded, so the unmerged baseline
(always legal) stands.

Mint-or-reuse is what keeps the merge tree in **one** physical region; it is not
what makes it leak-free. Even a tier that minted fresh for every member (a child
`R_c`, a parent `R_p`) would not leak the child: the parent object references the
child, so the parent's free-time cascade decrefs `R_c` to zero — exactly the edge
the self-edge elimination removed at compile time is supplied at runtime by the
cascade across `R_p → R_c`. So a wiring gap in one tier costs region *count*
(VM/JIT divergence), never correctness. The win mint-or-reuse buys is the
collapse to one region: fewer page mints, better locality — the down-payment on
the forest's owned-subtree drop.

