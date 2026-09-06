# What a signal exit owes

<!-- audited: 2026-09-05 -->

A native tail call runs its fall-through block on normal completion alone, so a
signal exit answers for the releases left in it.

## What the fall-through owes, a signal exit owes too

The relocation above decides which releases *leave* the post-`TailCall` block. What it
cannot decide is whether the block ever runs. The block belongs to the native
fall-through, and a native reaches it on exactly one outcome: **normal completion**
(`bits.is_empty()`, the `SignalAction::Ok` classification). Every other outcome — an
error, a suspend, a fiber carrier (`fiber/resume`/`fiber/abort`/`fiber/propagate`), a
capability denial — leaves through the signal machinery, which does not run the block
before returning to the dispatch loop.

One release in that block is the frame's own **extra** reference, and it is the
one the signal exit runs. A tail call hands its callee one fresh owning
reference per **borrowed** argument, because a captured upvalue is owned by the
closure env rather than by this activation: pure-moving it would hand the callee
a reference the caller never had ([the relocation](relocate.md)). That retain
has exactly one consumer per path — a frame-replacing closure callee's
owned-param release, or, the frame not being replaced by a native, the
fall-through block's own `DecrefValueRegion`. A signal exit reaches neither, so
it consumes the retain itself. The count argument is the borrowedness: the value
has a holder that is not this frame, by the definition of the class, so dropping
the extra reference frees nothing.

**Except the retain that names the parked payload.** A SUSPEND does not abandon
the continuation: the driver the tail suspend unwinds to parks it at the
post-`TailCall` ip and the resume replays the block ([owner.md](owner.md)). So
on that exit the fall-through's `DecrefValueRegion` is still a consumer, and the
retain it consumes is exactly what the park owes — a fiber body owns one
reference of every value it yields, and a borrowed payload has no other
([owner.md](owner.md)). The suspending exit therefore leaves standing each
retain whose region is the payload's, and consumes the rest as before: a park
delivers ONE payload and the discharge of an abandoned fiber releases ONE
reference of it, so a retain on any other region has no such stand-in and would
be stranded once per abandoned park. The test is the payload's region against
the stash's, the same reading the abandoned-frame walk makes of its own tables
([the abandoned-frame walk](unwind.md)). A capability denial parks too, but its
payload is a struct the denial built, which names no argument — so its retains
keep the ordinary consume.

That exemption is what carries a **dynamic** `emit` in tail position, whose
non-literal first argument makes it an ordinary native call rather than the
`Emit` terminator: the borrowed-argument retain is the body reference the park
owes, and there is no second mint.

**A terminal `:error` consumes its retains like any other exit.** The exemption
above cannot carry one. An `:error` fiber is resumable, so a restart replays
this block, and the stash release then reaches a retain the **catcher** has
already consumed as the payload's delivery ([the abandoned-frame
walk](unwind.md)). What funds the catcher is a delivery the raise mints for
itself at the signal exit — in either position, under the identity gate and the
ledger record that travel with it (`Fiber::delivery`, `record_mint`;
[owner.md](owner.md)). So this exit consumes its retains as on any other path,
and the nil stamp makes the replay a no-op.

**What the record frees is this block's own releases.** With the delivery
funded, every reference the frame holds answers to the frame's own routes, so
the walk and the discharge stop exempting the payload's region and the releases
in the abandoned block run. Two references are reclaimed that way, both of them
this position's: a payload the body ALLOCATED, whose owned-argument release sits
in the block, and the second name of a payload that reaches the call twice — the
first occurrence moves the frame's reference and the second takes a retain of
its own (rules.md Rule 5), as `(emit s s)` does. A restart runs those same
releases at the replay, so the accounting is the same either way.

**Every OTHER release in that block stays.** They divide in two and neither half
has that argument. An **argument's** own release is the ownership move, and a
signal exit is exactly where the payload may BE that argument — a fiber carrier
returns its fiber argument, a yielding io op hands the scheduler a request
embedding its port — so the signal machinery accounts for it on the path it
takes. Everything else the block still holds is a release the relocation
declined to move, which it declines only when escape refuses
`frame_held_regions` — the same count argument, refused. Running either would be
a new release on a path that owed none, with nothing to fund it.

**A frame that leaves by a signal can still be replayed.** A suspending signal
parks the continuation at the post-`TailCall` ip and the resume re-enters it
([owner.md](owner.md)); an `:error` fiber is resumable too, so a restart replays
the same block. Running the release at the signal exit and again at the replay
would be a double release, so the exit **stamps the stash local `nil`** as it
takes the value: the replayed `DecrefValueRegion` then loads an immediate and
no-ops — the same self-cancelling discipline a replicated release relies on
([the relocation point](replicate.md)). The stash is the block's alone, written
once before the call and read once after it, for this release and nothing else.

Resolving the region and releasing it are **two steps**, with the signal handler
between them: the handler installs the payload (which may be this very value)
and may swap the live fiber out from under the frame whose locals name it, so
the names are taken first and the references dropped after.

What this closes is one member of a **dead continuation's** pending value
releases. A fiber abandoned while parked at such an exit — an aborted fiber the
restarts system keeps resumable, a capability-denied fiber nobody restarts —
stranded the retain per call, and its first stranded reference is often the
fiber value itself, which then pins the body closure, its captures and its
parked payload behind it. What remains stranded is the denied call's own
argument scratch ([owner.md](owner.md)).

