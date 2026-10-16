# The mechanism

<!-- audited: 2026-10-16 -->

The RC-instruction machinery the [rules](rules.md) constrain: how each
instruction names its region, and when a static slot may stand in. Two nets keep
a mis-resolution from silently becoming a use-after-free.

A region owns pages and carries one `u32` reference count. RC starts at **1** —
the compiler's initial reference, that is, the TT `letregion` owner.
Cross-region references raise it above 1.

- `IncrefRegion` raises RC. The runtime also auto-increfs at two points: scanning
  an immutable object's contents at allocation (`alloc_obj`), and storing into a
  mutable container at runtime.
- `DecrefRegion` lowers RC. At 0 the region's pages return to the pool **and**
  every region its contents reference is decremented (the cascade), recursively.
- A region frees only when a decref takes its count to 0 — `DecrefRegion` or its
  value- and cell-resolved variants — or when `FreeRegionGroup` drops a co-owned
  group whole ([ownership.md](ownership.md)). There is no `FreeRegion`.

A value may escape into a container, a closure, or a yielded signal. Its
region's RC was then already raised at the escape site, so the `DecrefRegion`
at its `decref_point` drops the initial reference without freeing. The region
lives exactly as long as RC says. There is no promotion pass that moves a value
to a longer-lived region; the value is born in the right region (Rule 3) and RC
tracks the rest.

## Two resolutions: value-resolved and slot-resolved

Each region-RC instruction names its region in one of two ways:

- **value-resolved** (`IncrefValueRegion`/`DecrefValueRegion`): the operand is a
  *register*; the runtime reads the value and asks `region_of` for its physical
  region. This is the honest encoding whenever the region cannot be named at
  compile time: a passed-through arg, a branch-dependent mix, an opaque call
  result, a runtime mutable store. The prediction-free calling convention is
  built on it. A callee hands its caller one owning reference to the result's
  *runtime* region (`IncrefValueRegion` at every tail). The caller consumes it
  (`DecrefValueRegion` at the result's `decref_point`), and neither side names
  the other's region statically.
- **slot-resolved** (`IncrefRegion`/`DecrefRegion`): the operand is a
  *`StaticRegion` slot*; the runtime resolves it through the current activation's
  `activation_region_map` to the physical region this execution minted for that
  slot. Usable only where the region is statically known.

## A call's result is named by the call's own region

Nothing of the callee's *interior* naming crosses the call boundary. Every call
node mints one `call_r`, the caller-side name for "whatever region the returned
value turns out to live in". Its release is the value-resolved route above, so a
static region of the callee is never named in the caller.

That holds however much of the callee this compilation can see. The walk
**inlines** a resolvable lambda callee's body (`region::infer::walk::inline`),
so the intrinsics buried inside it record cross-region edges over *this* call's
arguments, keyed at their own sites in the callee body. That is the whole reason
the inline exists. The regions that walk yields are the callee's, minted against
the callee's own nodes and remapped to fresh physical regions per activation. So
they are discarded: the caller's binding for the result holds `call_r`, exactly
as it does for an opaque callee.

Letting them through instead makes the caller a nominal holder of a region it never
allocates, and the `decref_point` machinery reads that fiction as fact:

- the holder's uses **extend the region's `decref_point`** into the caller. Where the
  caller's use sits on a path that never runs the allocation, the region's one
  release is emitted on that path, and the allocating path emits none at all. The
  value route loads a slot holding `nil` there, so the release is inert as well as
  misplaced and the region is held to fiber teardown;
- the region gains a **second holder binding**, which disqualifies it from the
  single-holder value route `region::infer::compensate` needs, so the per-arm compensation
  that would otherwise cover the allocating arm declines as well.

This is the result-side half of one rule. The argument-side half is
`inline_bound_regions`, which keeps a `Return`/`Break` reached *inside* an inline from
extending a **caller** region's `decref_point` onto a callee node. Both say the same
thing: an inline is a device for collecting edges, not a splice, and the two
activations' namings must not mix.

A call to a lambda whose body the walk is already inside is not inlined: a
self call, or a call that closes a mutual recursion. The re-walk names each
allocation by the region its site already has, and for such a callee that
region is one the walk is using for the current activation. An edge from an
argument into it reads as containment between two regions of one activation.
In `(go (%rest lst) (%pair (%first lst) (%pair 0 acc)))` the argument is the
outer pair, and the re-walk stores it into the inner pair's site. The graph
then holds a cycle between the two pairs, and the co-owned group free
([ownership.md](ownership.md)) frees both before the call that receives the
outer one. Such a call takes the opaque path instead.

`region::infer::tests::inline::*` pins the naming.
[region-inline-result-naming.lisp](../../../tests/impl/region-inline-result-naming.lisp)
is the leak face, and
[region-inline-result-naming-uaf.lisp](../../../tests/impl/region-inline-result-naming-uaf.lisp)
is the soundness complement. The caller holds exactly one release for the
result, so everything the callee hands back that is not freshly its own must
ride a counted edge.

## A spliced call's arguments come out of an array the convention owns

A call with a spliced argument — `(f ;args)`, and the `apply` it underlies — cannot
push its arguments onto the operand stack, because how many there are is a runtime
fact. So the lowerer builds them into a fresh `@array` and hands that array to
`CallArrayMut` / `TailCallArrayMut`, which spreads it across the callee's parameters.
The array is the operand stack spelled as a heap value: the calling convention builds
it, the call consumes it, and no binding of the program ever names it.

Three rules follow, and each is the spliced half of a pair the plain call already
states.

**The array has a region of its own.** `MakeArrayMut` and the call are two
allocations. A static region slot names one allocation execution between drops
([the region model](model.md)), so the array takes a managed slot of its own
rather than the call's. Sharing the call's slot orphans the array's physical
region the moment the call maps its own mint over it. No route can name an
orphan afterwards: not the value route, which needs a binding, and not the
abandoned-frame walk, which reads the slot.

**The call releases the array.** The array dies where the arguments are read
out of it, which is *inside* the call. No instruction can follow that point,
because a frame-replacing tail callee never arrives at the block after
`TailCallArrayMut`. So the release is the runtime's: each dispatcher takes the
array's slot and frees the region once the callee holds what it needs. Taking
the slot is what keeps the release single. A frame may raise between the array's
construction and the call, since an `ArrayMutExtend` over a source that is not a
sequence raises there. That frame parks with the slot still mapped. A restart
runs on to the call, which takes the slot, and a discard's walk reclaims it. A
frame that reached the call has nothing mapped for either to find.

**The callee mints its own reference to every argument.** The store funnel
counts each `ArrayMutPush` / `ArrayMutExtend`, so the array holds one counted
reference per element and freeing it cascades that reference away. A spliced
call therefore hands the callee nothing of the frame's. The callee mints one
owning reference per parameter (`own_params`) in tail position exactly as in
call position. `tail_arg_is_borrowed` gives the same answer for a captured
upvalue, for the same reason: the reference belongs to the holder that built
it, not to this activation.

So a spliced tail call **moves nothing**. Every release the frame owes is
relocated ahead of the frame replacement ([the relocation](relocate.md)) instead
of being exempted as an ownership move. Reading a spliced argument as a moved
operand instead strands one region per call for each source the splice read. It
also leaves the freed element still named by a source the frame never released.

[region-splice-args.lisp](../../../tests/impl/region-splice-args.lisp) pins one
bounded rate per callee kind and call position.
[region-splice-args-uaf.lisp](../../../tests/impl/region-splice-args-uaf.lisp)
is the soundness complement under `--trace=guardfree`: the release the array's
reclaim adds must not reach a value the callee still reads.

## The return mint is emitted exactly once

The callee half of that convention is **one** mint per returned value: a function
hands its caller exactly one owning reference, and the caller's single
`DecrefValueRegion` at the result's `decref_point` consumes it. Two lowering
sites can supply it, and which one applies is decided by whether the result is
*named*:

- **the `Return` mint** (`lower_return`, marked on the HIR by
  [return_incref.rs](../../../src/hir/return_incref.rs)) — the named path. ANF
  binds the tail value to a synthetic slot, so the frame holds its own
  reference. The mint raises RC. The binding's `decref_point`, extended past the
  mint by `return_sites`, drops the frame's reference and leaves net one for the
  caller.
- **the `TailCall` fall-through retain** (`lower_call`'s tail arm) — the
  anonymous path. A *native* tail call pushes no bytecode frame, so on normal
  completion the dispatch loop runs the post-`TailCall` block before the
  enclosing lambda returns. In a **propagating** tail position (a `let`/`lambda`
  body, which ANF deliberately leaves unnamed) there is no binding, hence no
  `decref_point` to balance a `Return` mint — the fall-through retain *is* the
  mint.

They cover the same value whenever ANF *does* name a tail call's result — the
canonical wrap `(let [t (f …)] (return t))`, which ANF builds for a tail call
nested in a `begin`/`if`/`cond`/`match` arm. Emitting both retains the result
twice against one release: an over-keep of one region per call, growing per
loop iteration. So the fall-through retain **stands down** whenever a `Return`
mint covers the same result (`return_minted_calls`), and the named path's
mint-then-release accounting carries the convention alone. A frame-replacing
*closure* tail call reaches neither instruction (the callee emits its own
`Return` mint), so the rule is uniform over callee kinds.

Two narrower sites already suppress the fall-through retain for the same
"exactly one reference" reason, and are unaffected. One is a `-mut`
pass-through store/remove funnel whose dispatch wrapper released the container
owned-param reference here (`container_release_sites`). The other is a
moves-out ∩ `PassThrough` native whose in-body escape retain is already the
caller's reference (`moves_out_release_sites`).

The pinning tests are
[region-native-tail-compound-leak.lisp](../../../tests/impl/region-native-tail-compound-leak.lisp) (the
per-shape region-count deltas: bare, `let`-body, `begin`-nested, `if`-nested,
over Fresh / Funnel / pass-through natives) and
[region-native-tail-return-uaf.lisp](../../../tests/impl/region-native-tail-return-uaf.lisp) /
[region-hof-tail-return-uaf.lisp](../../../tests/impl/region-hof-tail-return-uaf.lisp) (the soundness complement — the anonymous
path must keep its retain).

## Where the rest of the argument lives

This document holds the instruction machinery: how a release names its region,
and the compile-time selection that decides between the two namings. Where a
release is PLACED, and what each placement owes, is argued next door.

- [Where a release is anchored](anchors.md) — the pins that set a region's
  `decref_point`: a binder's init, a binder's scope, and the two things a
  `break` does to a release.
- [The branch-arm release window](window.md) — the one release of a region
  several arms use, anchored where every path reaches it.
- [The frame-held admission](held.md) — which regions a frame holds alone, so
  a release may fire where none fired before.
- [Per-arm compensation](compensate.md) — the counted routes a branch falls
  back to when the window declines.
- [A release past a frame-replacing tail call](relocate.md) — why the block
  after a `TailCall` is dead on the closure path.
- [The relocation point and its replicas](replicate.md) — how one release
  covers a merge and every path that leaves before it.
- [What a signal exit owes](signalexit.md) — the release a native's
  fall-through block would have run.
- [An abandoned frame runs the releases it still owes](unwind.md) — the two
  tables a call boundary, a refusing host or a discharge walks.

Those arguments were sections of this document, and comments across the tree
still name them. Each title below resolves in one step, so a pointer written
against the old shape reaches its argument rather than a document that has
stopped making it.

| Argument | Now in |
|---|---|
| A binder's init release lands after the slot store | [anchors.md](anchors.md) |
| Every binder records its scope | [anchors.md](anchors.md) |
| `break` transfers its value; it does not consume it | [anchors.md](anchors.md) |
| A break out of a TAIL block carries the return mint | [anchors.md](anchors.md) |
| A release the break jumps over is not a release | [anchors.md](anchors.md) |
| A release inside one arm is not a release on the other arms | [window.md](window.md) |
| An arm is a conditional position, not a syntactic arm body | [window.md](window.md) |
| The admission: this frame must be the region's only holder | [held.md](held.md) |
| The return facet costs the merge nothing | [held.md](held.md) |
| Lexical capture is not a second holder to fear | [held.md](held.md) |
| A fiber crossing is a counted holder too | [held.md](held.md) |
| A mutated holder poisons its value route, not its cell box | [window.md](window.md) |
| The boundaries | [window.md](window.md) |
| An arm that leaves through a callee takes a replica, not the anchor | [window.md](window.md) |
| The return frontier is per-path | [compensate.md](compensate.md) |
| A compensating release of an env cell names the box, not the holder's slot | [compensate.md](compensate.md) |
| A release past a frame-replacing tail call is not a release | [relocate.md](relocate.md) |
| The callee's return mint, and why the point owes it nothing | [relocate.md](relocate.md) |
| A compiled capture cell is frame-held exactly as its binding is | [relocate.md](relocate.md) |
| A move that crosses a read through the cell it frees is declined | [relocate.md](relocate.md) |
| What the exemption keeps, a channel must still run | [relocate.md](relocate.md) |
| A collector parameter takes the moved reference over itself | [relocate.md](relocate.md) |
| The relocation point outlives the block, and a branch merge inherits it | [replicate.md](replicate.md) |
| Self-cancelling is a property of the ROUTE, not of the region's class | [replicate.md](replicate.md) |
| What the fall-through owes, a signal exit owes too | [signalexit.md](signalexit.md) |
| A carrier that comes back with a result never left the frame | [signalexit.md](signalexit.md) |
| An abandoned frame runs the releases it still owes | [unwind.md](unwind.md) |
| A squelch boundary abandons frames the same way, so it runs the same walk | [unwind.md](unwind.md) |

## Compile-time region selection (coalescing)

The compiler may prove that a value is a **fresh local allocation whose region
is a known slot**. That means the value was allocated in this function:
`alloc_region` has an entry, or for a returned binding `binding_source_regions`
resolves to one such region. The region is also `live`, and it is none of the
dynamic classes below. The compiler then substitutes the
slot-resolved `IncrefRegion` for the value-resolved `IncrefValueRegion`, and
likewise on the decref side.

This is **instruction selection, not a change of RC unit**. Through the
activation map, the slot resolves to the *same physical region*
`region_of(value)` would return. The allocation stamped that slot to that
region, and a value never moves regions. So every region's RC trajectory is
bit-identical, and leak counts and teardown residue are unchanged *by
construction*. The win is one fewer runtime deref per coalesced site. The
slot-resolved form also touches no operand stack: the value register stays on
top as the return value.

The substitution is **purely callee-mint-side** for the return convention: the
caller still cannot name the callee's region, so the caller's balancing
`DecrefValueRegion` stays value-resolved. The pervasive coalescible site is the
prediction-free return mint at every function tail. The two narrower sites are
both reassigned-binding traffic over a value the lowerer just allocated
locally:

- the **reassign incref-on-store** (`lower_assign`'s drop-on-overwrite, pinning
  a 1-slot container's new content). It coalesces only for a *fn-local*
  container. A *module-scope* container's value is in
  `mutated_binding_value_regions` and stays value-resolved.
- the decref-side **captured-reassign init-drop** (`store_captured_cell_init`,
  dropping the producer's reference to a captured binding's fresh init value,
  `DecrefValueRegion` → `DecrefRegion`).

All three sites reach the same `coalescible_region` predicate. Its
runtime-population guard requires the region's slot to be stamped by an
allocation emitted in this function, so it refuses any captured or
cross-thread value at every site alike.

The reduction this buys is *measured, not asserted*. It counts coalesced
(slot-resolved) against value-resolved mints at the candidate sites, plus the
self-edges eliminated below. The lowerer records each decision in the
thread-local instrument `lir::lower::rcstats`, because the final LIR does not
show the choice. A coalesced mint's `IncrefRegion` is indistinguishable from a
store-edge's, and an eliminated self-edge leaves no instruction.
[benches/regionrc.rs](../../../benches/regionrc.rs) reports the totals across
the stdlib load and both Elle suites, [tests/lang](../../../tests/lang/overview.md)
and [tests/impl](../../../tests/impl/overview.md).

## The dynamic boundary (stays value-resolved)

These sites are genuinely runtime facts and must **never** coalesce — the region
is not knowable at compile time:

| Site / class | Why it stays value-resolved |
|---|---|
| caller-side `DecrefValueRegion` of a call result | the caller cannot name the callee's region — prediction-free by design |
| tail borrowed-arg incref (`tail_arg_is_borrowed`) | a captured upvalue / env **cell** region — dynamic |
| tail native-result retain | pass-through native result; region named only at runtime |
| reassign drop-old (`DecrefValueRegion{old_reg}`) | the displaced 1-slot-container content — the runtime fact the container tracks |
| `Mixed`/`Unknown` `RegionEffect` results | region unknown; the clique is a may-store over-keep |
| pass-through natives (`first`/`rest`/`get`) | result lives in an arg's region, named only at runtime |
| capture cells (`cell_release_regions`, `DecrefCellRegion`) | release frees the *cell's* own region, not the inner value's |
| phantom param regions | no `alloc_here`, filtered from `live_regions` — runtime-counted |
| suspended frames | `activation_region_map` captured/restored across resume; the slot is per-activation |
| terminal fiber signals | set-once park-retain, no compile-time edge |
| runtime mutable-store traffic | the `push_with_incref` funnel counts at the store site (the TT gap is dynamic) |
| ownership-forest ops (`AdoptRegion`, `FreeRegionGroup`, `AdoptIntoActivation`) | a forest member is a runtime fact (a call-result / cross-activation region); `AdoptIntoActivation`'s parent — the activation's pages-less owner node — has no slot at all ([owner nodes](owner.md)) |

## A `%pair` edge takes no static incref

`emit_increfs_for` emits one `IncrefRegion(source)` per cross-region store edge
`(site, source, target)` — a value in `source` stored into a structure in
`target` — balanced by `target`'s free-time cascade at `DecrefRegion(target)`.

A `%pair` car or cdr edge takes none. Its target is the pair allocated at that
very site (`RegionInfo::alloc_counts_edge`), and the allocation counts the
reference itself: `handle_list` allocates through `alloc_obj`, whose scan
increfs each cross-region field and records the edge the cascade decrefs. A
static `IncrefRegion` would count that one reference twice against one
release, and leak the source once per pair built. The edge stays in
`cross_region_refs`, because the merge seed and the ownership forest read it
as containment. The callee-inline re-walk records a known callee's own `%pair`
edges a second time, at the same sites, and those take no incref either.

A merged pair hides the count, because one region holds both ends. So does a
call result in the car, whose slot-resolved incref names a slot nothing
populates. [region-pair-edge-leak.lisp](../../../tests/impl/region-pair-edge-leak.lisp)
pins the rate on a nest the merge seed refuses.

## Self-edge elimination

The free-time cascade **skips self-references**
([introspect.rs](../../../src/value/fiberheap/regionpool/introspect.rs) decrefs a
referenced region only when `rid != own_id`). So a `source == target` self-edge
`R→R` has no balancing decref: keeping its `IncrefRegion(R)` **leaks** `R`.
Eliminating a self-edge is therefore the sound transform — the compiler-side
mirror of the cascade's own `own_id` self-skip. Among the edges that take an
incref, it is the *only* redundant case:

- **alias edges** — a repeated stored value emits N edges into a *distinct*
  target; the cascade finds N references and decrefs N times, so all N
  increfs are required. Collapsing them is an over-collapse UAF.
- **may-store clique edges** — over-approximations whose balancing decref is the
  target's runtime content scan (per *actual* store, not per emitted edge).
  Eliminating them trades a known leak for a possible UAF.

A self-edge appears only when a region **merge** puts a store edge's source and
target in one region; see [merging](merging.md). The `%pair` edge a merge
collapses is one, and it takes no incref either way (above). The compiler
detects one with
`RegionInfo::is_merge_self_edge` — `merged_root(source) == merged_root(target)`
over the merge forest — which is exactly the slot coincidence the merged
allocation resolves to (`static_slot` canonicalizes through that forest). When
the predicate fires, `emit_increfs_for` **drops** the `IncrefRegion` rather than
emitting it. The detection isolates the redundant self-edge from the two
must-keep classes above by construction. The merge seed never collapses an
escaping alias (it is not sole-held) nor a clique edge (it is not a `%pair`
immutable store), so their endpoints keep distinct merge roots.

This elimination belongs with the merge's allocation canonicalization and
child-decref suppression ([merging](merging.md)). A merge that kept the
increment would leak its region, so the elimination fires wherever the merge
forest puts an edge's two ends in one region.

Its correctness net is not a per-edge runtime assert: once both endpoints share
a slot, a slot-vs-slot check is a tautology. The net is the compile-time
decref-dominance assertion, exactly one `DecrefRegion` per merged slot
(`record_merged_slots`). `--trace=guardfree` over the builder corpus completes
it: an over-collapse surfaces as a UAF, and a self-edge left in place grows the
live region count.
[region-merge-builder-loop.lisp](../../../tests/impl/region-merge-builder-loop.lisp)
is the canonical pinning test.

## The equivalence oracle

A mis-coalesce is a use-after-free: a slot resolved to the wrong physical region
makes its cascade free a live region. The net for a coalesced *mint* (the
value→slot substitution, the region selection above) is the debug-only
`AssertRegionMatches { region_id, src }`, emitted immediately before every
coalesced `IncrefRegion`. Self-edge *elimination* carries no coalesced incref to
guard; its net is the decref-dominance assertion and guardfree, above.

In the bytecode interpreter it panics when
`activation_region_map.resolve(region_id) != region_of(src)`. That turns an
inference bug into a deterministic panic at the exact instruction, under the
trustworthy guardfree oracle, instead of a later heap corruption. It mirrors the
native-effect declaration oracle ([effects.md](effects.md)).

Release builds and the JIT/WASM tiers treat it as a no-op. The GPU tiers
exclude any function carrying it through the `is_gpu_instruction` whitelist.
Those tiers' coalesced sites are covered instead by the language suite each build
runs, the rig profile that compiles every function on its first call, and the
escape golden. The instruction renders into no
`[region_instrs]` golden line, since it is scaffolding, not part of the
semantic RC stream.
