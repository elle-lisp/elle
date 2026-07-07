# Region representation — id-spaces, per-execution model, layout

Implementation-facing. How the compiler and runtime represent regions: the two
id-spaces, the per-activation physical-region model, the page layout, and how an
object's inline payload shares its region. The correctness obligations these
serve are in [region-rules.md](region-rules.md); the consumer model is in
[docs/regions.md](../regions.md).

## Two id-spaces: static and runtime

A region id means one of two different things depending on where it came from,
and conflating them is a class of UAF. Name them:

- **Static region ids** (`new_static_region`, `lir/lower`): compile-time *slot*
  numbers baked into bytecode. A static id is a per-function slot, **not** a live
  region. It is never used directly as a physical region.
- **Runtime physical ids** (`new_runtime_region`, per-heap `RegionStore`): the actual
  pages-owning regions, minted per allocation *execution* and recycled on free.

These two id-spaces are **types**, not conventions. A runtime physical id is a
`RuntimeRegion(NonZeroU32)`; a compile-time slot is a `StaticRegion(NonZeroU32)`.
`NonZeroU32` on both makes region/slot 0 *unrepresentable* (Rule 1 by
construction, not by a runtime `!= 0` assert), and being distinct newtypes means
a static slot cannot be passed where a runtime region is expected — "never index
a static id into `RegionStore`" is a compile error. "No region active" / "this
value has no region" is `Option<RuntimeRegion>` (`None`), never a sentinel `0`.

Crucially, a region is **not** a uniform optional field hung on every
instruction (which would let an allocation exist with no region — the invalid
state spelled `None`). It is a mandatory `region: StaticRegion` field on exactly
the LIR instruction *variants* that allocate or route a per-call region, and
absent from the structure of those that don't. "Region not applicable here" is
encoded by the field's absence; an allocation with no region is unconstructable.
Serialized into bytecode a `StaticRegion` becomes a raw `u32`; the VM decodes
that `u32` slot and resolves it to a `RuntimeRegion` — the two never meet as one
type.

Both index the single `RegionStore`, so two soundness guards keep them from
colliding:

- a static slot is resolved to a physical id through the current activation's
  `activation_region_map` (`runtime_region_for_alloc_slot`/`new_runtime_region_for_call_slot`/`take_runtime_region_for_drop_slot`), never indexed into
  the store as if it were physical;
- `new_runtime_region` never reissues an id that currently names a live region.

Drop either guard and two logical regions land on one physical id — a torn read
when one is freed under the other.

## The per-execution region model

A static region id is a per-function slot; every activation mints its own
physical region for it. `runtime_region_for_alloc_slot` records the slot→physical mapping
in the activation's frame so the matching `DecrefRegion` frees the same physical
region; `take_runtime_region_for_drop_slot` clears the slot so the next loop iteration mints
fresh. This is what makes deep recursion and loops run in bounded memory: one
static id names a per-function slot, never a single live physical region shared
across activations.

The model carries an emission-side obligation — **one allocation execution per
slot between drops**. `runtime_region_for_alloc_slot` mints fresh on every
execution and *overwrites* the frame's mapping, so if the lowerer emits N
allocation instructions against one slot with only the final `DecrefRegion`,
the first N−1 physical regions are orphaned the moment their mapping is
overwritten — an unreleasable initial reference each, i.e. a structural leak
(Rule 4's dual: every allocation *execution* needs its own demise, so every
allocation *instruction* needs its own slot unless a drop provably intervenes,
as the loop back-edge drop does). The file-letrec capture-cell pre-pass is the
case in point: `lower_begin` emits one `MakeCaptureCell` per captured top-level
binding, and routing them all through the Begin's single region slot would
orphan every cell but the last (at stdlib scale, thousands of cells plus
everything they pin). Pre-allocated capture cells therefore get **one region
per cell** (`begin_cell_regions`), each released by its own `DecrefRegion` at
its binding's last use.

## Constants lower as ordinary allocations, not promoted values

A heap literal — a string, an array/quoted form, a closure template — is **not**
a pre-allocated `Value` baked into the code object. The constant pool holds the
literal's immutable *template* (the bytes, the structure, the closure template)
as plain compile-time data. A `MaterializeConst` instruction builds a *fresh*
value from that template each time it executes, into the literal's own
solver-assigned region (`alloc_here(hir.id)`, the one-region-per-value baseline),
resolved per activation to a fresh physical region and allocated with
`arena::alloc_in_region(obj, region)` — exactly as `MakeArrayMut`/`List` do.

So a literal is born in the right region (Rule 3), dies at its `decref_point`
(Rule 4), and lives past that point only by ordinary RC if it escapes (Rule 5).
Re-materializing per execution is the correct-and-slow baseline: runtime
`(eval …)` and module load re-run the compiler, so the same source materializes
fresh copies each time, each reclaimed when it falls out of use. The rejected
alternative — a "constant-pool region" whose lifetime is the code object — would
promote a value into a longer-lived region (Rule 3 forbids promotion), need a
second demise mechanism outside `DecrefRegion`, and share one region across every
activation of the code object; do not adopt it.

Closure **templates** are no exception: the template is itself a
region-allocated heap object (its bytecode an inline `RegionSlice<u8>`),
materialized at its definition site; a closure **instance** holds a normal
cross-region reference to it, increfed when the instance is built and
cascade-released when its region frees. Region RC is the single reclamation
mechanism for code objects too.

## Physical representation

A per-thread page pool with size classes hands pages to regions on demand and
reclaims them at RC 0; it can munmap excess pages under pressure. Regions never
share pages (Rule 6). RC lives in `RegionStore`, one counter per physical region.
One region per value, unmerged, is the baseline: it claims a page per allocation
— correct but expensive. Two kinds of *merging* amortize that cost, both
collapsing several solver `Region`s onto one physical region (the
consumer-facing performance account is in
[regions/performance.md](../regions/performance.md)):

- the **builder-idiom seed** below — merge a freshly-built child aggregate into
  the parent aggregate it is stored into (the `%pair` car/cdr store). This is
  implemented as the analysis and runtime mint-or-reuse described in § Merging;
- **sibling page-amortization** — collapse sibling regions with coincident
  lifetimes and no edge between them. A later rider, not yet implemented.

## RegionSlice contents share their object's region

