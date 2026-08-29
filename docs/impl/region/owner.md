# Owner nodes — an activation as a forest root

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
re-delivery. Its production consumers are the capture-back-edge cut and the
transferred-returned-subtree cut (both below).

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
  a funnel-recovered `containment_edges` edge — so the cut admits the funnel store face,
  where the store is an opaque `Funnel` call, exactly as it admits a compile-time store
  edge). A capture-only SCC is the letrec closure web (the merge's instrument, or class 4/6
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
(rooted and bare shapes, funnel-recovered stores),
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
  with `containment_edges`), so a funnel-recovered edge admits identically to a
  compile-time store edge (the value-resolved adopt needs no store opcode).
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
the funnel-recovered face; the refusal family) and at runtime by
`runtime::tests::ownership::region_ownership_reclaims_returned_cycle_across_calls`,
`…_reclaims_fiber_terminal_cycle`, and `…_transfer_adopt_rides_parks_and_fiber_teardown`
(bounded flag-on beside the leaking flag-off counterfactual, under the JIT too).

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

**Park/unpark symmetry — what a park retains, and who releases it.** These rules keep a
parked fiber's accounting symmetric with its unpark:

- **The resume carrier is never retained.** `prim_fiber_resume` (and `fiber/abort` /
  `fiber/propagate`) returns its fiber *argument* as the signal payload — a carrier, not a
  result: the resume handler replaces it with the child's actual outcome before any caller
  release runs, so a dispatch pass-through retain on it would have no consumer.
  `dispatch_native_call` skips the retain for a fiber-carrier signal
  (`SignalAction::Resume`/`Abort`/`Propagate`). A parked fiber's liveness holds are its
  holders' ordinary counted references — a binding's slot, a container store — so a fiber
  whose last reference drops while parked genuinely reaches rc 0 and frees, chain and all
  (the discharge below). Pinned by the `multi-resume`/`yield-discard`/`cancel-discard`
  oracle probes and `runtime::tests::ownership::fiber_parked_then_dropped_reclaims`.
- **A suspending native tail call parks its continuation.** A non-suspending native tail
  call keeps its frame and falls through to the post-`TailCall` block, whose compiler-emitted
  releases consume the tail args' moved/retained references with exact per-arg ownership
  (`tail_call_inner`). A SUSPENDING one (`fiber/resume` in tail position — the SIG_SWITCH
  handoff) must reach that block the same way: the handler leaves `suspended` untouched so
  the standard interrupted-frame parks (`do_fiber_first_resume`, `resume_suspended`'s
  re-suspend, `call_inner`) capture the continuation at the post-`TailCall` ip, and the
  resume replays it — running exactly the releases the fall-through would have. Parking an
  empty chain instead ("the result is the child's result") strands every owned tail arg —
  one region per nested drained fiber (the `fiber-nested` probe;
  `runtime::tests::ownership::fiber_nested_tail_resume_reclaims`).
