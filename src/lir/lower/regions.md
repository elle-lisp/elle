# The region instructions

<!-- audited: 2026-11-21 -->

The retains and releases the lowerer emits where the region solver put them, and how it saves a release a tail call or a `break` strands.

The solver ([infer.rs](../../hir/region/infer.rs)) assigns each allocation its
region, merges regions, and places each release. The lowerer decides none of
it. [The region model](../../../docs/regions.md) is the memory model, and
[the lowerer](overview.md) owns everything else the lowerer does.

## What the lowerer reads

| `RegionInfo` field | What the lowerer emits from it |
|--------------------|--------------------------------|
| `alloc_region[node]` | The region an allocation is born into, through `emit_alloc` |
| `region_data[r].decref_point` | The node after which `r` is released |
| `cross_region_refs` | An `IncrefRegion` at the store that makes the edge (`emit_increfs_for`) |
| `rest_list_layout(lambda)` | How a variadic lambda builds its rest list, written onto its `LirHead` ([restlist.md](../../../docs/impl/region/restlist.md)) |

`with_region_info` ([order.rs](order.rs)) indexes the answer once:
`increfs_by_site` over the edges, `decrefs_by_decref_point` over the regions,
and `cell_drops_by_demise` over the 1-slot containers. Each bucket of releases
is sorted holder before holdee (`order_releases`), so no release reads a page
another release at the same point has freed.

`static_slot` ([emitops.rs](emitops.rs)) maps a solver region to the static
slot its instructions name. A merged region resolves to its root's slot.

## One node's releases

After it lowers a node, `lower_expr` ([expr.rs](expr.rs)) emits the node's
retains, then `emit_decrefs_for` ([regiondecref.rs](regiondecref.rs)), then the
per-arm releases. `emit_decrefs_for` emits one release per region whose
`decref_point` is the node, in the sorted order. A caller that must store the
value first defers the node's releases (`deferred_decref_points`), as each
binder does for its initializer.

`release_route` ([regiondecref/classify.rs](regiondecref/classify.rs)) decides
how a region's release reads its value. Another mechanism may release the
region, no slot may name it, or the slot may be refused. Otherwise the release
frees an environment cell's box, adopts the slot's value, or releases it. Four
sites release a region, and each asks this one function: its
`decref_point`, a branch arm's compensation
([compensate.md](../../../docs/impl/region/compensate.md)), a relocation's
replica, and a call's move. So no site releases a region by a route another
refuses.

| File | What it emits |
|------|---------------|
| [regionemit.rs](regionemit.rs) and [regionemit/](regionemit/) | The retains, the adopts, a captured binding's cell store, and which slot names a region |
| [regiondecref.rs](regiondecref.rs) and [regiondecref/](regiondecref/) | The releases, by route, and a branch arm's compensation |
| [naming.rs](naming.rs) | The static region id, and the address space a value slot belongs to |
| [order.rs](order.rs) | The indexes, and the order of releases at one point |

## A call's move

`call_move_offers` ([control/call/handoff.rs](control/call/handoff.rs)) offers
a non-tail call's argument to its callee where the call node releases the
argument's slot, by a plain value release and nothing else. Its position and
slot ride on `Call` and `SuspendingCall` as `moves`. The borrowed arguments of
a tail call that the runtime mints at the `TailCall` are decided in the same
file ([handoff.md](../../../docs/impl/region/handoff.md)).

## A tail call

A frame-replacing `TailCall` leaves every instruction emitted after it in its
block to the native fall-through alone. Five mechanisms decide who owns a
reference across it, and each function's doc comment holds its rule:

| Function | File | What it decides | Argument |
|----------|------|-----------------|----------|
| `tail_arg_is_borrowed` | [control.rs](control.rs) | Whether an argument is a reference the frame does not own, so the call mints the callee one | [rules.md](../../../docs/impl/region/rules.md) |
| `tail_callee_defers_release` | [control/call/defer.rs](control/call/defer.rs) | Whether the new activation takes over the release of a callee closure the call strands | [selfrec.md](../../../docs/impl/selfrec.md), [cycletail.md](../../../docs/impl/region/cycletail.md) |
| `open_tail_exit_hoist` and `with_tail_exit_hoist` | [relocate.rs](relocate.rs), [splice.rs](splice.rs) | Which release past the call moves ahead of it | [relocate.md](../../../docs/impl/region/relocate.md) |
| `begin_branch_arms`, `seal_arm_hoists`, `open_branch_merge` | [relocate.rs](relocate.rs) | Which release past a branch merge is copied ahead of each arm's tail call | [replicate.md](../../../docs/impl/region/replicate.md) |
| `open_operand_arm_points` | [relocate/armpoint.rs](relocate/armpoint.rs) | Which release an arm of a branch operand owes for a region a sibling arm hands the call | [operand.md](../../../docs/impl/region/operand.md) |

`tail_callee_defers_release` asks three binding-keyed channels before the
general reading. A stranded self-recursive callee defers and asks escape
nothing. A closure-cycle member and a letrec member the body tail-calls each
defer unless the binding escapes its fiber. Every other callee defers where its
region dies at the call and does not escape.

A release moves or takes a copy only where the frame holds its region alone
(`RegionInfo::frame_held_regions`). A copy has to count once per path, so only
a run that releases a slot's value and stamps the slot `nil`
(`self_cancelling_run`, [splice/run.rs](splice/run.rs)) is copied. A release
that a copy will take reads the value route of its binder's slot
(`replicating_release`).

## An emit

`lower_emit` ([control.rs](control.rs)) ends the block with `Terminator::Emit`
and opens the resume block with `LoadResumeValue`. A suspending emit whose
payload the body releases nowhere (`borrowed_emit_payloads`) retains the payload
before the terminator. The resume block releases it first, through a slot of its
own that the frame's release table names. The resume value arrives uncounted, so
the lowerer mints the body's reference with `IncrefValueRegion` where
`unfunded_resume_values` names the node. A dynamic `emit` is an ordinary call
to the primitive. In non-tail position `lower_call` takes the same retain and
release around it, and in tail position the borrowed argument's retain is that
reference.
[park.md](../../../docs/impl/region/park.md) holds the argument, and
[unwind.md](../../../docs/impl/region/unwind.md) the release table.

## A break

The solver anchors every region a `break` jumps over at the `Block` node, or at
the node that consumes the block's value
([anchors.md](../../../docs/impl/region/anchors.md)). The lowerer emits those
releases after the exit label, so they run on the break path and the
fall-through alike.

A region the loop body allocates is minted per iteration, so the anchor
releases only the value the slot held last. `lower_break`
([expr/loops.rs](expr/loops.rs)) therefore opens a relocation point at the end
of the block it leaves (`open_break_exit_hoist`), and a release emitted while
that block is open takes a copy there. The value the break carries is exempt,
because the block hands it to its consumer.

Do not write a release at a `break` site. The anchor and the break's point
already cover every path, and a third release frees a region twice.
