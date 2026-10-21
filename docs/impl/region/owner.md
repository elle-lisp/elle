# Owner nodes — an activation as a forest root

<!-- audited: 2026-10-23 -->

The pages-less region that realizes an activation or a fiber as a forest root, the two cuts that fill it, and what frees it.

What a park does to the node and the deferred set it carries is
[what a park retains](park.md).

An owned subtree's root need not be a pages-owning region. The forest's owner lattice is
{region, activation, fiber}, and the runtime realizes an **activation owner** as an **owner
node**: a pages-less region used purely as a forest root. `new_runtime_region()` mints the
node id, so it can never alias a live region, and no allocation ever targets it. Its
`RegionEntry` exists only to carry `owned_children`.

A member joins by the ordinary `adopt_region(node, member)`: `Counted → Owned`, count
consumed. Every typestate guarantee of an owned subtree therefore holds unchanged. A
member's stray decref is a structural no-op, and a second adoption is a debug-asserted bug.
The node's demise is one `free_region_set` over node and transitive children. Interior
cycles reclaim with the set, and the Shared frontier, read from the recorded `outgoing`
tables, cascades once. **No new reclamation mode exists.** The node rides the same subtree
drop a region root does, and tearing down its own entry returns zero pages.

That entry is also the node's only trace on the gauges. The node holds no object and claims
no page, so `arena/count`, `arena/bytes`, and `arena/page-claims` read flat across its whole
life. `arena/region-count` alone can see it, because `active_region_count` counts entries
rather than contents ([diagnostics.md](diagnostics.md)). That is what makes the region
gauge the dual reading of the object gauge rather than a coarser copy of it.

Pinned by `regionstore::tests::forest::pages_less_owner_node_subtree_drops_members`,
`…::pages_less_owner_node_counts_in_active_region_count`, and
`…::interior_cycle_in_owner_node_reclaims`.

**The channel is `AdoptIntoActivation { child }`.** It is value-resolved like
`AdoptRegion`: the handler resolves the child's runtime region through `result_region_of`,
unwrapping a capture cell. It carries **no parent operand and no static slot**. The parent
is the *current activation's* node, minted lazily at the first adopt. An activation that
adopts nothing pays nothing, and an immediate child has no region, so it adopts nothing and
mints no node.

The channel is **idempotent on an already-Owned child**: the handler adopts nothing when the
child's region is already a forest member. A program can hand one region to the channel
twice: a masked-`:error` fiber restarted after delivering the same payload, or a value
handed back twice. The region then stays owned by its **first** adopter instead of tripping
the one-owner adopt assert. That is sound because the first adopter's release post-dominates
the later hand-off's use, every consumer being gated to discard. The compiler-paired
`AdoptRegion` sites keep the strict assert, since their inference claims each member exactly
once. Only this consumer-facing channel absorbs re-delivery. Its production consumers are
the capture-back-edge cut and the transferred-returned-subtree cut, both below.

**The capture-back-edge SCC — owner = activation.** One containment-graph shape neither
region-rooted mode can own is the **capture-back-edge cycle**: a container captured by a
closure it holds. Here `m ⊇ c` by store and `c ⊇ m` by capture, the m↔c SCC. A region root
cannot own it. `m` is captured, so its `decref_point` is over-extended one structural step
past the closure. `m` is also store-adopted, so its own `DecrefValueRegion` stays live. The
owner-aware lifetime obligation therefore refuses the subtree, and
`adopt_edges_refuses_captured_store_member_on_lifetime` pins the refusal. The co-owned group
free cannot own it either: `c` is a closure region, whose cell⊇closure containment the
external-uniqueness scan cannot see.

The activation owns it instead
(`region::infer::ownership::compute_activation_adopts` → `RegionInfo::activation_adopt_sites`).
The SCC's members are adopted into the executing activation's owner node and freed by its
completion release. That release post-dominates every in-activation use by construction.
Each admission gate refuses to Shared, the always-legal baseline:

- **the signature** — a genuine mutual-reach SCC (≥ 2 members) whose interior edges include
  at least one *capture* AND at least one *store*. A store is a non-hard `cross_region_refs`
  edge or a funnel-recovered `containment_edges` edge. The cut therefore admits the funnel
  store face, where the store is an opaque `Funnel` call, exactly as it admits a
  compile-time store edge. A capture-only SCC is the letrec closure web, which the merge or
  the letrec admission owns ([letrec.md](letrec.md)). A store-only SCC is the co-owned
  group's;
