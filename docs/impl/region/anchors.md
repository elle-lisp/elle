# Where a release is anchored

<!-- audited: 2026-09-05 -->

Where the solver anchors a release: what each binding form pins, and what a
`break` does to the releases its jump passes over.

## A binder's init release lands after the slot store

A binding's initializer is an ordinary expression, so `lower_expr` emits its
releases where it emits every node's: immediately after the node. That position
is **before** the binder has stored the value into its slot, and a release
landing there does the wrong thing on either route ([two
resolutions](mechanism.md)): a **value-routed** one reloads the holder slot,
reads the `nil` the binder pre-stamped and releases nothing at all, while a
**slot-resolved** one names the region directly and frees a value the binder is
about to store — leaving the slot pointing into freed pages.

A release lands there only when the initializer is *itself* the region's
`decref_point`, which is the unused-binding narrowing: nothing reads the bound name,
so the value's last use is pulled back to where the value was made. `Let` and
`Letrec` therefore route the init node through `deferred_decref_points` and emit its
releases themselves, after the store (`tests/elle/region-unused-let-binding.lisp` is
the pin).

`Define` is the binder that must not be narrowed there in the first place, because
**a `def` evaluates to what it bound**. Every other binding form's value is its
*body*, so an init no name reads really is dead at the init; a `def`'s value IS the
init, so it is live wherever the `def` is — handed to a callee, returned, bound to a
second name, propagated out of a `begin` or a branch arm. The narrowing's floor for a
`Define` is therefore the point the walk gave the `def` itself
(`propagated_inits`, `hir/liveness/lastuse`): the enclosing consumer when there is
one, and the `Define` node when the `def`'s value is discarded — whose releases
`lower_expr` emits after `lower_define` has stored. Narrowing below that frees the
value under the expression it was handed to
(`tests/elle/region-define-init-release-uaf.lisp`); leaving it at the init frees
nothing (`tests/elle/region-define-init-release.lisp`).

So a `def`'s initializer region is released by the ordinary last-use mechanism,
whatever it holds. This is what a cell-free self-recursive `def` rides — its closure
region needs no suppression, because the binding's last use as a **callee** resolves
to the node that consumes it and the release lands where the recursion has already
completed ([selfrec.md](../selfrec.md)).

## Every binder records its scope

A `Var` read inside a `While`/`Loop` is extended to the loop node when the binding
it names is bound **outside** that loop: the body re-reads it on every iteration,
so its region has to outlive the loop (`hir/liveness/lastuse`). The premise is a
containment test — is the binding's **scope node** a descendant of the loop? — and
it is only as good as the scope map is complete. A binder the walk does not record
has no scope node at all, and an absent scope is read as *bound outside*.

Both answers to that question are consequential, in opposite directions. Read as
bound **inside** when it is not, the release fires per iteration and the next
iteration reads a freed region — a use-after-free. Read as bound **outside** when
it is not, the release is hoisted past a loop whose body re-allocates the value
every iteration, so one release covers N allocations and N−1 regions are held to
fiber teardown — an unbounded leak. Neither direction is a safe default, which is
why the answer must come from a recorded fact rather than from absence.

