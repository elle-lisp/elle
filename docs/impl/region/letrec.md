# The letrec closure-cycle merge

The builder-idiom seed merges one tight `child → parent` store edge. The same
collapse-to-one-region mechanism reclaims a shape per-region RC cannot: the
**immutable reference cycle mutual recursion forms** (`ping`/`pong`). Each member is a
capture-cell↔closure structure: the forward-reference **cell** holds the closure
(`StoreCaptureCell`) and a *sibling* closure **captures** the cell, both at the `letrec`,
never mutated. The cells and closures reference each other around the SCC, so per-region
RC never reaches zero (rules.md Rule 8) — the cycle leaks. Unlike a *mutable*
`@array` cycle (the deliberate boundary, adopt.md § "Why this is hybrid"), an immutable one
is reclaimable, and a fiber that builds one per loop iteration would otherwise leak
unboundedly.

**Self-recursion is not this shape.** A purely self-recursive local fn (`loop` references
only itself) is **cell-free**: its self-edge does not mark it captured
(`hir/analyze/scopes.rs`), so it has no forward cell and no cell↔closure cycle — its
self-reference resolves to the currently-executing closure ([selfrec.md](../selfrec.md)),
reclaimed by ordinary RC / the tail-call deferred release, RC-identical to a top-level recursive
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

A cycle is mergeable only when **every member is sole-held**, **every closure has a
static-slot cell**, and every closure clears the **frontier gate** below. The frontier
question is `compute_shared_seeds`' (return / emit / send crossings), *not*
`EscapeInfo::lambda_escapes_definition`: that method additionally folds in the capture
facet (a value captured by an escaping closure), a containment relation — and an SCC's
closures capture each other, so one member crossing a frontier would propagate
"escaping" around the whole cycle and falsely refuse a mergeable one.

### The frontier gate — the fiber half refuses, the return half is return-funded

The two halves of the frontier read differently, because the merge's release is a
**decref, not a free**, and what matters is only whether it can reach zero while
something else still reads the arena.

- **The fiber half always refuses.** An emitted or sent member crosses to a resumer or
  receiver whose hold the compiler did not place, and a parked frame may borrow it
  uncounted. The cycle stays Shared — the always-legal baseline.