Non-obvious and load-bearing: immutable aggregates (string, array, struct, and a
**closure's captured env**) store their variable-length payload as an
`RegionSlice` laid out *in the same region pages* as the HeapObject header. Such
contents therefore have **no** region of their own and **no** cross-region RC
edge — their lifetime *is* the containing object's region's lifetime. Freeing the
object's region frees its inline payload with it.

The consequence to keep in mind: a closure's captured environment dies with the
closure's region. A prematurely-freed closure region surfaces as a *torn
captured-env read* in `populate_env`, not as an RC underflow — there was never a
separate region to underflow. When `(squelch f …)` shares `f`'s env, the new
closure's env is copied inline into the new closure's region, so that region now
owns the captures.

The corollary for **metadata-only clones**: an operation that rebuilds a heap
object to change only its metadata (`with-traits` is the canonical case) must
**copy the payload slice into the clone's own region** — `RegionSlice` is
`Copy`, and copying the `(ptr, len)` pair instead aliases backing pages in the
*source's* region with no counted edge: the source's ordinary demise then frees
the payload under the live clone (the with-traits UAF — a clone of `[1 2 3]`
captured by a spawned closure read freed pages in the send serializer;
tests/elle/region-withtraits-slice-uaf.lisp). It also falsifies the operation's
`Fresh` declaration, which claims the whole result lives in the call's own
region. The one sanctioned alias is the closure-env share (`squelch`/`attune`),
which pays for itself with an explicit backing edge in the free-cascade scan's
Closure arm.

## Merging

Merging collapses two solver `Region`s onto **one** physical region: both
allocations land in the same pages, and a single `DecrefRegion` frees them
together. It is the one mechanism allowed to break "one region per value"
(region-rules.md § "There are exactly two measures") — sound only when the merged
values share a lifetime, which the predicate below pins.

The first merge is the **builder-idiom seed**: a freshly-built child aggregate
merged into the **parent aggregate it is stored into**. The canonical shape is a
nested `%pair` — `(%pair (%pair 1 2) 3)` — where the inner pair is the car of the
outer. It is a down-payment on the forest's owned-subtree drop
(§ "Adoption and subtree drop"): a fully-fresh nested literal collapses to one
region, every car/cdr edge becomes intra-region, and the whole structure frees as
a unit. This is **not** sibling page-amortization (two values with no edge between
them) — that is a separate, later rider.

### The seed predicate

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
   magnitude (§ "The lifetime obligation the root carries").
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

### Emission: one slot per merge tree, one demise at the root

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
  region-rules.md § "Self-edge elimination"). `emit_increfs_for` drops it. A merge
  *without* this drop leaks `R`; a child drop *without* the merge frees early —
  the two move together, which is why allocation-canonicalization, child-decref
  suppression, and self-edge elimination are one mechanism, not three.

The whole mechanism is keyed on `RegionInfo::merged_parent` being non-empty. Under
`--checked-intrinsics=on` (the CLI default) `%pair` lowers as a native call, not a
`Pair` intrinsic node, so the seed predicate finds no sites, `merged_parent` is
empty, `merged_root` is the identity, and every step above is inert — the emitted
stream is byte-identical to the one-region-per-value baseline. The merge fires only
where `%pair` survives as an intrinsic (`--checked-intrinsics=off`).

### Runtime: the per-execution slot model and mint-or-reuse

The hazard merging must resolve is the **per-execution slot model** (§ The
per-execution region model): two alloc instructions (child, then parent) stamped
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

### The letrec closure-cycle merge

The builder-idiom seed merges one tight `child → parent` store edge. The same
collapse-to-one-region mechanism reclaims a shape per-region RC cannot: the
**immutable reference cycle mutual recursion forms** (`ping`/`pong`). Each member is a
capture-cell↔closure structure: the forward-reference **cell** holds the closure
(`StoreCaptureCell`) and a *sibling* closure **captures** the cell, both at the `letrec`,
never mutated. The cells and closures reference each other around the SCC, so per-region
RC never reaches zero (region-rules.md Rule 8) — the cycle leaks. Unlike a *mutable*
`@array` cycle (the deliberate boundary, § "Why this is hybrid"), an immutable one
is reclaimable, and a fiber that builds one per loop iteration would otherwise leak
unboundedly.

**Self-recursion is not this shape.** A purely self-recursive local fn (`loop` references
only itself) is **cell-free**: its self-edge does not mark it captured
(`hir/analyze/scopes.rs`), so it has no forward cell and no cell↔closure cycle — its
self-reference resolves to the currently-executing closure ([selfrec.md](selfrec.md)),
reclaimed by ordinary RC / the tail-call adopt, RC-identical to a top-level recursive
`defn`. So the merge is the **mutual**-recursion instrument; a pure self-recursive letrec
never has a cell and never reaches it.

The merge collapses the whole cycle — the closure SCC **and** its cells — onto one
region. Every interior reference then becomes intra-region, and all three
ref-counting paths self-skip a same-region reference (`rid != own_id`): the
alloc-scan incref over the closure env (`incref_cross_region_refs`), the
capture-cell store incref (`value/arena/mutate.rs::capture_store_with_rebind`), and
the free-time cascade (`regionpool/introspect.rs`). So the merged arena carries
RC 1 and one `DecrefRegion` frees the cycle wholesale — no edge accounting, no
member list. This is why the merge, not a group-free, is the right instrument: a
group-free would wholesale-free the closure SCC while the *cells* — in their own
regions, outside the freed set — still referenced it, and each cell's own
`DecrefRegion` would then over-free a dangling closure (a use-after-free
`--trace=guardfree` detonates under the full stdlib). Collapsing the cells *into*
the closures' region removes the dangling reference by construction.

**Two-layer detection** (`regions::merge::compute_closure_cycle_merges`), because
the cell↔closure structure is not one SCC in the graphs the other passes build:

- The **closures** carry the cycle. A `closure ⊇ closure` capture graph is
  re-derived from each lambda's captures (`binding_source_regions` of a captured
  binding is its *closure* region), and — unlike `capture_containment_edges`, which
  drops the `r == closure_r` self-edge — the **self-edge is admitted**. An SCC of
  size ≥ 2 is a mutual cycle. The single-closure self-edge is redundant for a genuine
  mutual cycle (the sibling edges already close the SCC); it is load-bearing only for
  the one mixed shape that still has a cell — a self-recursive member a *sibling* also
  captures (so it keeps a cell for that sibling) but that is not itself in a mutual
  cycle, a size-1 SCC the self-edge admits so its retained cell can merge into the
  closure. (A *purely* self-recursive closure is cell-free and refused at the cell gate
  below — it never reaches the merge.)
- The **cells** are coincident-lifetime members hung off the SCC: each cycle
  binding's prebound capture cell (`begin_cell_regions`), paired in through the
  binding's source closure region.

A cycle is mergeable only when **every closure is non-escaping**, **every member is
sole-held**, and **every closure has a static-slot cell**. The non-escape gate is
the **Shared-seed set** (`compute_shared_seeds` — return / emit / send frontier
crossings), *not* `EscapeInfo::lambda_escapes_definition`: that method additionally
folds in the capture facet (a value captured by an escaping closure), a containment
relation — and an SCC's closures capture each other, so one member crossing a frontier
would propagate "escaping" around the whole cycle and falsely refuse a mergeable one. A returned closure (in the Shared-seed set) genuinely outlives
the activation and is refused — it stays Shared (the always-legal baseline);
reclaiming an escaping closure cycle awaits the owner = activation/fiber cut.

