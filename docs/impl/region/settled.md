# Settled invariants

<!-- audited: 2026-09-21 -->

The invariants the region system upholds, one line each, with the spec that
owns the argument.

Each line states an invariant. The linked spec carries the argument and names
the pinning test — the reference for every correctness claim; do not
re-litigate one from this summary alone. The mission and the leak classes
these invariants close are [the model](../memory.md).

## Reclamation

- Subtree drop frees an Owned region's whole subtree — interior cycles
  included — at the root's single demise, walking the recorded edge table,
  never heap contents ([ownership.md](ownership.md)).
- The mutable-store funnel holds by construction: raw `RefCell` accessors are
  private to `value/`, so an uncounted container store is a compile error
  ([ownership.md](ownership.md)).
- A fiber's region is never a member of a region-rooted cut: a fiber acquires
  aliases merely by running, so no structural obligation can bound its borrows
  ([adopt.md](adopt.md)).
- Every alias of an adopted member is bounded by the root's drop; reachability
  and boundedness are separate properties ([adopt.md](adopt.md)).
- A deferred tail-call release has the activation node's life: the two are one
  per-activation record, moved whole at every park, restore, discard, and
  teardown ([owner.md](owner.md)).
- Merge, owner-node adoption, branch compensation, the type-dispatch prune,
  the native fresh-result rule, funnel-recovered containment, and the
  abort-delivery mint close the remaining forest classes
  ([merging.md](merging.md), [owner.md](owner.md), [effects.md](effects.md)).

## Where a release lands

- An arm is a conditional position, not a syntactic arm body: a `cond`'s
  clause tests and an `and`/`or` tail are arms too, read off the nested-`If`
  each form is equivalent to ([window.md](window.md)).
- Every binder records its scope, so the loop-containment test that extends a
  read to the loop node rests on a recorded fact, never on absence
  ([anchors.md](anchors.md)).
- A rest pattern's collection is built, not read out: the one pattern name that
  owns its value, parked in a stamped slot of the lowerer's own
  ([anchors.md](anchors.md)).
- A `break` transfers its value to its block; the broken region's release is
  pinned where the block's value is consumed, and a break out of a tail block
  carries the return mint ([anchors.md](anchors.md)).
- A release the break jumps over is re-anchored to the block's consumer; a
  nested loop, a nested lambda, and a frame-replacing exit bound the window
  ([anchors.md](anchors.md)).
- The iteration that breaks releases at the break: a `break` opens a
  relocation point of its own at the end of the block it leaves
  ([replicate.md](replicate.md)).
- A binder's init release lands after the slot store, and a `def` evaluates to
  what it bound, so its floor is the `def` itself ([anchors.md](anchors.md)).
- The branch-arm release window anchors a region's one release at the merge
  every path reaches, admitted only where escape proves the frame holds the
  region alone (`frame_held_regions`); everything else keeps the counted
  compensation routes ([window.md](window.md)).
- A region's release route belongs to one binding — the one whose init
  allocated it. Four sites record a route and no others; a `Loop` parameter
  and a pattern name poison nothing ([window.md](window.md)).
- A returned region owes the relocation point no funding edge: a callee
  reaches a value this frame owns as an operand or through its captured
  environment, and both ends of that enumeration are safe
  ([relocate.md](relocate.md), [window.md](window.md)).
- An env cell's compensating release names the box, which nothing repoints, so
  compensation's per-arm routes carry it where the window cannot
  ([compensate.md](compensate.md)).
- A release past a frame-replacing tail call is not a release: it is carried
  back ahead of the call, under the same frame-held admission, with the call's
  own names exempt as the ownership move ([relocate.md](relocate.md)).
- What the fall-through owes, a signal exit owes too: the borrowed-argument
  retain's one consumer per path is reached by no signal exit, so the exit
  consumes it, nil-stamping the stash ([signalexit.md](signalexit.md)).
- A carrier that comes back with an absorbed result never left the frame, so
  it takes the fall-through like a normal completion
  ([signalexit.md](signalexit.md)).
- An abandoned frame runs the releases it still owes, off the emitter's two
  release tables, each route carrying its own receipt; a frame the restarts
  system can replay is not abandoned ([unwind.md](unwind.md)).
- A squelch boundary abandons frames the same way, so it runs the same walk;
  the fiber's survival never enters the reading ([unwind.md](unwind.md)).
- An arm that leaves through a callee takes a replica, not the anchor; a
  release the relocation replicates takes the value route, whose nil stamp is
  what lets two copies act once ([window.md](window.md),
  [replicate.md](replicate.md)).