- **member gates** — every member ownable (no frontier crossing, no dynamic-lifetime
  class) and sole-held, with pairwise-distinct holder bindings. Each member must have its
  own slot for the value-resolved adopt to load;
- **disjointness** — no member is claimed by another mechanism. A merge participant
  (builder-idiom or closure-cycle), a co-owned group member, or a store/capture-adopt
  subtree region is never also node-adopted. That is the one-owner invariant at the emit
  level;
- **the hull** — every region referencing INTO the SCC, transitively over all edge kinds
  (hard may-stores included), must itself be ownable. The members free at the activation's
  completion, so every holder must provably die within the activation. A holder that
  returns or crosses a fiber frontier refuses the SCC. The hull members keep their own
  baseline releases, and their cascades onto the Owned members are structural no-ops;
- **one activation, no loop seam** — the members' allocation sites share an innermost
  enclosing structural scope, the adopt site, so a cross-lambda SCC refuses. No
  `While`/`Loop` may enclose a member's allocation without also enclosing the adopt site.
  Adopt-per-iteration is sound, with fresh regions each round. Alloc-inside/adopt-outside is
  not, because the static suppression would outlive the slot's last iteration value;
- **the park split** — no park may run between a member's allocation and its adopt. In
  that interval the members are `Counted`, and their own releases are already suppressed
  under the suppress ⊆ adopt contract. A route that abandons such a park therefore
  strands the whole SCC: no release table names the members, and no node adopted them.
  The adopt scope can park when `Signal::may_park` holds: any inferred bit outside
  `:error`/`:halt`/`:ffi`, or a polymorphic propagation. The walk then sites a second,
  earlier adopt ahead of every reachable park (the park split, below). A shape the
  split cannot order refuses to Shared. A park ordered before every member allocation
  needs no gate: no member exists when it parks, and the early adopt still precedes
  every later one.

The lowerer emits one value-resolved `AdoptIntoActivation` per member at the adopt site
(`emit_adopt_into_activation`). `emit_decrefs_for` drives it exactly as it drives the
co-owned group's free. `analyze_regions_with` suppresses **both** members' own compiler
decrefs through `suppressed_decref_regions`. That is the same suppress ⊆ adopt contract the
capture adopt carries. Every decref-emit site (`emit_decrefs_for`, `emit_arm_decrefs`,
`emit_branch_compensation`) re-checks the same set defensively, so no release path can
double a node member's demise.

The members stay `Counted` between construction and the adopt. Normal RC absorbs the
interval: an outside holder's earlier cascade decref just lowers the count the adopt then
consumes. From the adopt to the activation's completion they are `Owned`, and the node's
release frees the cycle wholesale, interior m↔c references reclaiming with the set.

