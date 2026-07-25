# The mechanism

The RC-instruction machinery the [rules](rules.md) constrain: how each
region-RC instruction names its region, when the compiler may resolve it to a
static slot instead of a runtime value, and the two nets that keep a
mis-resolution from silently becoming a use-after-free.

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

## `break` transfers its value; it does not consume it

A `Return` hands a value across a *function* frontier. A `break` is the
intra-function dual: it hands a value to the enclosing `block`, whose value is
its fall-through value **or** the value of any `break` targeting it. No
reference changes hands — the value stays in the same activation — so there is
no mint. What a `break` does change is *where the value dies*, and by two
compounding facts, neither of which the ordinary consuming-node treatment
(Rule 4) covers:

- `break` lowers to a store into the block's result slot plus a **jump to the
  block's exit label**. Control leaves the body there, so a release the lowerer
  placed at a `decref_point` inside the body is emitted into the break's
  unreachable fall-through and never executes at all. Treating `Break` as a
  consumer of its operand anchors exactly there, and the value is then held to
  fiber teardown — one region per break.
- The block's own exit label is not late enough either: the block's value may
  flow straight into a consumer (`(f (block … (break v) …))`), and releasing at
  the exit frees it under that consumer.

So the transfer is stated as two facts, both over structures the solver already
holds:

- **Region flow** (`hir/regions/walk`, the `Block`/`Break` arms): a `Block`'s
  result region set is the union of its fall-through value's regions and every
  targeting `break`'s value regions. A binding that names the block's value
  therefore names those regions, and the ordinary binding-chain `decref_point`
  extension carries the release past the binding's own last use — which is what
  keeps `(let [r (block … (break v) …)] (use r))` from freeing `v` under `use`.
- **The break pin** (`regions/analyze/decref.rs`, the dual of `return_sites`):
  each broken region's `decref_point` is extended to `last_use[block]` — the
  node that consumes the block's value, or the `Block` itself when nothing does.
  The lowerer emits a node's decrefs *after* it, and for the `Block` that is
  after the exit label, so the one release fires on the break path and the
  fall-through path alike. Every `decref_point` rule is a max, so a later
  binding-chain or return extension still wins.

The lowerer needs no new instruction and no compensating release at the break
site. On a path that did not run the break, the value-route reloads a slot that
still holds `nil` and the release no-ops — the same nil-stamp discipline the
branch-union release relies on.

A region allocated inside the body whose value is *not* the one broken out —
its `decref_point` sits between the break site and the block exit — is still
skipped on the break path; that residue is measured by the `break-skipped`
probe in `tests/elle/oracle.lisp`. Pinned here:
`tests/elle/region-break-transfer.lisp` (the reclamation), the `break-value*`
probes (the rates), `regions::tests::blocks` (the placement, structurally), and
`region-break-transfer-uaf.lisp` (the soundness complement — a value broken out
and read afterwards, stored, or returned must survive).

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
| ownership-forest ops (`AdoptRegion`, `FreeRegionGroup`, `AdoptIntoActivation`) | a forest member is a runtime fact (a call-result / cross-activation region); `AdoptIntoActivation`'s parent — the activation's pages-less owner node — has no slot at all (owner.md § "Owner nodes") |

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
see [merging.md](merging.md) § Merging. The compiler detects one with
`RegionInfo::is_merge_self_edge` — `merged_root(source) == merged_root(target)`
over the merge forest — which is exactly the slot coincidence the merged
allocation resolves to (`static_slot` canonicalizes through that forest). When the
predicate fires, `emit_increfs_for` **drops** the `IncrefRegion` rather than
emitting it; the detection isolates the redundant self-edge from the two must-keep
classes above by construction, because the merge seed never collapses an escaping
alias (it is not sole-held) nor a clique edge (it is not a `%pair` immutable
store), so their endpoints keep distinct merge roots.

This elimination is half of one mechanism with the merge's allocation
canonicalization and child-decref suppression (merging.md § "Emission: one
slot per merge tree, one demise at the root"): a self-edge dropped without the
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
value→slot substitution, § "Compile-time region selection") is the debug-only
`AssertRegionMatches { region_id, src }`, emitted immediately before every
coalesced `IncrefRegion`. (Self-edge *elimination* carries no coalesced incref to
guard — its net is the decref-dominance assertion and guardfree, § "Self-edge
elimination".) In the bytecode interpreter it panics when
`activation_region_map.resolve(region_id) != region_of(src)` — turning an
inference bug into a deterministic panic at the exact instruction, under the
trustworthy guardfree oracle, instead of a later heap corruption (the mirror of
the native-effect declaration oracle, [effects.md](effects.md)).
Release builds and the JIT/WASM tiers treat it as a no-op (the GPU tiers exclude
any function carrying it via the `is_gpu_instruction` whitelist); their coalesced
sites are covered instead by the runner's cross-tier divergence detection and the
escape golden. The instruction renders into no `[region_instrs]` golden line — it
is scaffolding, not part of the semantic RC stream.