- A branch merge inherits the points that covered its entry, not only its
  arms' — the branch functionalization inserts for a reassigned mutable is the
  everyday shape ([replicate.md](replicate.md)).
- What a tail call's operand names is its value, not its syntax: the reading
  descends the value-transparent wrappers and stops where the value is made
  ([relocate.md](relocate.md)).
- A compiled capture cell is frame-held exactly as its binding is: the cell's
  holders are its binding's holders, one indirection out
  ([relocate.md](relocate.md)).
- What the frame-exit exemption keeps, a channel must still run: a letrec
  member the body tail-calls rides the deferral, run once at the callee's
  normal completion ([relocate.md](relocate.md)).
- A spliced call's arguments come out of an array the calling convention owns:
  the call releases it, and a spliced tail call moves nothing
  ([mechanism.md](mechanism.md)).

## Who owns a value at a boundary

- The return mint is emitted exactly once per returned value — by the `Return`
  marker or by the tail fall-through retain, never both
  ([mechanism.md](mechanism.md)).
- The return frontier is per-path: an arm that leaves without the value takes
  the dead-arm compensating release; a used sibling arm needs a retain on the
  release's own node ([compensate.md](compensate.md)).
- A tail call moves one reference per occurrence, not per call
  ([rules.md](rules.md) Rule 5).
- A call's result is named by the call's own region; an inline is a device for
  collecting edges, not a splice ([mechanism.md](mechanism.md)).
- A native's result is one region, members included: a helper building part of
  a result allocates through the call's ctx ([ctx.md](ctx.md)).
- A fiber crossing is a counted holder, so it refuses no placement: the park's
  escape retain going out, the resume value's own mint coming back
  ([window.md](window.md), [park.md](park.md)).
- A fiber body owns one reference of every value it yields; the compiler mints
  the missing one for a borrowed payload ([park.md](park.md)).
- What yields is the emit operation, not the `Emit` node: a dynamic `emit` is
  an ordinary native call the analysis recognizes structurally
  ([park.md](park.md)).
- A boundary ends a park with no reader and no install, so it owes both
  references, told apart by the delivery ledger ([park.md](park.md)).
- A value handed to another fiber is delivered, not stored uncounted:
  `Delivers` answers the argument side with no clique, a fiber-frontier escape
  seed, and an unbounded result; an injected error payload's delivery is
  minted at the injection ([effects.md](effects.md)).
- The may-store clique is over pairs of arguments, never over one argument's
  own regions ([clique.md](clique.md)).
- A native's declaration is a claim about escape, not only about edges:
  `Mixed` seeds every argument on the store facet, so a native that stores
  nothing declares `Opaque` however unbounded its result
  ([effects.md](effects.md)).
- A stranded recursive closure's deferred release asks escape nothing: it runs
  after the callee's return mint, so only the fiber facet could refuse, and
  the crossing funds that itself ([../selfrec.md](../selfrec.md)).
- A returned letrec closure cycle merges when its release runs after the
  return mint; the fiber half of the frontier refuses outright
  ([letrec.md](letrec.md)).
- The arena channel and the callee channel are independent: one tail call can
  carry both, and they never name the same region ([letrec.md](letrec.md)).
- A cycle that hands a member out is released where that member is released,
  which retires the sole-held proxy for exactly those members
  ([letrec.md](letrec.md)).

## Reassigned bindings — the 1-slot container

The model is [bindings.md](bindings.md); its readers are
[reads.md](reads.md).

- A chain of forwarding edges hands one reference along; the fold follows it
  whole, and a forwarding link emits no content drop.
- A stored value's release belongs to the store that took it, pinned at that
  store site — never spread over the cell's other stores.
- What the cell donates it must hold alone; what it counts it need not. An
  alias of the init withdraws the donation, not the model, and an aliased
  stored value takes the counted store.
- The content drop post-dominates every access, hoisted past a loop that
  carries the cell.
- A whole-value read of the container takes a counted reference; a branch is a
  read of whichever arms read, and a version of the container is not an alias
  of it ([reads.md](reads.md)).
- A returned binding's cell counts what it holds, exactly as an unreturned one
  does: the `Return`'s mint claims nothing the cell holds, and a `Return` is a
  reader of the cell's content.
- A value a 1-slot container holds is a runtime fact: a mint past the last
  store reads the region off the value, never the emptied slot.
- Compensation's premises are read per route, never per holder
  ([compensate.md](compensate.md)).