The static-slot cell requirement is met in **every position**, top level and inside a
lambda body alike: a `letrec` binding that is immutable, never mutated, and
lambda-initialized — the recursive-closure shape — lowers its forward cell as a
compiled `MakeCaptureCell` held in the binding's own (stack) slot
(`BindingInner::letrec_compiled_cell`, the one predicate `lower_letrec` and the
region walk's Letrec arm both read), so its cell region is a `begin_cell_regions`
member wherever the letrec sits. A `letrec` binding **outside** that shape — mutated/
reassigned, or not lambda-initialized — keeps, inside a lambda, the runtime
`populate_env` env-cell route (no static slot), so it has no `begin_cell_regions`
cell and refuses the merge; a purely self-recursive binding is cell-free by
construction and never a member.

**Drop site — the binding scope.** A cycle has no member whose natural last-use
post-dominates the rest (no containing parent pins it, unlike the builder idiom), so
the merge sets the canonical root region's `decref_point` to the cycle's **binding
scope**: the single non-lambda `Let`/`Letrec` that prebinds every member's capture
cell (the `begin_cell_regions` key). This is decided by structural ancestry, never a
numeric `compute_order` compare (§ "The lifetime obligation the root carries"). The
root is the SCC closure of least program order (region ids order nothing); any member
mints the shared physical region at runtime (mint-or-reuse), so the root only names
the merged slot and carries the single decref.

Why the binding scope is the right post-dominator — and not its enclosing scope. The
members are bound in that one `letrec`, so **every direct reference to a member is
lexically within its scope** and the scope-exit (the lowerer's `emit_decrefs_for` on
the node, after its whole body) post-dominates them all. A reference *out* of the
scope is possible only by a **foreign capture** — a closure outside the SCC that holds
a member — and that is a cross-region reference *into* the merged arena, RC-counted:
increfed when the capturing closure is built (`incref_cross_region_refs` scans its env
for cross-region refs and records the outgoing edge) and released by the free-time cascade
(walking that recorded edge) when the capturer's region frees. So it keeps the arena's RC ≥ 1 past the single decref, and the arena survives
until the capturer dies. The single
`DecrefRegion` therefore releases only the cycle's own allocation reference, promptly,
at the binding scope-exit, and can never free a still-referenced arena. Eligibility is
gated on **letrec-subtree containment**: every member's allocation site must lie within
the binding-scope letrec's own subtree (a post-order interval test — the cells' sites
*are* the letrec node; the closures' `Lambda` nodes are its init descendants), so the
drop site is a structural ancestor-or-self of every member by construction. A member
whose region reaches the SCC from outside that subtree (a reused binding identity
naming a foreign lambda) refuses the cycle. The binding-scope drop is strictly tighter
than the enclosing structural post-dominator, because the cell target sits *at* the
binding node, whose enclosing-scope stack excludes itself, dragging the
allocation-site common ancestor up to the binding scope's **parent** — for a top-level
discarded cycle, the file `Begin`, i.e. program teardown. Dropping at the binding scope
itself closes that program-duration over-keep (the residual the §9 promptness ledger
named). The remaining slack — the binding scope-exit can still fall after a member's
last use *within* the letrec body — is bounded by that one scope, a granularity nit,
not the unbounded blowup.

**A tail-call letrec body hands the drop to a tail-call adopt — for a member *or* a
non-member callee.** When the letrec body ends in a frame-replacing tail call, the
binding-scope `DecrefRegion` is emitted past the `TailCall` — dead code — so the
release must ride the activation's completion instead. **The compiler cannot know at
compile time whether a tail call replaces the frame**: that is decided at runtime by
the callee *value* (a `func.as_closure()` replaces the frame and trampolines; a
`func.as_native_def()` keeps the frame and falls through to the live scope-exit drop),
and any binding — a redefined operator `+`, a `%`-intrinsic — may be rebound to
either. So the merge never classifies the callee; it wires **both** release channels
and lets exactly one fire.

- **A tail call to an SCC member** rides the existing stranded-cycle channel:
  `lower_letrec` marks the member bindings the letrec body tail-calls
  (`stranded_cycle_bindings`, derived from the body's `is_tail` calls without
  descending into nested lambdas), `tail_callee_adopts` returns true for such a callee
  (read through a **non-upvalue** reference only, so a nested closure in the body can
  never adopt the arena out from under a later use), and the `TailCall` carries
  `adopt_region = region_of(callee)` — the merged arena, because a member lives in it.

- **A tail call to a NON-member** (a native `%add`, a redefined operator `+`, a
  foreign closure `g`) rides an explicit slot instead. The analysis records the tail
  site in `RegionInfo::cycle_tail_adopt` (site HirId → the merged root region), the
  lowerer sets the `TailCall`'s `adopt_region_slot` to the root's static slot
  (`compute_closure_cycle_merges` → `ClosureCycleMerge::tail_adopt_sites`), and the
  runtime resolves that slot through the executing activation's region map — the arena
  was minted during the letrec setup and its scope-exit drop is dead. If the callee
  turns out a **closure**, the frame is replaced and `trampoline_loop`'s
  `adopted_closures` frees the resolved arena once (deduped) at the recursion's
  completion; if it turns out a **native**, the frame is not replaced, the slot is
  never consumed, and the live scope-exit `DecrefRegion` frees the arena — mutually
  exclusive, exactly one release, the compiler having classified nothing.

Both member and non-member releases run at the recursion's completion / the
scope-exit, so the same channel the cell-free self-recursive adopt rides
([selfrec.md](selfrec.md)). Interior sibling calls (`ev` tail-calling `od` inside the
SCC bodies) never adopt: `tail_callee_adopts` refuses any callee whose region is a
closure-cycle merge member (`RegionInfo::closure_cycle_members` — the merge owns the
release), and only the letrec-body marking overrides that refusal. On a body with
mixed tail exits (`(if c (ev k) (%add (ev k) 0))`) exactly one release fires per path:
the member arm adopts via `region_of` (its binding-scope drop dead there), the
non-member arm via `adopt_region_slot` or the live scope-exit drop.

**What the non-member tail still refuses — the by-move boundary.** A cycle member
passed **by-move as a tail argument** (`(g od)` — `od` itself, not `(ev k)`'s result)
refuses the whole cycle to Shared. The member's own move/return machinery decrefs the
merged arena a second time, colliding with the adopt (a double-free); the escape gate
does not catch it (an opaque callee's argument is not a return/fiber Shared-seed). So
the tail gate reads each argument's region-transparent flow bindings (mirroring
escape's `tail_sources`: through control/select/deref, stopping at a `Call`/
`Intrinsic`/`Lambda`) and refuses when one is an SCC member. A member stored into a
fresh aggregate then passed (`(g (%pair od 1))`) is RC-counted, and a member *called*
in an argument (`(g (ev k))`) contributes its result, not itself — both admitted. An
unresolvable non-member callee (no site to key the adopt at) likewise refuses.

**All-tier, unconditional.** The merge extends the same `merged_parent` forest the
builder seed populates and rides the same `merged_root` canonicalization and
`merged_slots` mint-or-reuse every tier already resolves — so it adds no opcode, no
JIT helper, and is not gated by `--region-ownership` (it lands on the flag-independent
`compute_merges` path). Pinned by `regions::tests::merge`
(`merge_collapses_mutual_recursion_letrec_closure_cycle` — the mutual SCC + cells collapse
onto one `merged_root`; `merge_collapses_in_lambda_mutual_recursion_letrec_closure_cycle`
— the same collapse and binding-scope drop for a letrec that is a lambda body;
`merge_admits_in_lambda_cycle_with_foreign_tail_callee` and
`merge_admits_native_tail_checked_on` — a non-member (foreign closure / native) body
tail now MERGES and records `cycle_tail_adopt`;
`merge_refuses_member_passed_by_move_to_foreign_tail` — the by-move boundary (`(g od)`
double-free) still refuses; `merge_refuses_escaping_letrec_closure`;
`merge_mutual_recursion_cycle_drops_at_binding_scope_not_enclosing`;
`self_recursive_letrec_is_cell_free_not_merged` — a pure self-recursive letrec has no cell
and is never a member; `merge_collapses_self_and_sibling_captured_member_cell` — the mixed
self+sibling-captured member's retained cell still merges), the guardfree fixture
`region_native_tail_mutual_cycle_uaf` (every non-member tail kind, mixed, and
per-loop-iteration reclamation, panic-clean under `--trace=guardfree` on both
`--checked-intrinsics` settings), and
`runtime::tests::ownership::region_ownership_reclaims_mutual_recursion_closure_cycle`
(bounded per-run region growth beside a leaking discriminator, at both flag settings), with
`region_ownership_reclaims_nested_mutual_recursion_per_call` driving the in-lambda cycle
per call (bounded beside the live-chain discriminator, base case included) and
`closure_cycle_discarded_release_is_prompt` pinning the binding-scope drop's promptness (a
discarded top-level cycle freed at its letrec, not held to teardown).
`region_ownership_reclaims_self_recursion_closure_cycle` pins the same bounded growth for a
pure self-recursive closure, which is reclaimed cell-free (ordinary RC / the tail-call
adopt — [selfrec.md](selfrec.md)), not by this merge.

## Adoption and subtree drop (the ownership forest)

Merging *collapses* two regions onto one physical id; **adoption** keeps them
distinct but **links** them into a parent→child ownership tree, so a whole subtree
frees as a unit when its root frees. Where merging is the tight single-edge case
(one child stored once into one coincident-lifetime parent), adoption is the
general case — a multi-region externally-unique component, including a mutable
retaining container and the values funnelled into it, a closure and its captures,
and the interior reference cycles that the per-region RC cascade cannot collect
(region-rules.md Rule 8). The compile-time analysis that classifies a region
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

### The runtime: a reclamation typestate and `owned_children`

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
  § Merging, generalized). A region is adopted **at most once** — a second adoption
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
  (§ "The lifetime obligation the root carries"): the member's release is sorted ahead
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
  `owned_children`, walked Rust-side with no heap deref — then **reads every member's
  recorded `outgoing` edge table** (§ The outgoing edge table) and partitions its
  targets: a target *in* the freed set is interior and dropped (reclaimed by this drop,
  never cascaded), a target *outside* is a genuinely-**Shared** frontier ref to cascade;
  only then returns every member's pages, bumping each generation (a stale pointer into
  them detonates at the next debug `region_of`, exactly as for an ordinary free;
  region-generations.md); and finally cascades the collected Shared-frontier refs once.
  **No phase dereferences a heap page to discover an edge** — discovery is the recorded
  table, not a content walk; pages are touched only to tear them down. Interior cycles
  reclaim with the pages: the drop walks `owned_children`, not the reference graph, so a
  `(push a b)(push b a)` knot interior to one owned subtree frees with the subtree and
  never strands. Pinned by
  `regionstore::tests::subtree_drop_cascades_shared_frontier_not_interior_cycle`.