So every binding form records its scope: `Define`, `Let`, `Letrec`, `Loop`,
`Destructure`, and a `Match` arm's **pattern**. The pattern is the one whose names
carry a region they did not allocate: a projection out of the scrutinee is an
uncounted read ([rules.md](rules.md) Rule 4's borrowing node), so it resolves to
the *scrutinee's* region, and the binding-chain extension carries the scrutinee's
release out to wherever the projection is last used. Unrecorded, an arm that reads
a name its pattern bound hoists the whole scrutinee's release past the enclosing
loop — every object the scrutinee holds, stranded once per iteration, on the arm
that runs and equally on one that never does (the extension is structural, so a
read in an arm no execution takes strands the scrutinee just the same).

A `Match` pattern records only its scope, not the init registration `Destructure`
also makes, and the difference is where the bound names are readable. A
`Destructure`'s names are read by *later siblings*, so the destructured value's own
last use must be extended to cover them. A `Match` arm's names are readable
strictly inside the `Match` node's subtree, and the scrutinee's last use is the
`Match` node itself — the branch consumes it — which already post-dates every read
of a projection in any arm. Registering an init would also expose the scrutinee to
the unused-binding narrowing (`compute_last_use`'s first phase pulls an init's last
use back to the init itself when no bound name is read), shrinking a lifetime the
`Match` node already states correctly.

Pinned by `tests/elle/region-match-bind-loop.lisp` (the reclamation, with the
arm-taken, arm-not-taken, nested-loop and guard faces driven as rows) and the
`struct-match` probe in `tests/elle/oracle.lisp` (the per-op rate), with
`tests/elle/region-match-bind-loop-uaf.lisp` as the soundness complement — a
pattern-bound projection stored, returned, broken out of the loop, captured, or
carried across a yield must survive the per-iteration release.

## `break` transfers its value; it does not consume it

A `Return` hands a value across a *function* frontier. A `break` is the
intra-function dual: it hands a value to the enclosing `block`, whose value is
its fall-through value **or** the value of any `break` targeting it. While the
block is *interior* to the function no reference changes hands — the value stays
in the same activation — so there is no mint; when the block is the function's
**tail** the break's value is also the function's result and takes the ordinary
return mint (below). What a `break` does change is *where the value dies*, and by
two compounding facts, neither of which the ordinary consuming-node treatment
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

- **Region flow** (`hir/region/infer/walk`, the `Block`/`Break` arms): a `Block`'s
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

Pinned here: `tests/elle/region-break-transfer.lisp` (the reclamation), the
`break-value*` probes in `tests/elle/oracle.lisp` (the rates),
`regions::tests::blocks` (the placement, structurally), and
`region-break-transfer-uaf.lisp` (the soundness complement — a value broken out
and read afterwards, stored, or returned must survive).

### A break out of a TAIL block carries the return mint

The pin above places the release at the block's exit label. When the block is the
function's **tail**, that exit label is the last thing before the frame is handed
back, so the value the break carried is the *returned* value and it must leave
with one owning reference: the release at the exit consumes the callee's, and the
caller's own `DecrefValueRegion` consumes another. Only the return mint balances
that — the same mint any other returned value gets.

Both passes that decide "tail position" must therefore agree that a `break`
targeting a tail block is in it. `mark_tail_calls` and `wrap_tail_returns`
(`hir/return_incref.rs`) each thread a `tail_blocks` set: a `Block` in tail
position adds its own id, and a `Break` whose target is in that set walks its
value as a tail value — marking a call there `is_tail` (whose callee-side retain
propagates) or, for anything else, wrapping it in `Return` (which mints).

The two flags answer different questions, and the invariant is that only a
**function boundary** invalidates the second: `in_tail` is severed by any node
whose child is not its result, but `tail_blocks` survives every node except a
`Lambda`, because a `break` reaches its target's exit label by a *jump* and no
enclosing construct can intercept it. The shape that turns on the difference is
the pervasive `(fn … (forever … (break v)))`: the loop between the tail block and
the break is not itself a tail position — the loop's fall-through value is the
loop's, not the function's — yet `v` is the function's result. Sever the set
there and `v` is returned with no mint while the exit-label release still fires:
the caller reads a freed value. Pinned structurally (`return_incref::tests` — the
mint count per break, with the interior-block control) and behaviourally
(`region-break-transfer-uaf.lisp`'s tail-loop witnesses, whose faulting shape is
`lib/tls.lisp`'s `tls/read`).

### A release the break jumps over is not a release

The transfer covers the value the break *carries*. Every **other** region whose
release sits in the same window — inside the block's body, at or after the break
site, before the exit label — is jumped over by the identical edge, and for a
region the break does not carry there is no consumer to hand it to: the release
is emitted into unreachable code and the region is held to fiber teardown.
`(block (let [x (mk)] (when c (break 1)) (use x)))` strands `x` on every
execution that breaks.

The close is the same pin, not a release at the break site. A per-path release
at the break would need a site-list of what to free there *and* a count argument
for each entry; the placement argument alone suffices, because a release moved
**later** can only over-keep. So a region whose `decref_point` falls in the
skipped window is re-anchored to `last_use[block]` — the first point both the
break path and the fall-through path reach, and the same anchor the broken value
takes. Carried and skipped regions then leave the block through one release
each, and the lowerer still needs no new instruction and no new site-list.

"Skipped" is read off the structural order (`compute_order`, the same index
every `decref_point` comparison uses): a node's releases are passed over by a
break exactly when its post-order index is **at or above** the break's — which
covers the break node itself (its own decrefs land in the dead block after the
jump) and every enclosing `let`/`begin` whose releases the lowerer emits after
the body.

Three boundaries bound the window. Two are about *how many times* a release runs
rather than where:

- **An iterative scope nested in the block** (`While`/`Loop`), for a region the
  loop body ALLOCATES. Such a value is re-allocated per iteration, so its release
  must stay per-iteration: hoisting it to the block's exit would leave one
  release for N allocations — a worse leak than the one being closed, and the
  same re-allocation argument the `capture_loop_ext` "bound outside" guard makes.
  A break out of a loop therefore still strands the *breaking iteration's* own
  regions, an over-keep bounded by one iteration.

  A region the loop merely READS is a different case, and the barrier is read off
  the allocation site rather than the release's position for exactly that reason.
  A parameter, or anything bound before the loop, is allocated once per
  activation; its release only *sits* in the loop because that is where its last
  use is. One release at the anchor covers it exactly once, which is the count
  the re-allocation argument asks for. Refusing there strands a parameter on
  every call that breaks — `dt-lookup`'s `name` and `value`, compared against
  each dynamic-table entry in a `while` and then broken past.

  A region with no allocation site in the unit at all is a parameter by
  construction: the caller allocated it, so no loop here re-allocates it.
- **A `Lambda` nested in the block.** Its body's releases run in a different
  activation, against a different frame's slots; the enclosing block's exit label
  is not a point that activation ever reaches.

The third guards the anchor itself — the hoist's premise is that the exit label
is a point every path **reaches**:

- **A frame-replacing exit on the block's fall-through** — a `Call` in tail
  position, lowered as `TailCall`. That path leaves through the callee instead of
  arriving at the exit label, so a release moved to the anchor would be dead on
  exactly the path that used to run it — one leak traded for another. Such a
  block declines the window whole.

  An exit inside a targeting break's own **value** is not that case, and reading
  it as one costs a region per call on any block with more than one break. On the
  break path the release is already jumped over, so there is nothing left for the
  exit to strand and the trade is one-sided: every other path gains its release
  and that path stays exactly as it was. The shape is ordinary because a break's
  value is walked as a tail value in a tail block, so a `{…}` or `[…]` literal
  carried by any break past the first is a tail-marked `Call` sitting in the
  window. `dt-lookup` in `lib/http2/hpack.lisp` is four of them.

  A `Return` node is **not** one of these, and reading it as one costs a region
  per call on the most ordinary shape there is. `Return` is what
  `wrap_tail_returns` puts around a *tail value*, and it is its only producer;
  `lower_return` emits the return **mint** and nothing else, so control falls
  through it. Both places one can appear inside a block's window — the block's
  own tail value, and the value of a `break` targeting a tail block — therefore
  reach the exit label like any other path. Declining there refuses the window of
  every function whose body is a `block` with a `break` in it, which is how a
  lookup helper is written: `(defn f [k] (block (let [x (mk k)] (when (hit x)
  (break …)) …)))` strands `x` on every call that breaks.

  The tail-block idiom `(fn … (forever … (break v)))` is unaffected either way:
  the broken value's own pin applies (it is the *returned* value, and its release
  is the one the return mint funds), and the `forever` is an iterative-scope
  barrier that keeps the window off the loop body regardless.

All three leave the conservative baseline (the release stays where it is,
skipped on the break path), never a mis-free.

Pinned by `tests/elle/region-break-skip.lisp` (the reclamation, with all three
boundaries driven as rows that must stay bounded on their own releases),
`regions::tests::blocks` (the placement and the boundaries, structurally), and
`tests/elle/region-break-skip-uaf.lisp` (the soundness complement — a value in
the window that is read, stored, or returned after the block must survive the
moved release).