**The park split — the adopt runs before any park.** `emit_decrefs_for` fires for every
node right after its own lowering. For a binding init it fires right after the binding's
store (`lower_let`/`lower_letrec`'s deferral). An adopt keyed at ANY node therefore runs
at that node's completion. When the adopt scope can park, the walk uses this to close the
`Counted` interval ahead of every park (`compute_activation_adopts`' park split).

The walk descends the scope's sequential spine: a `Let`/`Letrec`'s binding inits then body,
or a `Begin`/`Block`'s statements. Each constituent completes before the next begins. The
walk stops at the last constituent that allocates a member, recursing while that
constituent both allocates and parks. It keys a second adopt there, so
`activation_adopt_sites` carries two entries for one SCC. Every member's binding store
completes with that constituent, so the early adopt loads populated slots. Every park in a
later constituent finds the members already `Owned`, and the parked frame carries them in
its owner node ([what a park retains](park.md)).

The scope-exit adopt stays beside the early one. A path that leaves the scope before its
end, such as a `break` past the early key, still adopts there. The channel's idempotence
makes a second adopt of an `Owned` member a no-op. The early adopt may precede the SCC's own
interior stores. That is sound: `incref`/`decref` are inert on an `Owned` region, and the
store still records its edge, which the node's set-drop walks like any other. Some shapes
the spine cannot order: a park between two members' allocations, or a park and an
allocation inside one non-sequential constituent (a branch, a call). Each refuses at
admission, through the park-split gate above.

Pinned by `region::infer::tests::adopt::cuts::activation_adopts_capture_back_edge_scc`
(rooted and bare shapes, funnel-recovered stores) and
`…::activation_adopt_sites_ahead_of_park`. The second pins the park split: two sites, the
early key ordered after every member allocation and before the park.
`…::activation_adopt_refuses_park_between_allocations` pins the refusal, and
`…::activation_adopt_excludes_other_mechanisms` the merge/group disjointness. At runtime,
`runtime::tests::ownership::region_ownership_capture_back_edge_cycle_reclaims` pins it,
bounded beside a leaking discriminator, panic-clean, on the interpreter and under the JIT.
End to end, the `adopt-park-*` probe family in
[the concurrent probes](../../../tests/impl/probe/concurrent.lisp) pins it. A park inside
the adopt scope reclaims on the handle-drop, abort, and cancel routes alike, and the `ap-*`
controls attribute each ingredient.

**The transferred returned subtree — owner = the consuming activation.** The second
containment shape no region root can own is the **returned cycle**. A callee builds an
externally-unique subtree containing a reference cycle and hands its root back across the
return (or fiber) frontier. Inside the producer every member crosses no frontier but the
root does. The region-rooted cuts therefore refuse, since a Shared seed poisons the subtree
walk and the group walk alike. In the consumer the root is an opaque call-result whose
`DecrefValueRegion` releases one reference. A cycle's interior back-edge holds another, so
the cycle survives every release and leaks per call.

The owner that reclaims it is the **consuming activation**. Its owner node's release
post-dominates every use of the result, on either side of the frontier. Every
producer-side use precedes the return, and every consumer-side use precedes the
completion. The cut is `region::infer::ownership::compute_transfer_adopts`. It yields
`RegionInfo::transfer_adopt_regions` and merges the interior edges into the adopt maps. The
cut has a producer half and a consumer half, admitted only together. The interior adopts
freeze member counts, so an unadopted consumer would hold uncounted borrows. One
inadmissible consumer site therefore refuses the whole callee:

- **the producer summary** — a lambda reachable only through an immutable, single-init,
  never-mutated binding (or as a bare `fiber/new` body). Its body tail resolves through
  the structural wrappers to a single binding with exactly **one** source region: the
  **root**. The root must be allocated in the lambda. It may cross the **return** frontier,
  which is the shape, but not the **fiber** frontier, since an emitted or sent root has an
  unbounded second consumer. It must not be any dynamic-lifetime class.

  Every other member of `reach(root)` is born AND last-used inside the lambda. A captured
  outer value refuses, and so does a member a later sibling still reads: freeing at the
  consumer's completion must not free anything with a life of its own. Each such member
  crosses no frontier, is sole-held, and is claimed by no other mechanism. The subtree must
  be externally unique, with no edge from inside to outside; the return itself records
  none. It must contain an **interior cycle**. An acyclic returned subtree reclaims
  promptly by the RC cascade, and adopting it would only trade promptness away.

  Each non-root member gets its single owner exactly as the store/capture adopt assigns
  one. Uniquely to this cut, a **funnel-recovered** owner edge is emittable too. The adopt
  is keyed at the funnel *call site* (`funnel_store_sites` joined with
  `containment_edges`), and the value-resolved adopt needs no store opcode. A
  funnel-recovered edge therefore admits identically to a compile-time store edge.
- **the consumer gate, at every call site of the summarized callee** — the call's result
  region must cross no frontier, and appear in **no** edge of any kind, hard may-stores
  included. It must belong to no dynamic class. It must also be **discard-shaped**: no user
  binding holds it, or its sole holder's every read is an argument of an
  `Immediate`-effect native. A consumer that stores, captures, returns, or extracts from
  the result refuses the callee. Extraction through a pass-through native (`get`/`first`)
  records no edge. The discard-shape gate is what keeps an uncounted member borrow from
  escaping the node's reclamation horizon.
- **the fiber face** — the same summary applied to a `fiber/new` body whose inferred signal
  can deliver no non-terminal value: no yield, io, debug or wait bits, and not polymorphic.
  A completing `fiber/resume` then hands back the body's **terminal** value, the returned
  subtree, crossing the fiber frontier. Every other resume outcome is a fresh error struct
  or an immediate, each safely adoptable.

  The fiber binding must be single-init, never mutated, **uncaptured**, and bound in the
  same function body as its every use. Each activation of the consumer then drives its own
  private fiber, so no delivery can outlive the adopting activation. The
  restarted-`:error` re-delivery lands in the same activation, where the channel's
  idempotence absorbs it. Each use must be arg0 of a `fiber/resume` (a gated consumer site)
  or an argument of an `Immediate`-effect native (`fiber/status`). `fiber/value` is
  pass-through, a second route to the terminal subtree, and the use gate refuses it.
- **the inline face** — the same summary applied to a binding's initializer that returns
  the root itself, with no call between. That is the shape a grafted producer leaves in its
  caller ([grafting a call](../graft.md)): `(def x (mk))` becomes a binding of `x` to
  `mk`'s `let`, which builds the cycle and ends in the root. The initializer is the
  producer, and every binding that holds the root and is bound outside the initializer is
  a consumer. The consumer gate is the call face's, read at those holders. The root must
  cross no frontier at all, and it must have such a holder: a root only its producer holds
  is sole-held, and the region-rooted cuts own it. The consumer release that is replaced
  is the root's own.

Emission is two-sided. The producer's interior owner edges ride the ordinary adopt maps:
`owned_adopt_edges` at store/funnel sites, and `capture_adopt_edges` at the closure, with
capture members suppressed under the same suppress ⊆ adopt contract. They build the runtime
ownership tree under the root, while the root itself stays `Counted` through the hand-off.
The root's count at the consumer's release is ≥ 1 by construction, because the release *is*
the adopt.

At each consumer site, `AdoptIntoActivation` **replaces** the root's release, the
slot-loaded or discarded-result `DecrefValueRegion`. The adopt consumes the whole count,
the interior back-edge's stuck reference included. The node's completion release then
set-drops root and owned members in one collection, interior cycle edges dropping in-set.
Promptness is the designed activation bound. The subtree frees at the consuming
activation's completion rather than at the result's last use; for a top-level consumer
that is the root activation's exit. Only a discarded returned *cycle* pays that, and the
baseline never frees one at all.

The **fiber tier** of the owner lattice is reached structurally, not by a distinct opcode.
A consumer that parks moves its node into the suspended frame like any activation state.
The terminal-fiber teardown gathers parked nodes under the fiber node for one set-drop, as
the fiber teardown below describes.

Pinned by `region::infer::tests::adopt::transfer::transfer_adopts_*`: admission on each
face, the funnel-recovered edges, and the refusal family. At runtime,
`runtime::tests::ownership::region_ownership_reclaims_returned_cycle_across_calls`,
`…_reclaims_a_grafted_returned_cycle`, `…_reclaims_fiber_terminal_cycle`, and
`…_transfer_adopt_rides_parks_and_fiber_teardown` pin it, bounded beside a leaking
discriminator, under the JIT too.

**Lifecycle.** The node slot is per-activation state carried beside the region-remap frame:
`Fiber::activation_dues` parallels `activation_region_maps`, pushed empty on every
fresh activation entry (the interpreter's `saving_stack` push, the JIT prologue's
`push_region_map`) and popped with it. The node is freed **implicitly at the activation's
normal completion** — the interpreter trampoline's clean
break and the compiled function's `Return` path
(`elle_jit_release_activation_dues`) each take the slot and run one
`decref_region_if_present(node)`: rc 1→0, subtree drop — never by an emitted drop
instruction (no single static site covers return + tail + yield + error + squelch).
The kill exit of a compiled charge site runs the same release, because it ends the
activation with no frame to park ([jit-fuel](../jit-fuel.md)). This is
the same clean-break discipline as the trampoline's tail-call-adopted closure release, and a
frame-replacing tail call likewise keeps the activation — and its node — alive to the
recursion's completion.

**A deferred tail-call release has the node's life, so it rides the node's slot.** A
frame-replacing tail call strands the releases the lowerer emitted past the `TailCall`: the
callee closure's own per-call region, and the merged closure-cycle arena a letrec body
tail-calls out of ([letrec.md](letrec.md)). The new activation takes each over and owes
it one decref. That obligation answers to the same three moments the node's does. It must
reach the activation's normal completion across however many parks the activation takes
on the way, and it must be discharged where the activation is abandoned instead.

The two are therefore **one** per-activation record (`ActivationDues`: the node plus the
deferred set), and one record means one move discipline. The slot is pushed and popped with
the region-remap frame and MOVED into the parked `BytecodeFrame` at a suspend.
`restore_activation_region_map` restores it, and the discard chokepoint and the terminal
teardown release it as a unit. Split into two carriers, a park could move one and drop the
other. That is a leak on one side and a strand on the other, and nothing would fail on it.

Held in the trampoline loop's own Rust local instead, the set would be lost at every exit
that is not the clean break, and the ordinary one is the **park**. The loop returns, the
local dies, and the resumed body re-enters through a *fresh* loop with an empty set. A fiber
that yields once inside a tail-called closure would then strand the callee's region even
when it was resumed to completion.

The record on the activation slot also carries the deferral to a tail call the interpreter
trampoline never sees. The JIT sentinel arms and the top-level driver each consume the
pending tail call themselves. The deferral is therefore recorded where the tail call is
BUILT (`tail_call_inner`) rather than where it happens to be consumed.

**What an abandoned frame owes, it owes the deferred set too.** An error parks the set
with the frame, as a suspend does, so a restart's completion or a discard runs it
([unwind.md](unwind.md)). The one exit that runs the set itself is a call **boundary**
that refuses a tail call: a squelch, or the proof of a silent function. The refusal
ends the activation the call replaced, so no frame of it is left to park
(`end_refused_activation`). That exit stands exactly where the clean break stands:
nothing replays the frame, so the decref the activation took over is at its last chance
to run.

The signal's payload needs no exemption here, unlike the release table's own routes. A
deferred region is a per-call closure region or a merged arena. A raise's payload is one of
two things. It may be a value the native built into its own fresh call region, nobody's
argument, so it is in neither. Or it is one of the call's arguments, whose delivery the
raise mints (`mint_raised_argument_delivery`); that mint is what lets the frame's own
references go.

The shape that looks like a counterexample is a payload the raiser reaches through the
tail-called closure's **captured environment**. There the env's counted edge is the value's
last holder, once the frame-exit relocation has run the binding's own release ahead of the
`TailCall`. Releasing the closure region cascades that edge away. The value survives on the
raise's mint, which is a reference no frame holds.

**A discard runs what the abandoned frames owed (a boundary, a refusing host).**
Abandoning suspended work while the fiber runs on flows through one chokepoint,
`VM::discard_suspended_frames`. The routes are a call boundary's refusal (a squelch/attune
signal-violation, a broken proof), a host's refused park, and a host's error. A boundary
reaches the chokepoint from `VM::boundary_violation` on every tier: the interpreter
trampoline, `compile/run-on`, and the JIT call paths. A host reaches it from
`VM::refuse_hosted_park`, and, for the chain an error parked, from
`VM::discard_hosted_error`.

The discarded frames' continuations will never run, so every release that lived in one is
at its last chance here. The chokepoint runs the whole set a frame chain can owe. That is
the same set `Fiber::take_parked_state` hands the terminal teardown and the free-path
discharge, read the same way (`ParkedDues::of`). Three readings:

- each frame's parked **owner node**, one tolerant decref — rc 1→0, subtree drop over
  node + adopted members, the Shared frontier cascading once from the recorded `outgoing`
  tables;
- each release the frame's activation took over from its own **frame-replacing tail
  calls**, the plain decref its emitting instruction never ran;
- the releases the frame still owed off its **two release tables**, read against its
  saved locals and its saved activation map ([unwind.md](unwind.md)).

A fourth reading answers for the **park itself** rather than for the frames, and it is
the ledger's rather than any frame's. A boundary ends a park with no reader and no install,
so what the crossing and the install would each have consumed is owed here
([what a park retains](park.md)). It runs after the three above, whose entries the delivery
retain keeps standing until they have all run.

Each reading names regions the discard is entitled to release, and none of them names a
region a live frame still counts on. A node's members are exactly the regions the
inference proved externally unique and moved in through `AdoptIntoActivation`. A deferred
region is one the compiler itself named as this activation's to release. A table entry is
a release the *executing function* emitted for its own slot, with a receipt that says it
did not run: the value route's nil stamp, and the mapping the slot route takes. The frame's
saved stack and saved map were taken and cloned at that frame's own park. Its activation
returned before the boundary was reached, so the receipts are read against state no live
frame shares.

What stays refused is the rest of the parked `activation_region_map`. A blanket per-slot
release of it carries no receipt. The regions it names may be an outer, non-discarded
frame's or the catching activation's, and releasing one double-frees it. A frame dropped
*outside* the chokepoint still abandons its dues. That is a bounded leak, never a
double-free, since the members have no count for any other release route to reach. A frame
that raises is not such a frame: it parks with the rest of its chain
([unwind.md](unwind.md)).

Pinned by `runtime::tests::ownership::discard_frees_parked_activation_owner_node`, over
single-frame and multi-frame chains. The member's generation bumps at the discard, bounded
across repeated park-discard cycles. `…::discard_runs_the_abandoned_frames_release_tables`
pins both routes, against a frame whose slot the emitter never recorded. The leak gauge is
[region-squelch-unwind.lisp](../../../tests/impl/region-squelch-unwind.lisp), with
[region-squelch-unwind-uaf.lisp](../../../tests/impl/region-squelch-unwind-uaf.lisp)
under `--trace=guardfree` as the panic-clean gate.

**Exactly one reclamation path (the double-free invariant, positively).** A node member is
`Owned`: it has no count for any other release route to reach. The inference that emits its
adopt must suppress the member's own compiler decref, under the same suppress ⊆ adopt
contract the store/capture adopts carry. Membership is granted only through
`AdoptIntoActivation`, for a region proven externally unique. The node's completion free is
therefore the member's sole demise.

**All-tier.** The interpreter arm (`handle_adopt_into_activation`,
[src/vm/dispatch/region.rs](../../../src/vm/dispatch/region.rs)) and the JIT helper
(`elle_jit_adopt_into_activation`,
[src/jit/dispatch/region.rs](../../../src/jit/dispatch/region.rs)) share the VM's lazy-mint
and adopt body. The WASM backend handles the op structurally, with a no-op arm, since the
arena boundary reclaims. A function carrying it is GPU-ineligible (`is_gpu_instruction`).
Pinned end-to-end by
`runtime::tests::ownership::activation_owner_node_frees_adopted_member_on_normal_completion`
(interpreter) and `jit::compiler::tests::adopt_into_activation_frees_member_at_compiled_return`
(JIT). Each asserts the member's generation bump at completion and bounded region growth
across repeated activations.

**The fiber owner node.** The owner lattice's fiber tier is a second pages-less node,
`Fiber::fiber_owner_node`. It is the forest root for a region whose owner is the **fiber**
itself: a member that outlives every single activation of that fiber (the cross-call /
cross-fiber transfer class). It is fiber state on the `Fiber` struct, unlike the
per-activation node, which every park must move into a frame. It therefore rides
suspension, resumption, and fiber swaps structurally, with nothing to transfer. It is
minted lazily, and is `None` for a fiber that owns nothing.

No production lowering targets the fiber node *directly*. The transferred-returned-subtree
cut adopts into the consuming **activation's** node, and the fiber tier is reached
structurally. A parked consumer's activation node rides its frame, and the teardown below
gathers every parked node under the fiber node for one set-drop.

**Fiber teardown frees everything the fiber owns.** The members a fiber owns are released
at its **terminal** transitions, through one take-then-release pair
(`take_fiber_owned` / `release_fiber_owned`, [owned.rs](../../../src/vm/fiber/owned.rs)).
The taking empties the fiber's owned slots under the fiber borrow. Those slots are each
still-parked `BytecodeFrame`'s activation owner node and deferred tail-call releases, and
the fiber node. Through `Fiber::take_parked_state` they also include the parked
non-terminal signal whose park escape retain the resume path can no longer consume.

The releasing then runs against the heap with the borrow already dropped. Heap mutation
therefore never overlaps fiber access, and a cascade that frees the fiber's own heap value
cannot invalidate a live borrow. A hard kill takes BEFORE installing its terminal error, so
the superseded parked signal's retain is released, not stranded. When a fiber node exists,
each parked node's members are first gathered under it (`reparent_owned_children`), and
the emptied node is freed. The teardown is then **one** set-drop over the fiber's whole
owned set: node, members and interior cycles, the Shared frontier cascading once from the
recorded `outgoing` tables. With no fiber node, each parked node subtree-drops directly.

The terminal transitions are normal completion (`with_child_fiber`'s `:dead` arm), a halt,
and the hard kills. A halt is `VM::finalize_dead_fiber`, at every `SIG_HALT → Dead`
promotion. The hard kills are `fiber/cancel` of a new, paused or errored fiber, which
leaves it `:dead`, and `fiber/abort` of a not-yet-started one, which leaves it `:error`.
Both run `kill_fiber`, which takes the status from its caller. A kill also releases the
terminal signal it displaces, where the fiber held one.

An `:error` fiber is otherwise **not** terminal. It is resumable, since the restarts system
replays its re-parked chain, so an error promotion releases nothing: its parked chain and
nodes stay live for the resume. A fiber-node member carries one contract: it must never
hold the fiber's **terminal result**. A result that outlives its fiber is transferred out
(`reparent_owned_children`) before completion, never left to be freed under the consumer's
read.

Pinned by `runtime::tests::ownership::fiber_owner_node_freed_at_fiber_completion` and
`…_survives_parks_and_frees_at_completion`. The second covers a multi-frame chain, where
every parked frame's node and the fiber node reclaim. `fiber_kill_frees_parked_and_fiber_owned`
covers cancel of a parked fiber and abort of a new one.
[region-fiber-cancel.lisp](../../../tests/impl/region-fiber-cancel.lisp) under
`--trace=guardfree` is the frees-nothing-live gate.

**The free-path fiber discharge — the dropped-handle case.** Some fibers are abandoned
**outside** the terminal transitions: a parked fiber whose last reference drops, or a
resumable `:error` or capability-denied fiber nobody restarts. These reach no teardown
call, so the release runs where their demise is actually observed: the region free. When a
dying region's pages hold a `Fiber` object, `RegionStore::teardown_set` takes that fiber's
parked state. That is the same `Fiber::take_parked_state` set the terminal teardown
consumes: parked activation owner nodes and deferred tail-call releases, the fiber owner
node, and the parked non-terminal signal's escape retain. It also holds each parked frame's
own owed releases, read off its two release tables. The signal payload's region is exempted
only where the raise did not mint the delivery itself ([unwind.md](unwind.md)).

The free feeds those regions into its iterative cascade, after the debug equivalence
oracle, since they are not recorded content edges. The take empties the fiber's slots, so
a fiber that already tore down discharges nothing. An executing (borrowed) fiber is
skipped, because its region cannot be dying while it runs. Pinned by
`runtime::tests::ownership::dropped_parked_fiber_discharges_owned_state` and the
`yield-discard`/`denied-discard`/`abort-discard` probes in
[the concurrent probes](../../../tests/impl/probe/concurrent.lisp).

**The bounded residual: a dead continuation's pending value releases.** A discarded fiber's
parked frames still hold values whose releases live only in the continuation that will
never run. Most of them run at the discharge instead, off the compiler's own release
tables. Those are the frame's value-route slots and its slot-route static regions, each
carrying a receipt that says whether the release already ran ([unwind.md](unwind.md)). A
blanket release of the parked stack or the parked activation map could not do that. A
mapped slot can be stale where its value's release was emitted value-based or died past a
tail call, so a blanket release double-frees.

What is left is what neither table can NAME: a value with no binding of its own, and so no
route and no receipt. One case is a literal materialized straight into a denied call's
argument. The other is a parameter released through an env slot, which carries no nil
stamp. This class is bounded per discarded fiber and has no gauge of its own: the
`denied-discard` probe reads closed ([assessment.md](../assessment.md)). A variadic
callee's rest list is not in it. The rest parameter's slot is a value route with a receipt,
so the discharge releases the list
([region-discard-variadic-rest.lisp](../../../tests/impl/region-discard-variadic-rest.lisp)).

A **borrowed tail argument's** retain is not part of the residual. The frame mints it so
a callee has a reference to release. That retain has one consumer per path. A native tail
call's SIGNAL exit (an error, a suspend, a fiber carrier, a capability denial) reaches
neither of them, so the exit consumes it itself ([signalexit.md](signalexit.md)). Left
unconsumed, the stranded reference is often the fiber value the abort carried. That value
pins the body closure and everything the parked frame holds behind it.
