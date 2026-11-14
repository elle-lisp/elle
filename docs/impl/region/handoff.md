# What a call hands its callee

<!-- audited: 2026-11-14 -->

The reference a call hands a closure callee for each argument, and how the frame learns what the call settled.

## The owned parameter

A closure callee owns each non-captured fixed parameter. The solver gives the
parameter a placeholder region, and the callee releases the argument by value
at the parameter's last use ([rules.md](rules.md) Rule 2). So every entry into
the callee hands it one reference per such parameter:

| Entry | The reference the callee receives |
|---|---|
| A call | a fresh one the runtime mints (`CallArgument`), or the frame's own where the call moves it |
| A tail call | the frame's own, moved ([relocate.md](relocate.md)), or a fresh one for a borrowed argument |
| A spliced call, in either position | a fresh one ([mechanism.md](mechanism.md)) |
| An FFI callback | the converted argument's only one |

A captured parameter is held by its cell, and a rest argument by the collected
list or struct, so no entry hands either a reference of this kind.

## A call that is an argument's last use moves it

In call position the frame keeps its own reference and releases it at the
argument's last use. Where that last use is the call, the runtime would raise a
count for the callee only for the frame to drop its own as the callee returns.
So the call offers the frame's reference to the callee instead, and the two
counts are never taken.

The lowerer offers argument *i* when all three hold:

1. The argument is a read of a binding in a stack slot, and the call node is
   where the binding's region is released, by the value route that loads that
   slot ([mechanism.md](mechanism.md)). Nothing after the call reads the value,
   so the release may run inside the callee instead. ANF may name a call written
   straight into an argument inside that position, `(q (let [t (f)] t))`, and
   the read of `t` is such a read.
2. The release is a plain `DecrefValueRegion`. It is not a cell's release, a
   transfer adopt, or the release of a co-owned group member, a merge root or
   an owner whose subtree holds members.
3. No other argument and not the callee name the region, by a source or by a
   borrow ([borrow.md](borrow.md)), and no other release the call node emits
   is tied to it by an ordering edge ([rules.md](rules.md) Rule 4). Where one
   is, the callee could release the moved reference while it still reads the
   other: its own closure, or a value that lives inside the moved one.

A moved release runs earlier than its place among the call node's releases.
Condition 3 leaves it no edge to any of the others, so the holder-before-holdee
order they keep among themselves is unchanged.

The `Call` carries each offer as the argument's position and the slot the
release loads. The runtime takes an offer only where the callee is a closure
and the argument fills an owned parameter. It skips the mint and stamps the
slot `nil`, so the frame's release loads an immediate and does nothing. Every
other callee declines: a native, a parameter, a collection, a captured
parameter, a rest position, and a call that raises before the callee's
environment exists. A declined offer leaves the call as it was, so a tier that
reads no offers stays correct. The lowerer offers nothing where the callee is a
constant, which is a native, or where the callee is neither a binding nor a
binding's cell.

The stamp is also the receipt. A frame that parks after its callee took a move
holds `nil` in the slot, so a resume, a restart and the abandoned-frame walk
each find nothing to release there ([unwind.md](unwind.md)). The callee's
release of the parameter answers for the reference on every path.

A predicate called through a parameter, `(p? (gs i))`, is the everyday shape.
The read hands the frame one reference, and the call hands that same reference
on. So each item costs one count and one release: the read's count and the
callee's release.

## A tail call mints a borrowed upvalue only for a closure

A tail call moves the frame's reference to each owned argument. A borrowed
argument has no reference of the frame's to move, so the call hands the callee
a fresh one ([rules.md](rules.md) Rule 5). The mint is wasted when the callee is
a native: the frame stays, and its fall-through block releases the mint at once.

So where the argument reads a captured upvalue of an immutable binding, or a
heap constant, the runtime makes the mint at the `TailCall`, once it knows the
callee. The closure environment or the seeding environment holds such a value,
and nothing the frame runs before the call can release it. Every other borrowed
argument keeps its mint ahead of the call. A pattern element's scrutinee may be
released before the `TailCall`, and a cell another argument's call reassigns
may drop the value it held.

The lowerer still gives the argument a stash slot and releases the slot in the
fall-through block, but it stores `nil` there, never the value. The frame can
park before its `TailCall` starts: another argument raises or yields, or the
call's fuel charge pauses it. The frame's release table names the stash, so
the walk over a park nobody resumes releases what the stash holds. An unminted
value there would be released once more than it was counted. The `TailCall`
carries each late mint
as the argument's position and its stash slot:

| Outcome | The stash |
|---|---|
| A closure callee replaces the frame | gone with the frame. The callee's owned-parameter release consumes the mint |
| A native suspends | the minted value, which the runtime writes there. The replayed fall-through releases it, or the walk does for a park nobody resumes, as for a retain taken before the call ([signalexit.md](signalexit.md)) |
| Any other outcome, or a park before the call | `nil` |

The other outcomes are a native that completes, raises, carries a fiber or is
denied, a parameter or collection callee, and a call that raises before the
frame is replaced. On each the frame owes no reference for the value, and a
replayed fall-through finds nothing to release.

## The discharge, on each tier

A call reports the offers it settled: each move the callee took, and each late
mint it made for a native suspend. Before anything else reads the frame, the
frame stamps each taken move's slot `nil` and writes each such late mint's
argument into its stash. The interpreter writes the slot on the operand stack.
Compiled code hands `elle_jit_call` and `elle_jit_tail_call` one word holding
the offered positions, and the helper writes the settled positions back into
it. The frame then selects `nil` or the argument into each settled slot ahead
of its signal check, so a park spills the selected value. A self tail call
becomes a loop that replaces no frame, so the loop makes each late mint itself
before it jumps.

An offer names an argument position below 64. The lowerer makes no offer past
it, and a borrowed argument there keeps its mint ahead of the call. The WASM
tier counts no references, so it reads neither list.

## Pins

| Test | What it pins |
|---|---|
| [region-call-traffic.lisp](../../../tests/impl/region-call-traffic.lisp) | The counts and releases a call costs, on each tier ([traffic.md](traffic.md)) |
| [region-call-move-uaf.lisp](../../../tests/impl/region-call-move-uaf.lisp) | The move, under guardfree |
| [region-tail-late-mint-uaf.lisp](../../../tests/impl/region-tail-late-mint-uaf.lisp) | The late mint, under guardfree |
| [handoff.rs](../../../src/lir/lower/tests/release/handoff.rs) | Which arguments the lowerer offers, and which it refuses |