### The outgoing edge table — reclamation without a heap scan

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

### Owner nodes — an activation as a forest root

An owned subtree's root need not be a pages-owning region: the forest's owner lattice is
{region, activation, fiber}, and the runtime realizes an **activation owner** as an **owner
node** — a pages-less region used purely as a forest root. The node id is minted by
`new_runtime_region()` (so it can never alias a live region) and no allocation ever targets
it; its `RegionEntry` exists only to carry `owned_children`. A member joins by the ordinary
`adopt_region(node, member)` — `Counted → Owned`, count consumed — so every typestate
guarantee above holds unchanged: a member's stray decref is a structural no-op, a second
adoption is a debug-asserted bug, and the node's demise is one `free_region_set` over node +
transitive children (interior cycles reclaim with the set; the Shared frontier, read from the
recorded `outgoing` tables, cascades once). **No new reclamation mode exists** — the node
rides the same subtree drop a region root does; tearing down its own entry returns zero
pages. Pinned by `regionstore::tests::forest::pages_less_owner_node_subtree_drops_members`
and `…::interior_cycle_in_owner_node_reclaims`.

**The channel is `AdoptIntoActivation { child }`** — value-resolved like `AdoptRegion` (the
handler resolves the child's runtime region through `result_region_of`, unwrapping a capture
cell) but carrying **no parent operand and no static slot**: the parent is the *current
activation's* node, minted lazily at the first adopt so an activation that adopts nothing
pays nothing (an immediate child — no region — adopts nothing and mints no node). The
channel is **idempotent on an already-Owned child**: the handler adopts nothing when the
child's region is already a forest member, so a program that hands one region to the channel
twice — a masked-`:error` fiber restarted after delivering the same payload, a value handed
back twice — leaves it owned by its **first** adopter (whose release post-dominates the later
hand-off's use, every consumer being gated to discard) instead of tripping the one-owner
adopt assert. The compiler-paired `AdoptRegion` sites keep the strict assert — their
inference claims each member exactly once; only this consumer-facing channel absorbs
re-delivery. Its production consumers, under `--region-ownership`, are the
capture-back-edge cut and the transferred-returned-subtree cut (both below).

**The capture-back-edge SCC — owner = activation.** The one containment-graph shape neither
region-rooted mode can own is the **capture-back-edge cycle**: a container captured by a
closure it holds (`m ⊇ c` by store, `c ⊇ m` by capture — the m↔c SCC). A region root cannot
own it — `m` is captured, so its `decref_point` is over-extended one structural step past
the closure, and `m` is store-adopted (its own `DecrefValueRegion` stays live), so the
owner-aware lifetime obligation refuses the subtree (the refusal
`adopt_edges_refuses_captured_store_member_on_lifetime` pins) — and the co-owned group free
cannot either (`c` is a closure region, whose cell⊇closure containment the
external-uniqueness scan cannot see). The activation owns it instead
(`regions::ownership::compute_activation_adopts` → `RegionInfo::activation_adopt_sites`):
the SCC's members are adopted into the executing activation's owner node and freed by its
completion release, which post-dominates every in-activation use by construction. Admission
gates, each refusing to Shared (the always-legal baseline):

- **the signature** — a genuine mutual-reach SCC (≥ 2 members) whose interior edges include
  at least one *capture* AND at least one *store* (a non-hard `cross_region_refs` edge, or
  a funnel-recovered `containment_edges` edge — so the cut admits the checked-on production
  path, where the store is an opaque `Funnel` call, exactly as it admits the intrinsic
  path). A capture-only SCC is the letrec closure web (the merge's instrument, or class 4/6
  admission); a store-only SCC is the co-owned group's;
- **member gates** — every member ownable (no frontier crossing, no dynamic-lifetime
  class), sole-held, with pairwise-distinct holder bindings (each member must have its own
  slot for the value-resolved adopt to load);
- **disjointness** — no member is claimed by another mechanism: a merge participant
  (builder-idiom or closure-cycle), a co-owned group member, or a store/capture-adopt
  subtree region is never also node-adopted (the one-owner invariant at the emit level);
- **the hull** — every region referencing INTO the SCC, transitively over all edge kinds
  (hard may-stores included), must itself be ownable: the members free at the activation's
  completion, so every holder must provably die within the activation (a holder that
  returns or crosses a fiber frontier refuses the SCC). The hull members keep their own
  baseline releases — their cascades onto the Owned members are structural no-ops;
- **one activation, no loop seam** — the members' allocation sites share an innermost
  enclosing structural scope (the adopt site; a cross-lambda SCC refuses), and no
  `While`/`Loop` encloses a member's allocation without also enclosing the adopt site
  (adopt-per-iteration is sound — fresh regions each round; alloc-inside/adopt-outside is
  not — the static suppression would outlive the slot's last iteration value).

The lowerer emits one value-resolved `AdoptIntoActivation` per member at the adopt site
(`emit_adopt_into_activation`, driven by `emit_decrefs_for` exactly like the co-owned
group's free), and `analyze_regions_with` suppresses **both** members' own compiler decrefs
through `suppressed_decref_regions` — the same suppress ⊆ adopt contract the capture adopt
carries, and the same set every decref-emit site (`emit_decrefs_for`, `emit_arm_decrefs`,
`emit_branch_compensation`) already re-checks defensively, so no release path can double a
node member's demise. The members stay `Counted` between construction and the adopt (normal
RC absorbs the interval — an outside holder's earlier cascade decref just lowers the count
the adopt then consumes); from the adopt to the activation's completion they are `Owned`,
and the node's release frees the cycle wholesale, interior m↔c references reclaiming with
the set. Pinned by `regions::tests::adopt::activation_adopts_capture_back_edge_scc`
(rooted and bare shapes, both intrinsic and funnel-recovered stores),
`…::activation_adopt_excludes_other_mechanisms` (merge/group disjointness), and at runtime
by `runtime::tests::ownership::region_ownership_capture_back_edge_cycle_reclaims`
(bounded flag-on beside the leaking flag-off counterfactual, panic-clean, on the
interpreter and under the JIT).

**The transferred returned subtree — owner = the consuming activation.** The second
containment shape no region root can own is the **returned cycle**: a callee builds an
externally-unique subtree containing a reference cycle and hands its root back across the
return (or fiber) frontier. Inside the producer every member crosses no frontier but the
root does, so the region-rooted cuts refuse (a Shared seed poisons the subtree walk and the
group walk alike); in the consumer the root is an opaque call-result whose
`DecrefValueRegion` releases one reference — but a cycle's interior back-edge holds another,
so the cycle survives every release and leaks per call. The owner that reclaims it is the
**consuming activation**: its owner node's release post-dominates every use of the result,
on either side of the frontier (every producer-side use precedes the return; every
consumer-side use precedes the completion). The cut
(`regions::ownership::compute_transfer_adopts` → `RegionInfo::transfer_adopt_regions` plus
interior edges merged into the adopt maps) has a producer half and a consumer half, admitted
only together — the interior adopts freeze member counts, so an unadopted consumer would
hold uncounted borrows; one inadmissible consumer site refuses the whole callee:

- **the producer summary** — a lambda reachable only through an immutable, single-init,
  never-mutated binding (or as a bare `fiber/new` body), whose body tail resolves through
  the structural wrappers to a single binding with exactly **one** source region: the
  **root**. The root must be allocated in the lambda, may cross the **return** frontier
  (that is the shape) but not the **fiber** frontier (an emitted/sent root has an unbounded
  second consumer), and must not be any dynamic-lifetime class. Every other member of
  `reach(root)` is born AND last-used inside the lambda (a captured outer value, or a member
  a later sibling still reads, refuses — freeing at the consumer's completion must not free
  anything with a life of its own), crosses no frontier, is sole-held, and is claimed by no
  other mechanism. The subtree must be externally unique (no edge from inside to outside —
  the return itself records none) and must contain an **interior cycle** (an acyclic
  returned subtree reclaims promptly by the RC cascade today; adopting it would only trade
  promptness away). Each non-root member gets its single owner exactly as the store/capture
  adopt assigns one — and, uniquely to this cut, a **funnel-recovered** owner edge is
  emittable too: the adopt is keyed at the funnel *call site* (`funnel_store_sites` joined
  with `containment_edges`), so the checked-on production path admits identically to the
  intrinsic path (the value-resolved adopt needs no store opcode).
- **the consumer gate, at every call site of the summarized callee** — the call's result
  region must cross no frontier, appear in **no** edge of any kind (hard may-stores
  included), belong to no dynamic class, and be **discard-shaped**: no user binding holds
  it, or its sole holder's every read is an argument of an `Immediate`-effect native. A
  consumer that stores, captures, returns, or extracts from the result refuses the callee —
  extraction through a pass-through native (`get`/`first`) records no edge, so the
  discard-shape gate is what keeps an uncounted member borrow from escaping the node's
  reclamation horizon.
- **the fiber face** — the same summary applied to a `fiber/new` body whose inferred signal
  can deliver no non-terminal value (no yield / io / debug / wait bits, not polymorphic): a
  completing `fiber/resume` then hands back the body's **terminal** value — the returned
  subtree, crossing the fiber frontier — and every other resume outcome is a fresh error
  struct or an immediate, each safely adoptable. The fiber binding must be single-init,
  never mutated, **uncaptured**, and bound in the same function body as its every use
  (each activation of the consumer then drives its own private fiber, so no delivery can
  outlive the adopting activation — the restarted-`:error` re-delivery lands in the same
  activation, where the channel's idempotence absorbs it); each use must be arg0 of a
  `fiber/resume` (a gated consumer site) or an argument of an `Immediate`-effect native
  (`fiber/status`). `fiber/value` is pass-through — a second route to the terminal subtree —
  and is refused by the use gate.

Emission is two-sided. The producer's interior owner edges ride the ordinary adopt maps
(`owned_adopt_edges` at store/funnel sites, `capture_adopt_edges` at the closure — capture
members suppressed under the same suppress ⊆ adopt contract), building the runtime ownership
tree under the root while the root itself stays `Counted` through the hand-off (its count at
the consumer's release is ≥ 1 by construction: the release *is* the adopt). At each consumer
site the root's release — the slot-loaded or discarded-result `DecrefValueRegion` — is
**replaced** by `AdoptIntoActivation`: the adopt consumes the whole count (the interior
back-edge's stuck reference included), and the node's completion release set-drops root +
owned members in one collection, interior cycle edges dropping in-set. Promptness is the
designed activation bound: the subtree frees at the consuming activation's completion (for a
top-level consumer, the root activation's exit) rather than at the result's last use — paid
only for a discarded returned *cycle*, which the baseline never frees at all. The **fiber
tier** of the owner lattice is reached structurally, not by a distinct opcode: a consumer
that parks moves its node into the suspended frame like any activation state, and the
terminal-fiber teardown gathers parked nodes under the fiber node for one set-drop — the
transfer runtime below. Pinned by `regions::tests::adopt::transfer_adopts_*` (admission,
both intrinsic and funnel-recovered faces; the refusal family) and at runtime by
`runtime::tests::ownership::region_ownership_reclaims_returned_cycle_across_calls`,
`…_reclaims_fiber_terminal_cycle`, and `…_transfer_adopt_rides_parks_and_fiber_teardown`
(bounded flag-on beside the leaking flag-off counterfactual, on both `--checked-intrinsics`
settings and under the JIT).

**Lifecycle.** The node slot is per-activation state carried beside the region-remap frame:
`Fiber::activation_owner_nodes` parallels `activation_region_maps`, pushed empty on every
fresh activation entry (the interpreter's `saving_stack` push, the JIT prologue's
`push_region_map`) and popped with it. The node is freed **implicitly at the activation's
normal completion** — the interpreter trampoline's clean
break and the compiled function's `Return` path
(`elle_jit_release_activation_owner_node`) each take the slot and run one
`decref_region_if_present(node)`: rc 1→0, subtree drop — never by an emitted drop
instruction (no single static site covers return + tail + yield + error + squelch). This is
the same clean-break discipline as the trampoline's tail-call-adopted closure release, and a
frame-replacing tail call likewise keeps the activation — and its node — alive to the
recursion's completion.

**A park moves the node into the suspended frame.** A suspending exit — a yield, a
suspending native, `fiber/resume`'s SIG_SWITCH handoff, a fuel pause, a capability denial —
parks the activation's continuation as a `BytecodeFrame`; the frame **takes** the
activation's node (`BytecodeFrame::activation_owner_node`, a parameter of
`BytecodeFrame::suspend` so every suspend site must decide it) exactly as it carries the
activation's `activation_region_map`. The members stay Owned (RC frozen) across the park,
so the node is the only route to them; losing it at the suspend would strand every adopted
member. Where the park is built by the *caller* of the already-unwound activation (a fiber
body's pause in `do_fiber_first_resume`, a callee interrupted mid-instruction in
`call_inner`), the node rides out in `ExecResult::activation_owner_node`, captured by
`execute_bytecode_saving_stack` beside the region map just before the frame pops.
`resume_suspended` restores the parked node into the slot beside
`restore_activation_region_map`, so the resumed body's normal completion frees it through
the same trampoline clean break, and a body that parks again re-captures it (the yield
handler's take, or the re-suspend frame built from the exec result). The node is **moved**
at every step — taken from the slot into exactly one frame, restored from the frame into
exactly one live slot, never cloned — so a second release path is unrepresentable by
construction. Pinned by
`runtime::tests::ownership::activation_owner_node_survives_yield_resume_completion`,
`…_survives_repeated_parks`, and `…_rides_exec_result_across_fuel_pause` (interpreter park /
re-park / caller-built park), and `jit::suspend::tests::park` (the JIT yield side-exit
parks the node; the interpreted resume completes and frees it).

**A discard frees the parked node (squelch/abort = subtree drop).** Abandoning suspended
work — a squelch/attune signal-violation, an abort — flows through one chokepoint,
`VM::discard_suspended_frames` (reached from `enforce_squelch` on every tier: the
interpreter trampoline, `compile/run-on`, and the JIT call paths). The discarded frames'
continuations will never run, so the completion release above never fires for them; the
chokepoint therefore runs it *at the discard*: each discarded `BytecodeFrame`'s parked node
gets the same one tolerant decref — rc 1→0, subtree drop over node + adopted members, the
Shared frontier cascading once from the recorded `outgoing` tables. This frees **only** the
node: the regions named by the frame's `activation_region_map` are a borrowed view —
possibly shared with an outer, non-discarded frame or the activation that catches the
squelch — and releasing them here would over-release (the historical squelch double-free);
a node's members, by contrast, are exactly the regions the inference proved externally
unique and moved in through `AdoptIntoActivation`, so the discard release cannot touch a
region any live frame still counts on. A frame dropped *outside* the chokepoint (an
abandoned error park) still abandons its node — a bounded leak, never a double-free (the
members have no count for any other release route to reach). Pinned by
`runtime::tests::ownership::discard_frees_parked_activation_owner_node` (single frame and
multi-frame chains; the member's generation bumps at the discard, bounded across repeated
park-discard cycles) with the full-stdlib squelch corpus under `--trace=guardfree` as the
panic-clean gate.

**Exactly one reclamation path (the double-free invariant, positively).** A node member is
`Owned`: it has no count for any other release route to reach, the inference that emits its
adopt must suppress the member's own compiler decref (the same suppress ⊆ adopt contract the
store/capture adopts carry), and membership is granted only through `AdoptIntoActivation`
for a region proven externally unique. The node's completion free is therefore the member's
sole demise.

**All-tier.** The interpreter arm (`handle_adopt_into_activation`,
`src/vm/dispatch/region.rs`) and the JIT helper (`elle_jit_adopt_into_activation`,
`src/jit/dispatch/region.rs`) share the VM's lazy-mint + adopt body; the WASM backend
handles the op structurally (a no-op arm — the arena boundary reclaims); a function carrying
it is GPU-ineligible (`is_gpu_instruction`). Pinned end-to-end by
`runtime::tests::ownership::activation_owner_node_frees_adopted_member_on_normal_completion`
(interpreter) and `jit::compiler::tests::adopt_into_activation_frees_member_at_compiled_return`
(JIT), each asserting the member's generation bump at completion and bounded region growth
across repeated activations.

**The fiber owner node.** The owner lattice's fiber tier is a second pages-less node,
`Fiber::fiber_owner_node` — the forest root for a region whose owner is the **fiber**
itself: a member that outlives every single activation of that fiber (the cross-call /
cross-fiber transfer class). It is fiber state on the `Fiber` struct, so — unlike the
per-activation node, which every park must move into a frame — it rides suspension,
resumption, and fiber swaps structurally, with nothing to transfer. Minted lazily; `None`
for a fiber that owns nothing. No production lowering targets it *directly* — the
transferred-returned-subtree cut adopts into the consuming **activation's** node, and the
fiber tier is reached structurally: a parked consumer's activation node rides its frame, and
the teardown below gathers every parked node under the fiber node for one set-drop.

**Fiber teardown frees everything the fiber owns.** The members a fiber owns are released
at its **terminal** transitions, through one take-then-release pair
(`take_fiber_owned` / `release_fiber_owned`, `src/vm/fiber.rs`): the taking empties the
fiber's owned slots (each still-parked `BytecodeFrame`'s activation owner node, and the
fiber node) under the fiber borrow; the releasing then runs against the heap with the
borrow already dropped, so heap mutation never overlaps fiber access and a cascade that
frees the fiber's own heap value cannot invalidate a live borrow. When a fiber node
exists, each parked node's members are first gathered under it
(`reparent_owned_children`) and the emptied node freed, so the teardown is **one**
set-drop over the fiber's whole owned set — node + members + interior cycles, the Shared
frontier cascading once from the recorded `outgoing` tables; with no fiber node each
parked node subtree-drops directly. The terminal transitions are: normal completion
(`with_child_fiber`'s `:dead` arm), a halt (`VM::finalize_dead_fiber`, at every
`SIG_HALT → Dead` promotion), and the hard kills — `fiber/cancel` of a new/parked fiber
and `fiber/abort` of a not-yet-started one (`kill_fiber`, which the discarding
`suspended = None` sites route through). An `:error` fiber is **not** terminal — it is
resumable (the restarts system replays its re-parked frame) — so an error promotion
releases nothing: its parked chain and nodes stay live for the resume. The contract a
fiber-node member carries: it must never hold the fiber's **terminal result** — a result
that outlives its fiber is transferred out (`reparent_owned_children`) before completion,
never left to be freed under the consumer's read. Pinned by
`runtime::tests::ownership::fiber_owner_node_freed_at_fiber_completion`,
`…_survives_parks_and_frees_at_completion` (a multi-frame chain: every parked frame's
node and the fiber node reclaim), and `fiber_kill_frees_parked_and_fiber_owned`
(cancel of a parked fiber; abort of a new one), with
`tests/elle/region-fiber-cancel.lisp` under `--trace=guardfree` as the
frees-nothing-live gate.

A fiber abandoned **outside** those transitions — a parked fiber whose last handle drops,
or a chain replaced without a discard — still strands its nodes until heap teardown
(`RegionStore::teardown_all`), the same bounded abandoned-park class as an error park
whose fiber is never resumed.

### The capture adopt — emitted at the closure, for every capture kind

A capture containment edge (`closure ⊇ captured`) has no store site — capture records no
`cross_region_refs` edge (the RC double-count fix: the runtime auto-incref over the
`Closure` env stands in for a static `IncrefRegion`) — so its adopt is keyed by the
closure's **construction**: at `MakeClosure`, `lower_lambda_expr` reloads each adopted
captured value and emits a value-resolved adopt in place of the capture's baseline
`IncrefRegion`, and `analyze_regions_with` suppresses the member's own compiler decref (the
closure's subtree drop is its sole reclamation). How the edge points depends on how the
captured binding is materialized (`ownership::capture`):

- **By-value capture** — an immutable, non-prebound local the closure holds directly: the
  edge is `closure ⊇ content`, adopted with `AdoptRegion(closure, content)`. The reload
  covers a direct local (from its binding slot, `LoadLocal`) and a by-value
  upvalue/transitive capture (from the constructing function's environment, `LoadCapture`).
- **Immutable letrec forward-reference cell** — a prebound `MakeCaptureCell` a sibling
  references before its initializer runs. The closure holds the CELL, not the content, so
  the edge is re-pointed at the **cell region** (`single_cell_region_of`) → `closure ⊇ cell`.
  Paired with the walk's `cell ⊇ content` edge (`record_cell_content_edges`, keyed at the
  cell's mint scope), external uniqueness sees the true chain `closure ⊇ cell ⊇ content`, so a
  **local, non-escaping `{closure, cell, content}` clique** is externally unique → Owned →
  reclaimed as a unit by the closure's subtree drop, the interior cell↔closure reference
  included. The cell store is UNCOUNTED, so this re-pointed edge is the ONLY way the scan sees
  the cell is held; the runtime realizes it with **`AdoptCellRegion`**, which resolves BOTH
  operands with `region_of` (never `result_region_of`, which would unwrap the cell to its
  content) so a cell's OWN region is named. A second `AdoptCellRegion` at the cell store links
  the content into the cell (`cell ⊇ content`, `maybe_emit_cell_content_adopt`) where the
  lifetime obligation admits; elsewhere the content reclaims by the cell's free-time RC
  cascade. Pinned by `regions::tests::cells::{walk_records_cell_contains_content_for_compiled_letrec_cell,
  capture_edge_points_at_cell_region_not_content}`,
  `regions::tests::adopt::owned_subtrees_admits_local_capture_cell_clique` (with its escaping /
  two-sibling refusals), and the emit by
  `lir::lower::tests::preallocated_capture_cells_get_distinct_regions_each_released`.
- **Re-storable cell** (`is_restorable_capture_cell` — an `@`-mutable captured local or a
  mutated captured parameter): a **borrow**, no owner edge. Its content lifetime is
  per-rebind — the rebind funnel decrefs each displaced prior — and SHORTER than the cell's,
  whose release is hoisted once past enclosing loops; adopting the content into the cell's
  subtree would free a displaced prior under the live cell (the loop over-free). So
  `capture_containment_edges` skips the capture and `compute_adopt_edges`'s `adoptable_cell`
  refuses its `cell ⊇ content` edge — which the walk still records (the cell holds *a*
  content) for external-uniqueness counting — and the content reclaims on the per-region-RC
  baseline. Pinned by `region_capture_cell_loop_uaf_ownership` (the guardfree witness) and
  `regions::tests::adopt::{capture_edge_skips_restorable_cell_admits_immutable_in_one_clique,
  restorable_compiled_cell_records_content_edge_but_is_not_adopted}`.

A mutually-recursive `letrec` closure **cycle** (each closure holds the other's forward
cell) is a *cyclic* clique, not a rooted subtree, so it is reclaimed by the closure-cycle
**MERGE** (§ "The letrec closure-cycle merge"), which collapses the SCC ∪ its cells onto one
arena before this pass — never by this capture-adopt path (`recur-local-mutual` /
`recur-local-self` read closed in `oracle.lisp`). The capture-adopt path serves the *acyclic*
rooted clique above.

The capture-adopt contract — every suppressed member is adopted — is discharged by **emit
capability**, not by refusing shapes the emit cannot reach: `lower_lambda_expr`'s reload
covers every capture kind, and the `debug_assert` at the emit is the backstop that every
adopt edge matches a real capture of the constructed closure. Pinned by
`lir::lower::tests::capture_adopt_reloads_upvalue_via_load_capture` (the env-reloaded
emission) and `regions::tests::adopt::capture_adopt_edges_are_emittable`.

What bounds the *admission* of a capture owner-edge is the general subtree filters — no
lowerability filter exists — and the **cross-activation (upvalue) owner-edge is refused at the
lifetime obligation**. With the capture edge re-pointed through the cell the containment is
now VISIBLE, so a subtree over an upvalue member DOES form: external uniqueness admits
`{nested, cell, enclosing, member}`, the member being captured by BOTH the nested closure and
the enclosing forwarding lambda. The refusal then holds structurally at the lifetime
obligation — the nested closure's region is minted per CALL of the enclosing closure, so
claiming a member that survives across calls would free it under the enclosing environment's
still-live reference and re-adopt an already-owned region on the next call. The forwarding
capture resolves the member's tight last-use to a position at/past the enclosing lambda's
node, after the nested root's in-body drop in post-order, so the obligation cannot prove the
member dies before the root and refuses. A member reachable through an upvalue capture is
therefore Owned only by an owner that outlives **every** capturer — the activation/fiber owner
node of the owner = activation cut — never by a region root; until that owner exists the shape
stays Shared (the always-legal baseline). Pinned by
`regions::tests::adopt::owned_subtree_upvalue_capture_owner_refused_on_lifetime` and
`closure_web_capture_not_yet_claimed`, and at runtime by
`runtime::tests::ownership::upvalue_capture_family_runs_sound`.

### The funnel adopt — the checked-on store face

Under `--checked-intrinsics` (the production default) a mutable store is an opaque
`Funnel` native call that records **no** `cross_region_refs` edge (the runtime funnel
counts the store; a compile-time edge would double-count — region-effects.md § `Funnel`),
so the store-keyed adopt would find no emittable interior edge and every funnel-built
subtree would refuse to Shared. The containment is recovered instead from the container
argument's `RetType` (`RegionInfo::containment_edges`, recorded **site-keyed** —
`(funnel call site, contained, container)` — by the walk's `Funnel` arm), and
`compute_adopt_edges` admits those edges as a third interior owner-edge kind beside
stores and captures: the adopt rides the same `owned_adopt_edges` map, keyed at the
**funnel call site**, and `emit_increfs_for` emits the same value-resolved
`AdoptRegion(container, member)` there — both endpoints reload from their binding slots,
so no store opcode is needed. This is the same funnel face the activation and transfer
cuts carry (their F-b admission); with it, the ownership forest works identically on the
checked-on production path and the intrinsic path, which is what let the
`--region-ownership` → `--checked-intrinsics=off` CLI forcing be deleted
(`config/parse.rs`).

The RC composition needs no explicit balancing: by the time the adopt executes, the
funnel's runtime store-incref has already counted the member, and `adopt_region` moves it
`Counted → Owned` **consuming the whole count**; the member's own later
`DecrefValueRegion` (sequenced before the root's drop by the lifetime obligation) and the
container's free-time cascade decref both land on the frozen `Owned` mode and no-op
structurally — the root's subtree drop is the member's sole demise. A funnel-adopted
member is a **store**-adopted member for the lifetime obligation (its own decref stays
live, so it is bounded by its structural `decref_point`, `EmitMode::Adopt`, loop clause
applied), and the emit site — the funnel call that stored the member into its owner —
structurally follows both allocations, so both slots are populated when the adopt runs. A
containment edge whose member the adopt cannot key (no owner edge at any site) still
refuses the subtree to Shared, the always-legal baseline.

What the funnel face deliberately does **not** cover: the builder-idiom MERGE stays
intrinsic-only. A checked-on `%pair` is a `Fresh` native call whose result region is a
call-result placeholder no static slot can name, so there is nothing for the
`merged_slots` mint-or-reuse to ride — and nothing leaks for it: a `Fresh` constructor's
embedding is alloc-scan counted and cascade-released (region-effects.md § `Fresh`), so
the merge's absence checked-on costs region-count locality, never reclamation. Pinned by
`regions::tests::adopt::adopt_edges_claims_funnel_recovered_subtree_checked_on` (the
funnel-site adopt edges), `…::adopt_edges_refuses_loop_enclosed_member_checked_on` (the
obligation holds on the funnel face), and at runtime by the checked-on facets of
`runtime::tests::ownership::region_ownership_reclaims_interior_cycle_subtree`,
`…_reclaims_nested_cycle_subtree`, and `…_reclaims_bare_cycle_group` (bounded flag-on
beside the leaking flag-off counterfactual, on both `--checked-intrinsics` settings).

### The lifetime obligation the root carries

Subtree drop fires at the **root's** single `DecrefRegion`, at the root's
`decref_point`. For that to be sound the root's demise must **post-dominate every
member's last use** — a child read after the root's `decref_point` would be freed
out from under the read. Adoption and merging discharge this through **one** structural
post-dominance predicate (`regions::postdom::drop_post_dominates`, decided over the scope
tree's post-order subtree intervals, *not* by `compute_order` magnitude):
`compute_adopt_edges` **refuses** a component with an un-post-dominated member (it stays
Shared, the always-legal baseline) and gate 6 refuses such a merge (§ Merging,
condition 6). The two differ only by `EmitMode` — ADOPT's store-member keeps its own
decref, so a loop enclosing the root's free is the cross-iteration UAF the predicate
refuses; MERGE's child is reachable solely through the parent (conditions 1+4), so the
loop clause is waived. The external-uniqueness walk
(`regions::ownership::compute_owned_subtrees`) proves the *frontier* is unique but
does not by itself order lifetimes, so this obligation is the emit's, not the
walk's. (The pinning test is the e2e reclamation tier; an interior-outlives-root
shape must stay Shared, never adopt.)

**Post-domination is necessary but not sufficient — the emit order carries the
rest.** Node-granularity post-dominance admits a member whose `decref_point` is the
**same node** as the root's demise (the straight-line coincident case — a fresh
container built and consumed in one expression). At that shared node the *intra-node
emission order* then decides soundness, because a store-adopted member keeps its own
`DecrefRegion`, whose no-op depends on the member still being **`Owned`**: it must be
emitted **before** every release that can free the member's owner at that node. Two
facts make "before" non-trivial for a container root:

- The root of a mutable-store subtree is typically a `Fresh` **call-result** region
  freed value-based, and it carries **more than one** runtime reference — the
  holder-binding release *and* the **discarded pass-through result** of the store
  itself (`%array-push`/`%put` return their container, so the store's own result is a
  second call-result region that resolves to the root at runtime and, when the result
  is discarded, releases the root). Whichever release zeroes the root triggers the
  subtree drop; the obligation's single `region_data[root].decref_point` names only
  one of them.
- A store-adopted member's own `DecrefRegion` is a structural no-op **only while the
  member is `Owned`**; once the subtree drop has reclaimed it, that slot-resolved
  decref faults (`regionstore/refcount.rs`, the phantom/double-free assert).

So the emit orders every store-adopted member's release **first** at each shared
`decref_point` — the members-first class in `with_region_info`'s bucket sort — ahead of
the call-result readers and the plain freers. A member's release reads and frees
nothing while `Owned`, so ordering it before the readers is safe; it then no-ops, and
whichever root-freeing release fires afterward subtree-drops the member exactly once.
The invariant this restores is stated positively in § "The runtime: a reclamation
typestate and `owned_children`": a store-adopted member's decref hits the still-frozen
`Owned` region — a no-op — because it is emitted before the root's drop. The reference
for the inverted-order double-free it prevents is a test, never this prose:
`lir::lower::tests::release::store_adopted_member_release_precedes_owner_in_shared_bucket`
(the emit-order pin), `region_array_push_pair_loop_uaf` (the guardfree witness), and
`runtime::tests::ownership::region_ownership_pair_pushed_into_let_bound_array_in_loop_reclaims`
(bounded + panic-clean).

### Why this is hybrid, and where RC remains

Owned subtrees reclaim structurally; **Shared** regions keep the unchanged
per-region RC and cascade. A region is Shared whenever the compiler cannot bound
its frontier — a value genuinely escaping its fiber with no common dominating
activation, or a may-store the solver could not resolve. The split is the endpoint,
not a transition: the same hybrid Project Verona keeps (Owned `iso`/`mut` subtrees,
RC'd `imm`). Adoption is gated behind `--region-ownership` until the full suite
passes identically flag-on and flag-off under `--trace=guardfree`; with the flag off
no region is ever `Owned`, every region stays `Counted` with empty `owned_children`,
and every path above is inert — the per-region-RC baseline stands unchanged.