The **JIT** tier keeps today's behaviour: `elle_jit_tail_call` carries neither this
channel nor the callee-adoption channels (`defer_callee_release`,
`deferred_release_slot`), so a compiled frame that leaves by a signal strands the
retain as before — a bounded over-keep, never an over-free.

Pinned by `tests/elle/region-tail-signal-exit.lisp` (the reclamation, with the
fiber-carrier exit, a heap payload beside it, and a restarted `:error` fiber
driven as rows), the `abort-discard` probe in `tests/elle/oracle.lisp` (the
per-op rate),
`lir::lower::tests::release::frameexit::{a_borrowed_tail_argument_is_named_on_the_call,
an_owned_tail_argument_is_not_named_on_the_call}` (the naming pins, both faces),
and `tests/elle/region-tail-signal-exit-uaf.lisp` (the soundness complement — a
value the signal payload carries, a restarted `:error` fiber that replays the
block, a suspending handoff, and a caught error whose handler reads the released
value's holder must all survive the exit's release). The suspend half of the
payload exemption is pinned by `tests/elle/region-dynamic-emit-borrow-uaf.lisp`
(a tail dynamic `emit` of a borrowed value, driven past an abandoned park) and
gauged by the `emit-dyn-tail` probe in `tests/elle/oracle.lisp`; the terminal
half by `tests/elle/region-dynamic-emit-terminal-uaf.lisp` (a tail dynamic
`(emit sig v)` raise of a borrowed value, read back through every holder that
outlives the fiber) and gauged by the `emit-dyn-*-error*` probes there, whose
`emit-dyn-error-fresh` and `emit-dyn-error-repeat` faces are the ones that read
the RECORD — a payload the body allocated, and one region named through both
arguments, each holding a frame reference the walk must stop exempting once the
mint funds the delivery.

## A carrier that comes back with a result never left the frame

The section above divides a native tail call's outcomes into normal completion, which
runs the post-`TailCall` block, and a signal exit, which abandons it. A **fiber
carrier** — `fiber/resume`, `fiber/abort`, `fiber/propagate`, `fiber/refuse` —
belongs to neither
half until the VM has driven the child. It leaves the primitive as a signal because it
is a *request*: the VM is asked to run another fiber and report what happened. Where
this fiber's own mask **absorbs** the child's outcome, the request is answered here,
the value is the call's result, and nothing has left. So the carrier takes the
fall-through — push the result, continue into the post-`TailCall` block — exactly as a
native that returned `SIG_OK` does.

Reading the absorbed outcome as an exit instead is what strands the block. The block
holds every release the frame still owes for this call: one `DecrefValueRegion` per
**owned argument** (the ownership move a native callee never runs in the caller's
place), the return mint, and the result's own release. Handing the value out through
`fiber.signal` and returning `SIG_OK` reaches none of them, and no other path runs them
either — an absorbed outcome is not an error, so the abandoned-frame walk does not
fire, and it is not a suspend, so no replay arrives.

The count argument the exemptions in the section above make is *why the fall-through
is the answer rather than a walk*. That section keeps an argument's release in the
dead block because a signal exit is where the payload may BE that argument, and keeps
the result's release because it names a local the fall-through would have stored.
Absorption removes both premises at once: the frame runs on, so the block stores its
own result before releasing anything, and it runs the compiler's exact per-argument
ownership rather than a runtime guess about which argument the signal took.

The **other two positions already read it this way**, so this is one rule stated in
three places rather than a new one. In Call position `handle_fiber_abort_signal` pushes
the absorbed result and returns `None`, and the compiler's post-call code runs. On the
JIT tier `handle_fiber_abort_signal_jit` hands it back in the return register and the
compiled caller's post-`TailCall` block runs. The interpreter's tail position was the
one that treated the answer as an exit.

What funds the result's release is unchanged by the position. `dispatch_native_call`
withholds the pass-through retain from a value a native returns as a signal payload
([effects.md](effects.md)), and for an absorbed carrier the seam that
produced the value has already counted one: the injection's `AbortDelivery` where an
abort's mask catches ([effects.md](effects.md)), and for a resume the reference the
crossing itself counted — the park's `EmitEscape` retain for a yielded value, the
child's `Return` mint for a terminal one. That is the same reference the Call position
consumes, so the two positions cannot drift apart.

Where the payload is *also* an owned argument — a literal materialized straight into
`(fiber/abort f "boom")` whose caller reads the result back — the frame owes **two**
releases on one region, and holds two references to fund them: the one its allocation
minted and the one the delivery did. Running one is what a skipped block looks like
from the outside: a rate of one region per abort, flat in the payload's size.

Pinned by the `abort-tail-result`, `abort-mask-caught-literal` and
`refuse-tail-result` probes in `tests/elle/oracle.lisp` (the per-op rates, each beside
the control that removes the tail position), and by
`tests/elle/region-fiber-abort-delivery-uaf.lisp` (the soundness complement — the block
frees the fiber and the payload at the call the carrier returned through, so every
reader that outlives it must still find them).