- **The return half is admitted where the arena's release runs after the return
  mint.** A returned member does not outlive the arena's *count*: because the merge
  collapses that member's region onto the arena, the value handed out lives **in** the
  arena, so the mint that funds the caller raises the arena's own count. Three
  references are in play over one call — the frame's own (taken at the letrec setup),
  the caller's (that mint), and any container's (taken by the store funnel) — and the
  release the merge owns is the frame's. So the admission is entirely a question of
  **order**: a release that runs after the mint drops the frame's reference with the
  caller's already standing, and on a path that returns something else nobody else
  holds the arena and the frame's is correctly the last.

  The order is settled by one structural question: **does the letrec body hand the
  value over itself?** Every tail exit of the body must be a node that leaves the
  frame — a `Return`, or a tail `Call`. Each of the three ways such an exit mints puts
  the mint ahead of the merge's release:

  - a **`Return`** mints where it stands (`lower_return`'s `IncrefValueRegion`), inside
    the body, while the binding-scope `DecrefRegion` the merge owns is emitted at the
    `Letrec` node — after the body's whole lowering — so the release follows it;
  - a tail call to a **closure** replaces the frame, so that `DecrefRegion` is dead code
    and the release rides a deferral instead — the member channel's
    (`stranded_cycle_bindings`) or the non-member channel's (`deferred_release_slot`),
    whichever applies — and `trampoline_loop` runs either at the recursion's **normal
    completion**, after the callee's `Return` mint;
  - a tail call to a **native** keeps the frame and falls through to that same
    binding-scope `DecrefRegion`, but the call's own mint — the post-`TailCall`
    fall-through retain, or `lower_return`'s retain where ANF named the result — is
    emitted at the call site, inside the body, so it precedes the release too.

  So *which* channel carries the release does not enter the ordering argument, and
  neither does whether the compiler can classify the callee: the merge wires both
  channels precisely because it cannot, and both are late enough. This is the same
  ordering argument as the cell-free self-recursive deferral's return admission
  ([selfrec.md](../selfrec.md) § "The deferral's escape gate is the fiber frontier
  alone") — the release runs *after* the mint, so unlike the frame-exit relocation
  there is nothing to bridge.

  What the body must **not** do is fall out to a bare value, because then the letrec's
  value is handed to an *enclosing* consumer: the mint that funds the caller, if there
  is one at all, sits outside the letrec node while the merge pins the release at the
  binding scope. `(let [c (letrec [ev … od …] ev)] … c)` is that residual — `c` holds
  the member past a `DecrefRegion` that already took the arena to zero. An exit the
  reading does not recognise is refused for the same reason: the reading exists to prove
  the mint is inside the body, so anything it cannot read must read as outside. A loop
  and a short-circuit `And`/`Or` fall out to a value that way; a `Cond` with no else arm
  and a `Match` fall out on the path where no arm is taken — which is why this reading
  needs the arms EXHAUSTIVE where branch compensation, asking a different question,
  admits a `Match` arm exactly as an `If` arm.

The gate is asked per SCC, and the return admission is asked only when some member
actually carries the return facet — a non-escaping cycle never consults the tail
shape, so the ordinary in-lambda and top-level cycles admit exactly as before.

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
numeric `compute_order` compare (adopt.md § "The lifetime obligation the root carries"). The
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

**A tail-call letrec body hands the drop to a tail-call deferred release — for a member *or* a
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
  descending into nested lambdas), `tail_callee_defers_release` returns true for such a callee
  (read through a **non-upvalue** reference only, so a nested closure in the body can
  never free the arena out from under a later use), and the `TailCall` carries
  `deferred_release_region = region_of(callee)` — the merged arena, because a member lives in it.
  That consumer refuses a callee crossing the **fiber** frontier, and admits the return
  facet — the same reading, for the same reason, as the merge's own frontier gate above
  and as the cell-free self-recursive deferral ([selfrec.md](../selfrec.md) § "The
  deferral's escape gate is the fiber frontier alone"): this deferral runs at the
  recursion's normal completion, after the `Return` mint that funds the caller's
  reference. The marking is keyed on `closure_cycle_members`, so only a member of an
  **admitted** merge reaches it and the gate never has to re-argue admission; it is kept
  whole so the two ends of the channel state the same premise. The non-member channel
  below states it too, and is admitted for the return facet on the same terms: both
  deferrals run at the recursion's completion, and the fall-through the non-member
  channel additionally has is the binding-scope drop, which the frontier gate already
  proved runs after the body's own mint.

- **A tail call to a NON-member** (a native `%add`, a redefined operator `+`, a
  foreign closure `g`) rides an explicit slot instead. The arena is therefore
  exempt from the frame-exit hoist (mechanism.md § "A release past a
  frame-replacing tail call is not a release"): its binding-scope `DecrefRegion`
  is dead past the frame replacement *by design*, and hoisting it ahead of the
  `TailCall` would make both channels fire. The analysis records the tail
  site in `RegionInfo::cycle_tail_release` (site HirId → the merged root region), the
  lowerer sets the `TailCall`'s `deferred_release_slot` to the root's static slot
  (`compute_closure_cycle_merges` → `ClosureCycleMerge::tail_release_sites`), and the
  runtime resolves that slot through the executing activation's region map — the arena
  was minted during the letrec setup and its scope-exit drop is dead. If the callee
  turns out a **closure**, the frame is replaced and `trampoline_loop`'s
  `deferred_releases` frees the resolved arena once (deduped) at the recursion's
  completion; if it turns out a **native**, the frame is not replaced, the slot is
  never consumed, and the live scope-exit `DecrefRegion` frees the arena — mutually
  exclusive, exactly one release, the compiler having classified nothing.

Both member and non-member releases run at the recursion's completion / the
scope-exit, so the same channel the cell-free self-recursive deferred release rides
([selfrec.md](../selfrec.md)). Interior sibling calls (`ev` tail-calling `od` inside the
SCC bodies) never defer: `tail_callee_defers_release` refuses any callee whose region is a
closure-cycle merge member (`RegionInfo::closure_cycle_members` — the merge owns the
release), and only the letrec-body marking overrides that refusal. On a body with
mixed tail exits (`(if c (ev k) (%add (ev k) 0))`) exactly one release fires per path:
the member arm defers via `region_of` (its binding-scope drop dead there), the
non-member arm via `deferred_release_slot` or the live scope-exit drop.

**What the non-member tail still refuses — the by-move boundary.** A cycle member
passed **by-move as a tail argument** (`(g od)` — `od` itself, not `(ev k)`'s result)
refuses the whole cycle to Shared. The member's own move/return machinery decrefs the
merged arena a second time, colliding with the deferred release (a double-free); the escape gate
does not catch it (an opaque callee's argument is not a return/fiber Shared-seed). So
the tail gate reads each argument's region-transparent flow bindings (mirroring
escape's `tail_sources`: through control/select/deref, stopping at a `Call`/
`Intrinsic`/`Lambda`) and refuses when one is an SCC member. A member stored into a
fresh aggregate then passed (`(g (%pair od 1))`) is RC-counted, and a member *called*
in an argument (`(g (ev k))`) contributes its result, not itself — both admitted. An
unresolvable non-member callee (no site to key the deferred release at) likewise refuses.

**All-tier, unconditional.** The merge extends the same `merged_parent` forest the
builder seed populates and rides the same `merged_root` canonicalization and
`merged_slots` mint-or-reuse every tier already resolves — so it adds no opcode and no
JIT helper: it lands on the `compute_merges` path every compile runs. Pinned by
`regions::tests::merge`
(`merge_collapses_mutual_recursion_letrec_closure_cycle` — the mutual SCC + cells collapse
onto one `merged_root`; `merge_collapses_in_lambda_mutual_recursion_letrec_closure_cycle`
— the same collapse and binding-scope drop for a letrec that is a lambda body;
`merge_admits_in_lambda_cycle_with_foreign_tail_callee` and
`merge_admits_native_tail` — a non-member (foreign closure / native) body
tail now MERGES and records `cycle_tail_release`;
`merge_refuses_member_passed_by_move_to_foreign_tail` — the by-move boundary (`(g od)`
double-free) still refuses;
`merge_mutual_recursion_cycle_drops_at_binding_scope_not_enclosing`;
`self_recursive_letrec_is_cell_free_not_merged` — a pure self-recursive letrec has no cell
and is never a member; `merge_collapses_self_and_sibling_captured_member_cell` — the mixed
self+sibling-captured member's retained cell still merges).

The **frontier gate**'s faces are pinned as one family, so the two halves cannot
drift into each other. The return half's admission is pinned across all three ways a
body leaves the frame — `merge_admits_returned_member_cycle_on_member_tail` (a member
tail call), `merge_admits_returned_cycle_on_non_member_tail` (a non-member one, whose
two callee kinds take the two late channels), and `merge_admits_returned_cycle_on_value_tail`
(a bare member value, whose `Return` mints inside the body) — against
`merge_refuses_returned_cycle_bound_out_of_tail_position` (the residual: the letrec's
value is handed to an enclosing consumer, so no mint stands before the binding-scope
drop). `merge_refuses_fiber_crossing_letrec_cycle` holds the fiber half, which no mint
can fund; its letrec body's tail *is* a member call, so the fiber facet is the only
gate left to bite.
`merge_refuses_returned_cell_free_self_recursive_closure` guards the reading itself: a
returned *self*-recursive closure is refused by CELL-FREEDOM, not by the frontier, so
that refusal must not be read as "returned ⇒ refused".

At runtime the merge is pinned by the guardfree fixtures
`region_native_tail_mutual_cycle_uaf` (every non-member tail kind, mixed, and
per-loop-iteration reclamation) and `region_letrec_return_cycle_uaf` (a returned member
re-entered after the deferral, across churn that recycles a freed page, and handles held
live across later mint/free cycles), and by
`runtime::tests::ownership::region_ownership_reclaims_mutual_recursion_closure_cycle`
(bounded per-run region growth beside a leaking discriminator), with
`region_ownership_reclaims_returned_mutual_cycle_per_call` doing the same for the
returned cycle, `region_ownership_reclaims_nested_mutual_recursion_per_call` driving the
in-lambda cycle per call (bounded beside the live-chain discriminator, base case
included) and `closure_cycle_discarded_release_is_prompt` pinning the binding-scope
drop's promptness (a discarded top-level cycle freed at its letrec, not held to
teardown). The oracle reads `recur-local-mutual-ret`, `recur-local-mutual-ret-foreign`
and `recur-local-mutual-ret-value` (the three admitted body shapes, all closed at 0)
beside `recur-local-mutual-ret-bound` (the refused residual).
`region_ownership_reclaims_self_recursion_closure_cycle` pins the same bounded growth for a
pure self-recursive closure, which is reclaimed cell-free (ordinary RC / the tail-call
deferred release — [selfrec.md](../selfrec.md)), not by this merge.