- **The parked signal's escape retain has a release on every path.** A suspending signal's
  payload is retained once as it escapes into `fiber.signal` (`EmitEscape` for
  `yield`/`emit`, `SuspendEscape` for a yielding io op or a capability denial). The resume
  path consumes it (the resumed body's own pending release, or — where the body has none —
  the displacing install's, below); a fiber that can never run again consumes it at its
  terminal teardown or free-path discharge instead (`release_discarded_signal` via
  `Fiber::take_parked_state`) — the `yield-discard`/`denied-discard` probes pin both faces.
- **A fiber body owns one reference of every value it yields.** Two references answer for a
  parked payload, and they answer to different consumers. The escape retain above is the
  **delivery** reference: the resumer's compiler-emitted release of the resume result consumes
  it, exactly as a completing child's `Return` mint funds the release of a terminal result.
  What the discard discharge stands in for is the *other* one — the body's own, released by
  the continuation past the suspend, which a fiber abandoned while suspended never runs. A
  payload the body allocated carries that reference itself. A payload it merely **borrows** —
  a capture, a parameter, a module-level binding — carries none, so the discharge would
  release the delivery reference the resumer already consumed and free the value under every
  holder that outlives the fiber. Dropping the discharge instead is not the alternative: it is
  the only release a discarded fiber's stranded continuation ever gets, and withdrawing it
  strands the body-allocated payload of every abandoned park (the `yield-discard` /
  `denied-discard` probes measure exactly that). Which case a park is in is compiler
  knowledge, so the compiler supplies the missing reference rather than a flag: the analysis
  names each `Emit` whose payload its own body releases nowhere
  (`RegionInfo::borrowed_emit_payloads` — every region the payload may live in must have its
  `decref_point` inside the emitting lambda), and `lower_emit` mints one there, an
  `IncrefValueRegion` before the suspend and a `DecrefValueRegion` first in the continuation.
  The copy the release loads is parked in a local slot of its own, the operand stack being
  what survives a suspend. Unresolvable counts as borrowed: minting where the body already
  owns a reference strands one per abandoned park, a bounded leak, while missing one frees a
  live value. Pinned by `tests/elle/region-fiber-yield-borrow-uaf.lisp`. A TERMINAL
  `:error` emit needs no compiler mint for the same invariant: its `EmitEscape` retain is
  the delivery reference exactly as above, and the body's own reference — where the raise
  chain holds one — is claimed through the frames' release tables instead of a blanket
  discharge, because the raise records its minted delivery (`Fiber::emit_delivery`) and
  the abandoned-frame walk and the parked frame's discharge stop exempting the payload's
  region where the record matches ([mechanism.md](mechanism.md) § "An abandoned frame runs
  the releases it still owes"). A halt takes neither the retain nor the record — a halted
  fiber is promoted to `:dead` and never resumed, so its delivery has no consumer.
- **A payload the RUNTIME built is released by the install that displaces it.** The second
  of the two references above — the body's own, released by the continuation past the
  suspend — exists only where the BODY allocated the value. Two parks build their payload
  in the runtime instead: a yielding io op, whose `IoRequest` the native returned, and a
  capability denial, whose `{:error :capability-denied …}` struct the denial path builds
  for the parent to mediate. Neither body ever named the value, so no `decref_point` names
  its region and the continuation releases nothing. The reference the allocation left is
  therefore the discharge's on a fiber that never runs again, and the DISPLACING install's
  on one that does: `fiber/resume`'s delivery and `fiber/abort` / `fiber/refuse`'s injected
  error each replace the payload in the slot, and each owes it a release as it does.
  Which parks are that shape is read two ways, because the bits answer for only one of
  them. An io park is its `SIG_IO` bit, and its release is
  `release_parked_signal` — resume-only, because a `Fresh` io op builds its completion
  buffer IN the request's region and hands that back as the resume value, where the
  resumer's release of the resume result is the second consumer and a release here would
  free the buffer under the caller. A denial parks under the WITHHELD capability's bits,
  which say nothing about who built the payload and are indistinguishable from an
  `(emit :fs v)` of a body-allocated value — so the classifier records the payload
  (`Fiber::denial_payload`, an uncounted marker compared bit-wise like
  `Fiber::emit_delivery`) and `release_displaced_denial_payload` releases exactly what the
  record names, on both installs. The injected error takes no skip: it is not a delivery,
  and the abort's own `AbortDelivery` mint funds the consumer it does have.

  **The two readings overlap on one denial, and the record wins it.** `:io` is a
  withheld capability like any other, so a fiber denied `:io` parks under `SIG_IO` —
  the very bit the io arm reads — and there the denial payload and an `IoRequest`
  cannot be told apart by bits at all. One reference is owed, so `fiber/resume` asks
  the record first and skips the io arm when it claims the park; running both frees
  the payload under the mediator that is still reading it. Every other install
  displaces on the record alone, the io arm never having reached them. Gauged by
  `tests/elle/region-denial-park.lisp` per install and by
  `tests/elle/region-capability-denial-resume-leak.lisp` per denial position, and pinned
  guardfree by `tests/elle/region-denial-park-uaf.lisp`, whose `:io` witnesses are the
  collision.
- **What yields is the emit OPERATION, not the `Emit` node.** A first argument the compiler
  cannot read as a keyword set falls through to the `emit` primitive
  ([../../signals/emit.md](../../signals/emit.md) § "Dynamic emit"), which parks the same way
  and owes the same body reference — so the question above is asked of the operation. The
  walk records the payload argument's regions against a call whose callee is the emit
  primitive exactly as its `Emit` arm records them against the node
  (`CallClassification::emit_natives` names that primitive under each of its names), and
  `borrowed_emit_payloads` answers over both.
  What supplies the reference differs by position, because a call already mints one for an
  argument the frame does not own. In **tail** position that borrowed-argument retain IS the
  body reference: it is taken before the `TailCall` and released by the post-`TailCall`
  block, which the resume replays ([mechanism.md](mechanism.md) § "What the fall-through
  owes, a signal exit owes too"). In **non-tail** position no such retain exists, so
  `lower_call` takes one at the payload argument — the same `IncrefValueRegion`, private
  stash slot, and `DecrefValueRegion` shape `lower_emit` uses, the resume landing at the
  instruction after the call.
  The signal is a runtime value here, so the mint cannot be gated on `suspends` the way the
  literal path gates it. It is taken whatever the signal turns out to be, and a TERMINAL one
  reaches the same consumer by another route: the raise leaves through the mask that catches
  it, which delivers the payload as the resumer's result, and the resumer's release of that
  result consumes one reference exactly as it consumes the delivery of a park. What supplies
  that reference in TAIL position is not the borrowed-argument retain, which a restart's
  replay of the post-`TailCall` block still claims: the exit consumes the retain and mints the
  delivery itself, recording it so the frames' owed releases stop standing in for a delivery
  this call now funds ([mechanism.md](mechanism.md) § "What the fall-through owes, a signal
  exit owes too"). Pinned by
  `tests/elle/region-dynamic-emit-borrow-uaf.lisp` and
  `tests/elle/region-dynamic-emit-terminal-uaf.lisp`, and gauged per op by the `emit-dyn-*`
  probes in `tests/elle/oracle.lisp`.
- **A delivery into a replayed frame carries one owning reference.** A parked
  `BytecodeFrame` re-enters at its suspending call's continuation, whose
  compiler-emitted result release consumes one owning reference of the value the
  replay pushes. A normally-completing child funds it: its `Return` runs the
  ReturnValue retain before the result is handed up, and each frame of a replayed
  chain funds the next the same way. An **aborted** child's error exit runs no
  `Return`, and the reference the replay consumes is the one `fiber/abort`'s
  injection minted for the payload — the replayed frame is one of the four
  consumers that single mint answers for
  ([effects.md](effects.md) § `Delivers`). Without a mint anywhere the replay
  consumes a reference the abort's caller still owns, and a fresh heap payload is
  freed under the caller's read (a constant payload has no region, which is what
  kept the theft invisible). Pinned by `region_fiber_abort_io_protect_uaf`
  (`tests/integration/fixtures/region-fiber-abort-io-protect-uaf.lisp`);
  `tests/elle/grpc.lisp`'s `with-server` teardown is the full-scheduler witness.
  A **primitive** that suspends is the other frame with no `Return` to fund it, and
  it is the general case rather than an exit path: the resume value takes the place
  of the primitive's result, and the continuation past the call releases that result
  like any other. Two parks have that shape — a dynamic `emit`, whose non-literal
  first argument falls through to the runtime primitive instead of compiling to the
  `Emit` terminator, and a capability denial, which parks a denied primitive call
  for the parent to mediate — and neither can be told from an `Emit` park at the
  delivery, where the frame is already built and, for a tail suspend, was built by a
  driver that never saw the primitive. So the classifier records the shape on the
  fiber (`Fiber::resume_value_unfunded`) and `do_fiber_resume_single` takes it with
  the parked signal, minting one `ResumeDelivery` retain on every route into the
  fiber. What needs no mint is an `Emit` park, whose resume block mints in bytecode
  (above), and an io completion, which the scheduler allocates fresh and hands over
  carrying its own birth reference. Pinned by
  `tests/elle/region-primitive-resume-uaf.lisp`.
- **A propagated signal is a fresh park, and owes its own delivery reference.**
  `fiber/propagate` installs the child's parked payload as the propagating fiber's own
  `signal`. That fiber's resumer then reads the payload as its resume result and runs the
  compiler-emitted release on it — the same consumer an `Emit` funds with its `EmitEscape`
  mint. Re-parking a value the child already parked mints nothing, so that release consumes
  a reference the propagate never took. One propagate hides the theft: an error unwind runs
  no continuation, so the raising body's own reference is stranded and unclaimed, and the
  release eats that instead. Two propagates do not, nor does a native error, whose payload
  reaches `fiber.signal` with no body reference at all. The count then runs one short of the
  recorded `fiber → payload` edges, and the last fiber's free cascade reclaims the payload
  while the caller still holds it. So the propagate mints one itself
  (`EscapeSite::PropagateEscape`), through the one `take_propagated_signal` helper the
  call-, tail-, and JIT-position handlers share — all three run the same install, so all
  three owe the same reference. Three cases take no mint. A NON-TERMINAL signal, because the
  fiber runs again and the resume path proper governs the payload — step 6a excludes the same
  set from its park retain, and the delivery follows the park. `SIG_HALT`, for the reason
  `handle_emit` skips it: a halted fiber is promoted to `:dead` and `fiber/resume` refuses
  it, so that delivery has no consumer and a retain would strand the payload. And the
  no-signal fallback, whose error `escaping_error` builds in a fresh region already carrying
  the reference the consumer releases — only a BORROWED payload is unfunded. The WASM tier's
  `handle_fiber_propagate` is not this shape: it never installs into `fiber.signal`, and
  returns the child's `(bits, value)` to its caller, whose park runs through `install_signal`
  instead. Pinned by `tests/elle/region-fiber-propagate-uaf.lisp`.
- **A resume value crosses counted, or not at all.** The delivery going *out* of a park
  is counted (above); the value coming *back* in is not, and by the same accounting must
  be. `VM::resume_suspended` pushes the resume value onto the parked frame's stack and
  takes no reference for it, so the body reads the resumer's own — fine while the resume
  call is still running, and a dangling read the moment the body parks again holding the
  value and the resumer moves on. So the `Emit` itself supplies the reference: its result
  is an ordinary call-result region (`walk`'s `Emit` arm), `lower_emit` mints one after
  `LoadResumeValue`, and the node's own `decref_point` releases it. The mint is skipped
  where the frame's **return transfer** already funds a reference for the same region —
  an `Emit` whose value the frame hands back carries the `Return` marker's mint, and a
  second would strand one per resume, so `RegionInfo::unfunded_resume_values` names the
  sites whose result region is off the return frontier. What this buys beyond soundness is
  the frame-held admission: with both directions counted, a fiber crossing is a counted
  second holder rather than an uncounted borrow, so the branch-arm window and the
  frame-exit release stop refusing it ([mechanism.md](mechanism.md) § "A fiber crossing is
  a counted holder too"). Pinned by `tests/elle/region-fiber-frontier-window-uaf.lisp`,
  with the leak face in `tests/elle/region-fiber-frontier-window.lisp`.
- **A child's inherited parameter baseline is a counted holder.** A new fiber snapshots
  its creator's dynamic-parameter bindings into one baseline frame — at creation
  (`prim_fiber_new`), or at the first-resume fallback for a fiber seeded by its resumer
  (`seed_child_inheritance`, `do_fiber_resume_single`) — precisely BECAUSE the creator's
  `parameterize` blocks unwind long before the scheduler resumes the child. So the child
  routinely outlives every structural holder of the bound values: the frame is Rust-side
  state no store funnel records, and with nothing counting the crossing, the spawner's
  completed activation frees the value's region while the child is parked — every later
  read of the parameter in the child is then a use-after-free (the h2 corpus's
  wrong-typed channel messages, and the wedges behind them, on the thread-pool backend
  whose completion timing lets spawners finish first). The seeding therefore counts,
  like every other seam that hands a value to another fiber: each heap entry of the
  installed baseline takes one retain (`EscapeSite::ParamBaseline`) and records a
  `fiber-region → value-region` content edge, released when the fiber's own heap object
  frees (the Fiber content-scan arm visits the seeded baseline, so the free cascade is
  the one symmetric release — the same shape as the terminal-signal park). The fiber's
  own later `parameterize` frames stay uncounted: their values are the parked
  activation's, released by its owed-release table. The generation-stamped borrow check
  ([generations.md](generations.md) § "Uncounted-borrow check") stays as the oracle that
  the count holds. Pinned by `tests/elle/param-fiber-inherit.lisp` and the
  `region_param_fiber_inherit_uaf` integration pin (debug builds panic at the resume
  boundary when the count is missing).
- **A parked TERMINAL result displaced by a resume or abort install is released as it is
  displaced.** A terminal result parked in `fiber.signal` carries the park-retain and a
  recorded `fiber-region → result-region` content edge, both counting on the fiber's
  free-time signal scan — sound only while the signal stays parked to the fiber's demise.
  A restart (`fiber/resume` of an `:error` fiber), a re-resumed drained stream source, and
  `fiber/abort`'s error install all replace the parked pair, so the scan never sees it:
  the installer first releases the retain and un-records the edge
  (`release_displaced_terminal_signal`). Skipping it leaves the recorded table holding a
  dead edge (the free-time equivalence oracle detonates on the drift), and each re-park
  stacks another, so the free cascade over-releases the payload region
  (`tests/elle/async-error-propagation.lisp` § 4 is the pinning corpus shape; the
  `region-fiber-park-symmetry.lisp` restart face churns the mechanism).

A park moves the activation's owner node into the suspended frame: a suspending exit — a
yield, a suspending native, `fiber/resume`'s SIG_SWITCH handoff, a fuel pause, a capability
denial — parks the activation's continuation as a `BytecodeFrame`; the frame **takes** the
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
fiber's owned slots (each still-parked `BytecodeFrame`'s activation owner node, the fiber
node, and — via `Fiber::take_parked_state` — the parked non-terminal signal whose park
escape retain the resume path can no longer consume) under the fiber borrow; the releasing
then runs against the heap with the borrow already dropped, so heap mutation never overlaps
fiber access and a cascade that frees the fiber's own heap value cannot invalidate a live
borrow. A hard kill takes BEFORE installing its terminal error, so the superseded parked
signal's retain is released, not stranded. When a fiber node
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

**The free-path fiber discharge — the dropped-handle case.** A fiber abandoned **outside**
the terminal transitions — a parked fiber whose last reference drops, a resumable `:error`
or capability-denied fiber nobody restarts — reaches no teardown call, so the release runs
where its demise is actually observed: the region free. When a dying region's pages hold a
`Fiber` object, `RegionStore::teardown_set` takes that fiber's parked state (the same
`Fiber::take_parked_state` set the terminal teardown consumes: parked activation owner
nodes, the fiber owner node, the parked non-terminal signal's escape retain, and each
parked frame's own owed releases — read off its two release tables, with the signal
payload's region exempted only where the raise did not mint the delivery itself,
[mechanism.md](mechanism.md) § "An abandoned frame runs the releases it still owes") and feeds
the regions into the free's iterative cascade — after the debug equivalence oracle, since
these are not recorded content edges. The take empties the fiber's slots, so a fiber that
already tore down discharges nothing, and an executing (borrowed) fiber is skipped — its
region cannot be dying while it runs. Pinned by
`runtime::tests::ownership::dropped_parked_fiber_discharges_owned_state` and the
`yield-discard`/`denied-discard`/`abort-discard` oracle probes.

**The bounded residual: a dead continuation's pending value releases.** A discarded fiber's
parked frames still hold values whose releases live only in the continuation that will
never run. Most of them run at the discharge instead, off the compiler's own release
tables — the frame's value-route slots and its slot-route static regions, each carrying a
receipt that says whether the release already ran ([mechanism.md](mechanism.md) § "An
abandoned frame runs the releases it still owes"). That is what a blanket release of the
parked stack or the parked activation map could not be: a mapped slot can be stale where
its value's release was emitted value-based or died past a tail call, so it double-frees.
What is left is what neither table can NAME — a value with no binding of its own, and so
no route and no receipt: a literal materialized straight into a denied call's argument,
the rest list the calling convention built for a variadic callee, and a parameter released
through an env slot, which carries no nil stamp. This class is bounded per discarded
fiber, measured by the `denied-discard` oracle rate.

One member of it is closed and is no longer part of the residual: a **borrowed tail
argument's** retain, which the frame mints so a callee has a reference to release. That
retain has one consumer per path, and a native tail call's SIGNAL exit — an error, a
suspend, a fiber carrier, a capability denial — reaches neither of them, so the exit
consumes it itself ([mechanism.md](mechanism.md) § "What the fall-through owes, a signal
exit owes too"). Its first stranded reference was often the fiber value the abort carried,
which pinned the body closure and everything the parked frame held behind it.

