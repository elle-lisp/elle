# The mechanism

<!-- audited: 2026-09-05 -->

The RC-instruction machinery the [rules](rules.md) constrain: how each
instruction names its region, and when a static slot may stand in. Two nets keep
a mis-resolution from silently becoming a use-after-free.

A region owns pages and carries one `u32` reference count. RC starts at **1** —
the compiler's initial reference, i.e. the TT `letregion` owner. Cross-region
references raise it above 1.

- `IncrefRegion` raises RC. The runtime also auto-increfs at two points: scanning
  an immutable object's contents at allocation (`alloc_obj`), and storing into a
  mutable container at runtime.
- `DecrefRegion` lowers RC. At 0 the region's pages return to the pool **and**
  every region its contents reference is decremented (the cascade), recursively.
- `DecrefRegion` is the only demise instruction. There is no `FreeRegion`.

If a value escapes — into a container, a closure, a yielded signal — its region's
RC was already raised at the escape site, so the `DecrefRegion` at its `decref_point`
drops the initial reference without freeing. The region lives exactly as long as
RC says. There is no promotion pass that moves a value to a longer-lived region;
the value is born in the right region (Rule 3) and RC tracks the rest.

## Two resolutions: value-resolved and slot-resolved

Each region-RC instruction names its region in one of two ways:

- **value-resolved** (`IncrefValueRegion`/`DecrefValueRegion`): the operand is a
  *register*; the runtime reads the value and asks `region_of` for its physical
  region. This is the honest encoding whenever the region cannot be named at
  compile time — a passed-through arg, a branch-dependent mix, an opaque call
  result, a runtime mutable store. The prediction-free calling convention is
  built on it: a callee hands its caller one owning reference to the result's
  *runtime* region (`IncrefValueRegion` at every tail) and the caller consumes it
  (`DecrefValueRegion` at the result's `decref_point`), neither side naming the
  other's region statically.
- **slot-resolved** (`IncrefRegion`/`DecrefRegion`): the operand is a
  *`StaticRegion` slot*; the runtime resolves it through the current activation's
  `activation_region_map` to the physical region this execution minted for that
  slot. Usable only where the region is statically known.

## A call's result is named by the call's own region

Nothing of the callee's *interior* naming crosses the call boundary. Every call node
mints one `call_r` — the caller-side name for "whatever region the returned value
turns out to live in" — and its release is the value-resolved route above, so a
static region of the callee is never named in the caller.

That holds however much of the callee this compilation can see. The walk **inlines**
a resolvable lambda callee's body (`regions::walk::inline`) so the intrinsics buried
inside it record their cross-region edges at *this* call site — the whole reason the
inline exists. The regions that walk yields are the callee's, minted against the
callee's own nodes and remapped to fresh physical regions per activation, so they are
discarded: the caller's binding for the result holds `call_r`, exactly as it does for
an opaque callee.

Letting them through instead makes the caller a nominal holder of a region it never
allocates, and the `decref_point` machinery reads that fiction as fact:

- the holder's uses **extend the region's `decref_point`** into the caller. Where the
  caller's use sits in a branch arm mutually exclusive with the arm that does allocate
  the region — the base case of `(if p (mk …) (go …))`, whose recursive call inlines
  the same body and so yields the base arm's own result region — the region's one
  release is emitted on the only path that never mints it, and the allocating path
  emits none at all. The value route loads a slot holding `nil` there, so the release
  is inert as well as misplaced and the region is held to fiber teardown;
- the region gains a **second holder binding**, which disqualifies it from the
  single-holder value route `regions::compensate` needs, so the per-arm compensation
  that would otherwise cover the allocating arm declines as well.

This is the result-side half of one rule. The argument-side half is
`inline_bound_regions`, which keeps a `Return`/`Break` reached *inside* an inline from
extending a **caller** region's `decref_point` onto a callee node. Both say the same
thing: an inline is a device for collecting edges, not a splice, and the two
activations' namings must not mix.

Pinned by `regions::tests::inline::*`, the leak face
`tests/elle/region-inline-result-naming.lisp`, and the soundness complement
`region-inline-result-naming-uaf.lisp` — the caller holds exactly one release for the
result, so everything the callee hands back that is not freshly its own must ride a
counted edge.

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
allocations, and a static region slot names one allocation execution between drops
([the region model](model.md)), so the array takes a managed slot
of its own rather than the call's. Sharing the call's slot orphans the array's
physical region the moment the call maps its own mint over it, and no route can name
an orphan afterwards — not the value route, which needs a binding, and not the
abandoned-frame walk, which reads the slot.

**The call releases the array.** The array dies where the arguments are read out of
it, which is *inside* the call — a point no instruction can follow, because a
frame-replacing tail callee never arrives at the block after `TailCallArrayMut`. So
the release is the runtime's: each dispatcher takes the array's slot and frees the
region once the callee holds what it needs. Taking the slot is what keeps the release
single. A frame abandoned between the array's construction and the call — an
`ArrayMutExtend` over a source that is not a sequence raises there — still has the
slot mapped, so the walk reclaims it, and a frame that reached the call does not.

**The callee mints its own reference to every argument.** The store funnel counts each
`ArrayMutPush` / `ArrayMutExtend`, so the array holds one counted reference per
element and freeing it cascades that reference away. A spliced call therefore hands
the callee nothing of the frame's, and the callee mints one owning reference per
parameter (`own_params`) in tail position exactly as in call position — the same
answer `tail_arg_is_borrowed` gives for a captured upvalue, and for the same reason:
the reference belongs to the holder that built it, not to this activation. So a
spliced tail call **moves nothing**, and every release the frame owes is relocated
ahead of the frame replacement ([the relocation](relocate.md)) instead of being
exempted as an ownership move. Reading a spliced
argument as a moved operand instead strands one region per call for each source the
splice read, and leaves the freed element still named by a source the frame never
released.

Pinned by `tests/elle/region-splice-args.lisp` — one bounded rate per callee kind and
call position — and by the soundness complement `region-splice-args-uaf.lisp` under
`--trace=guardfree`, where the release the array's reclaim adds must not reach a value
the callee still reads.

## The return mint is emitted exactly once

The callee half of that convention is **one** mint per returned value: a function
hands its caller exactly one owning reference, and the caller's single
`DecrefValueRegion` at the result's `decref_point` consumes it. Two lowering
sites can supply it, and which one applies is decided by whether the result is
*named*:

- **the `Return` mint** (`lower_return`, marked on the HIR by
  `hir/return_incref.rs`) — the named path. ANF binds the tail value to a
  synthetic slot, so the frame holds its own reference; the mint raises RC and
  the binding's `decref_point` — extended past the mint by `return_sites` — drops
  the frame's reference, leaving net one for the caller.
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
"exactly one reference" reason, and are unaffected: a `-mut` pass-through
store/remove funnel whose dispatch wrapper released the container owned-param
reference here (`container_release_sites`), and a moves-out ∩ `PassThrough`
native whose in-body escape retain is already the caller's reference
(`moves_out_release_sites`).

The pinning tests are `tests/elle/region-native-tail-compound-leak.lisp` (the
per-shape region-count deltas: bare, `let`-body, `begin`-nested, `if`-nested,
over Fresh / Funnel / pass-through natives) and `region-native-tail-return-uaf.lisp`
/ `region-hof-tail-return-uaf.lisp` (the soundness complement — the anonymous
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
- [Per-arm compensation](compensate.md) — the counted routes a branch falls
  back to when the window declines.
- [A release past a frame-replacing tail call](relocate.md) — why the block
  after a `TailCall` is dead on the closure path.
- [The relocation point and its replicas](replicate.md) — how one release
  covers a merge and every path that leaves before it.
- [What a signal exit owes](signalexit.md) — the release a native's
  fall-through block would have run.
- [An abandoned frame runs the releases it still owes](unwind.md) — the two
  tables an error, a squelch boundary or a discard walks.

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
| The admission: this frame must be the region's only holder | [window.md](window.md) |
| The return facet costs the merge nothing | [window.md](window.md) |
| Lexical capture is not a second holder to fear | [window.md](window.md) |
| A fiber crossing is a counted holder too | [window.md](window.md) |
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

Where the compiler can prove a value is a **fresh local allocation whose region
is a known slot** — the value was allocated in this function (`alloc_region` has
an entry, or for a returned binding `binding_source_regions` resolves to one such
region), that region is `live`, and it is none of the dynamic classes below — it
substitutes the slot-resolved `IncrefRegion` for the value-resolved
`IncrefValueRegion` (and likewise on the decref side). This is **instruction
selection, not a change of RC unit**: the slot resolves — through the activation
map — to the *same physical region* `region_of(value)` would return, because the
allocation stamped that slot to that region and a value never moves regions. So
every region's RC trajectory is bit-identical and leak counts and teardown
residue are unchanged *by construction*. The win is one fewer runtime deref per
coalesced site, and the slot-resolved form touches no operand stack (the value
register stays on top as the return value — stack-neutral).

The substitution is **purely callee-mint-side** for the return convention: the
caller still cannot name the callee's region, so the caller's balancing
`DecrefValueRegion` stays value-resolved. The pervasive coalescible site is the
prediction-free return mint at every function tail. The two narrower sites are
both reassigned-binding traffic over a value the lowerer just allocated locally:
the **reassign incref-on-store** (`lower_assign`'s drop-on-overwrite — pinning a
1-slot container's new content; coalesces only for a *fn-local* container, since a
*module-scope* container's value is in `mutated_binding_value_regions` and stays
value-resolved), and the decref-side **captured-reassign init-drop**
(`store_captured_cell_init` — dropping the producer's reference to a captured
binding's fresh init value, `DecrefValueRegion` → `DecrefRegion`). Both reach the
same `coalescible_region` predicate, so the runtime-population guard (the region's
slot must be stamped by an allocation emitted in this function) refuses any
captured/cross-thread value at all three sites alike.

The reduction this buys — coalesced (slot-resolved) versus value-resolved mints at
the candidate sites, plus the self-edges eliminated below — is *measured, not
asserted*: the lowerer records each decision in the thread-local instrument
`lir::lower::rcstats` (the choice is not recoverable from the final LIR — a
coalesced mint's `IncrefRegion` is indistinguishable from a store-edge's, and an
eliminated self-edge leaves no instruction), and `benches/regionrc.rs` reports the
totals across the stdlib load and the `tests/elle` corpus.

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

## Self-edge elimination

`emit_increfs_for` emits one `IncrefRegion(source)` per cross-region store edge
`(site, source, target)` — a value in `source` stored into a structure in
`target` — balanced by `target`'s free-time cascade at `DecrefRegion(target)`.
The cascade **skips self-references** (`regionpool/introspect.rs` decrefs a
referenced region only when `rid != own_id`). So a `source == target` self-edge
`R→R` has no balancing decref: keeping its `IncrefRegion(R)` **leaks** `R`.
Eliminating a self-edge is therefore the sound transform — the compiler-side
mirror of the cascade's own `own_id` self-skip. It is the *only* redundant case:

- **alias edges** — `(%pair x x)` and repeated-arg shapes emit N edges into a
  *distinct* target; the cascade finds N references and decrefs N times, so all N
  increfs are required. Collapsing them is an over-collapse UAF.
- **may-store clique edges** — over-approximations whose balancing decref is the
  target's runtime content scan (per *actual* store, not per emitted edge).
  Eliminating them trades a known leak for a possible UAF.

A self-edge appears only when a region **merge** collapses a store edge's source
and target into one region (a value merged into the aggregate it is stored into);
see [merging](merging.md). The compiler detects one with
`RegionInfo::is_merge_self_edge` — `merged_root(source) == merged_root(target)`
over the merge forest — which is exactly the slot coincidence the merged
allocation resolves to (`static_slot` canonicalizes through that forest). When the
predicate fires, `emit_increfs_for` **drops** the `IncrefRegion` rather than
emitting it; the detection isolates the redundant self-edge from the two must-keep
classes above by construction, because the merge seed never collapses an escaping
alias (it is not sole-held) nor a clique edge (it is not a `%pair` immutable
store), so their endpoints keep distinct merge roots.

This elimination is half of one mechanism with the merge's allocation
canonicalization and child-decref suppression ([merging](merging.md)): a
self-edge dropped without the
merge frees early, and a merge without the drop leaks, so neither side is emitted
without the other. Its correctness net is not a per-edge runtime assert — once both
endpoints share a slot, a slot-vs-slot check is a tautology — but the compile-time
decref-dominance assertion (exactly one `DecrefRegion` per merged slot,
`record_merged_slots`) together with `--trace=guardfree` over the builder corpus
(an over-collapse surfaces as a UAF; a self-edge left in place grows the live
region count). The pinning test is the canonical reference
(`tests/elle/region-merge-builder-loop.lisp`).

## The equivalence oracle

A mis-coalesce is a use-after-free: a slot resolved to the wrong physical region
makes its cascade free a live region. The net for a coalesced *mint* (the
value→slot substitution, [region selection](mechanism.md)) is the debug-only
`AssertRegionMatches { region_id, src }`, emitted immediately before every
coalesced `IncrefRegion`. (Self-edge *elimination* carries no coalesced incref
to guard — its net is the decref-dominance assertion and guardfree, � "".) In
the bytecode interpreter it panics when
`activation_region_map.resolve(region_id) != region_of(src)` — turning an
inference bug into a deterministic panic at the exact instruction, under the
trustworthy guardfree oracle, instead of a later heap corruption (the mirror of
the native-effect declaration oracle, [effects.md](effects.md)). Release builds
and the JIT/WASM tiers treat it as a no-op (the GPU tiers exclude any function
carrying it via the `is_gpu_instruction` whitelist); their coalesced sites are
covered instead by the runner's cross-tier divergence detection and the escape
golden. The instruction renders into no `[region_instrs]` golden line — it is
scaffolding, not part of the semantic RC stream.

