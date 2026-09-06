# A release past a frame-replacing tail call

<!-- audited: 2026-09-05 -->

Every release the lowerer emits after a `TailCall` is dead on the closure path,
and what it costs to move one ahead of that call.

## A release past a frame-replacing tail call is not a release

A tail call whose callee turns out to be a *closure* replaces the frame. Every
instruction the lowerer emits after the `TailCall` therefore belongs to the
**native fall-through**: a native pushes no bytecode frame, so on normal
completion the dispatch loop continues into that block (`tail_call_inner`,
src/vm/call/inner/tail.rs) and runs it. A closure callee never arrives there.

For a region the call's own **arguments** name, that is precisely the intent, and
it is the ownership transfer the calling convention rests on (rules.md Rule 5,
move-on-tail-call): the caller does not incref a moved argument, and the release
it never runs *is* the reference the callee's owned-param release consumes. The
callee's own region has the same story through a different channel — the new
activation takes over its release (`defer_callee_release`, `deferred_release_slot`),
which holds only where that channel reaches the release (§ "What the exemption
keeps, a channel must still run").

Every **other** release in that block has no such story. A parameter whose only
use is inside a closure the body builds, a parameter used nowhere at all, a scope
region the body allocated — each has its release emitted where control provably
never arrives, and the frame's own reference is stranded. The cost is one region
per call plus everything its free cascade would have reclaimed, so the dominant
witness is the stdlib helper whose body ends in a call to a local walker:
`(fn [dst src] (let [n (length src)] (letrec [go (fn [i] …)] (go 0))))` strands
both `dst` and `src` on every call, once per heap parameter the walker captures.

The close is the one case where a release moves *earlier*: the same single release
the solver placed is emitted immediately **before** the `TailCall` instead of
after it. The scope-region half of this is already unconditional — `lower_call`
emits the pending `DecrefRegion`s before every `TailCall` for the same reason.

**Relocating an instruction is not by itself free of obligation.** It is tempting
to argue that nothing is added and nothing duplicated, so no count argument is
owed. That is false, and it is the same category error the per-arm route makes
([the branch-arm window](window.md)): on the closure path the release did not run
before and now does, so at runtime this *is* a new release, and it owes exactly
what any new release owes — a reason to believe the frame holds the region's one
reference.

Two readings are therefore both required.

**What the call can reach** — the exemption, read off the call itself: every
region the callee, an operand's own value, the call's own result, or its deferred
channels (`deferred_release_slot`) name keeps its place in the dead fall-through,
where the ownership move and the deferred callee release own it. Read over
`alloc_region` and `binding_source_regions`, and again over the emitted
instructions, because ANF is free to rewrite an operand into a synthetic binding
the syntax walk does not connect back to the call.

**What an operand names is its VALUE, not its syntax.** The reading descends the
value-transparent wrappers — a `Let`/`Letrec` body, a `Begin`/`Block` tail, a
branch arm, an `And`/`Or`, a `DerefCell`, a `Return` — and stops where the value
is produced, recording that node's own region because that region *is* the value
handed over. It does **not** descend a `Call`'s callee or arguments, nor a
`Lambda`'s captures: a region reached only in there is one the operand's own
evaluation used and finished with before the tail call was made, and exempting
it leaves a release the frame still owes emitted where control never arrives.
`(f (g x))` hands the callee `g`'s **result**; `g`'s own closure region is not
reachable from the call at all. What the produced value does still hold, it
holds by a **counted** (or owning) edge in each case: a call's result carries
exactly one minted reference ([the return mint](mechanism.md)), and a closure's
env took the funnel's count when it was built ([the branch-arm
window](window.md)) — so the frame's own release remains the only reference it
drops. An inline `%`-opcode is not such a node: it mints no region and its heap
result (`%first`/`%rest`/`%get`) is an uncounted borrow living *in* its
operand's region, so the operand is the value-producing leaf and the descent
continues through it. This is the same reading the closure-cycle merge's by-move
boundary makes of the same question ([letrec.md](letrec.md)), for the same
reason.

Producing a value is not the same as producing a *fresh* one — a callee may hand
back an argument itself or a value it read out of one ([adopt.md](adopt.md)) —
and that costs the reading nothing, because the mint is per *value*, not per
freshness: whichever region the result turns out to live in, the callee raised
**that** region's count by exactly one on the way out ([the return
mint](mechanism.md)). So the frame's own release still drops only the frame's
reference, and the moved value survives it. The one node with no such count is
the inline `%`-opcode above, which is why the descent passes through it to the
operand that owns the page.

**A leaf is a part, not the whole.** What the exemption withholds is the frame's
own release, on the strength of the callee's owned-parameter release standing in
for it — so the two must name one reference. A **destructured leaf** is where they
come apart. `(let [[a b] t] (f a b))` hands `f` the leaves; `t` itself never
reaches the call, and each leaf is an element with a region of its own, which is
the region the callee's release names. Yet a leaf carries the source's regions in
`binding_source_regions`, because a leaf MAY live inside the source, so the naive
reading withholds `t`'s release and nothing takes it over: one region per call,
which every builder of the shape
`(let [[ft fl si pl] (make-…)] (send-frame s ft fl si pl))` pays. So a leaf
argument (`RegionInfo::destructure_leaf_bindings`) is reconsidered against the
region's release ROUTE: the exemption stands only where the call passes the very
slot that route loads. Every other binding that names a region names the whole
value — an alias binder is a second name for the reference the call moves, and
hoisting its release ahead of the call would free what the callee is about to take
over (stdlib `zip`'s `arrs`, a second name for the array an inner `let` returned).
Pinned by `tests/elle/region-tailcall-arg-transfer.lisp`, whose alias case is the
counter-factual for reading a leaf's rule onto a whole.

**Whether the frame holds the region alone** — the admission, and escape is its
sole authority. The exemption above is a statement about *arguments*, and
arguments are not the only path into a callee: a tail callee reaches its
**captured environment** too, which no argument names and no callee region
describes. `push-all`'s walker is exactly that shape — `(letrec [go (fn [i] …
dst)] (go 0))` names `dst` only through `go`'s env. That path needs no
enumeration and no refusal of its own, because the env's hold is a counted (or
owning) edge the funnel took when the closure was built ([the branch-arm
window](window.md)): a release of the frame's reference leaves the callee's
standing. The predicate is one and the same for both mechanisms
(`RegionInfo::frame_held_regions`): every holder binding escaping by the
**return** facet at most, the region's own release route unmutated — or naming a
cell box rather than a slot ([the branch-arm window](window.md)) — and the
region absent from the fiber frontier's atomless site half.

So this close covers a parameter or local the frame alone owns — captured by a
locally-called closure or not — whose release lands at the body's scope exit, and
with it the **env cell** of a captured local, whose `DecrefCellRegion` lands in
the same dead block. Why the return facet rides along rather than refusing is the
next section.

### The callee's return mint, and why the point owes it nothing

The shape that makes the return facet look like a refusal is the same walker one
parameter over: `push-all` returns `dst` through `go`. A relocated release is safe
when the reference it drops is not the region's last *live* one. For a value the
callee merely **reads** — the walker's `src` — the frame's release is the last one
and nothing reads the region after the frame is gone. For a value the callee
**returns**, the caller does read it afterwards, through a reference the callee's
own `Return` mints — and that mint fires *after* the relocated release. Between
the two the count must not reach zero.

Nothing can put it there, and the reason is the enumeration the exemption already
rests on. A callee reaches a value this frame owns by exactly two routes: as an
**operand**, where the release stays in the dead block and is the ownership move;
or through its **captured environment**, where the funnel took a counted (or
owning) edge when the closure was built. Both ends of that enumeration are safe:

- a callee neither route reaches cannot name the region at all, so its `Return`
  mints nothing against it and the frame's release is the region's last;
- a callee the second route reaches holds a count that the closure region's
  free-time cascade drops only at the callee's *completion*, after its `Return`. So
  the order over one call is: env edge taken, frame release, callee mint, env edge
  falls away — and the reference left standing is the caller's.

The admission is therefore a fact about the **region**, not about the point: a
region whose only escape facet is the return one is relocated wherever its release
lands, and what the point still decides is the exemption alone. That the callee's
captures are usually unknowable is what makes reading both routes the right
reading rather than a weaker one — this compilation resolves a `Var` callee to a
lambda in this unit and no further, so an imported or parameter callee's captures
are invisible, and a capture is counted however little of it the compiler can see.

Every other facet still refuses, and each for the reason it always did: a holder
that crosses the **fiber** frontier may be borrowed uncounted by a parked frame; a
region whose own **route** binding is mutated has a release that frees whatever the
slot holds then, except where the release names the cell box the mutation leaves
alone ([the branch-arm window](window.md)); a
holder captured by a closure that **escapes** leaves with it. What is dropped is
the return facet's refusal and nothing else — which is why escape must be able to
say "*this* facet and no other" (`EscapeInfo::binding_escapes_beyond_return`, the
complement of `binding_escapes_via_return`).

The everyday shape this reaches is the index-walk fold driver — `fold`, `reduce`
and `concat` all walk with it:

```
(fn [f n i acc] (if (%lt i n) (recur f n (%add i 1) (f acc i)) acc))
```

The base arm returns `acc`, so the region is on the return frontier. The recursive
arm hands the callee the *combiner's* result rather than `acc` itself, so neither
route reaches the accumulator at that point and the frame's release is its last —
which is what frees each displaced accumulator per step rather than per call.

### A compiled capture cell is frame-held exactly as its binding is

The admission reads the frame's holders through `binding_source_regions`, so a
region **no binding names** offers nothing to judge and is refused. A compiled
**capture cell** (`begin_cell_regions`) is exactly such a region: it is minted at
the scope that prebinds it — the `Letrec` of a binding some *sibling* closure
captures ([letrec.md](letrec.md)) — and the binding names the
closure region the cell points *at*, never the cell's own. So the cell's
`DecrefRegion`, which the solver places at that binding scope, is stranded whenever
the scope's body ends in a frame-replacing tail call, and it takes the closure down
with it: the cell's reference is what keeps that closure's region off zero, so the
closure leaks *behind* the cell even where its own release relocated cleanly. The
everyday shape is a pair of local helpers where one calls the other and the body
tail-calls the caller —
`(letrec [helper (fn [x] …) go (fn [m] (helper m))] (go n))`. Where that caller is
also **self-recursive** the projection is not what reclaims the cell: the ownership
forest's capture adopt claims it into the capturer's closure region and suppresses
its own decref (`capture_adopt_edges`), so the capturer's stranded-self deferral
takes the pair down together. Both are pinned, so neither channel can quietly
become the other's.

The fact that settles it is that the cell's holders are its binding's holders,
one indirection out. The frame holds the cell through its own static slot; every
other holder is a closure that captures the binding, and that hold is the
counted (or owning) edge the funnel takes at the cell store ([the branch-arm
window](window.md)). No route reaches the cell that does not reach the binding —
a `DerefCell` read goes *through* the cell to get at the closure — so whatever
escape says about the binding's regions it says about the cell's, by every facet
and by the mutated-holder reading alike. Projecting each binding's single
compiled cell region (`RegionInfo::single_cell_region_of`) alongside its
`binding_source_regions` therefore asserts no admission the predicate was not
already making; it names a region the predicate could not see. Without the
projection the cell is refused for want of a holder and strands the closure it
holds — one region short of the cascade, so the helper pair leaks whole.

A binding with more than one compiled cell — a file-body/nested-`begin`
double-declare — is refused: `single_cell_region_of` yields `None`, so the
admission agrees with the `AdoptCellRegion` emit to refuse rather than guess which
physical cell a given closure holds.

This is the cell of a **prebound forward reference**, not the env cell of a
reassigned capture: that one is a `cell_release_regions` member whose release names
the box through `LoadCaptureRaw` + `DecrefCellRegion`, and it is already frame-held
because the binding names its own region ([the branch-arm window](window.md)).

### A move that crosses a read through the cell it frees is declined

A captured binding's value and its env cell are addressed by one env index, and
they are two **regions**. The relocation decides per region, so the pair can get
different answers, and moving one of them alone inverts the order between them.
The value release loads the box RAW and lets `result_region_of` unwrap it, so it
READS the page the box's `DecrefCellRegion` frees ([bindings.md](bindings.md)). Move
the cell's release ahead of the `TailCall` while the value's stays behind, and the
unwrap reads a reclaimed page.

The everyday split is the admission's own holder rule. It judges a region through
the bindings that name it, so a value region **no binding names** is refused for
want of a holder, while the cell region is admitted on its binding's verdict
(§ "A compiled capture cell is frame-held exactly as its binding is"). An
`Immediate`-effect native's result is such a region: the walk records no result
region for the call, and the lowerer still routes the binding's release through the
env index.

Neither reading the relocation already makes can see the inversion: the exemption
asks what the CALL names and the admission asks whether the frame holds the region
alone, and this obligation holds between two regions rather than between a region
and the call. So the relocation asks one more question, of the window the move
crosses rather than of the region: a run spliced from after the `TailCall` to
before it crosses every instruction now between the two positions, and those
instructions say whether the move inverts anything. A `DecrefCellRegion` naming an
env index that some instruction in the window still reads — a `LoadCapture` or
`LoadCaptureRaw` at that index — declines the move and stays where the lowerer put
it.

Reading the window is enough because the clamp already fixed the emission order:
it puts the cell's `decref_point` at or after the value release's, and where both
land on one point the release order sorts the deepest read first
([rules.md](rules.md) Rule 4). So a value release that reads through the cell is
already in the window when the cell release asks to move.

Declining strands the box on the closure path — the bounded, always-legal fallback
the relocation takes for every region it refuses, and one box per activation rather
than a page freed under its own reader. The everyday shape is the closure-as-module
whose last form is a struct literal over the closures its captured defs built:
`(fn [] (def a (ptr/from-int 0)) (defn p [] a) {:p p})`. Pinned by
`a_cell_release_declines_a_move_across_a_read_through_it`, beside the admitted face
`reassigned_env_cell_release_precedes_the_frame_replacing_tail_call`, whose cell
holds an immediate and so has no release routed through it at all.

The order the clamp and this decline together hold is stated once more over the
finished emission, as a debug-only walk of every block
(`lir::lower::assert_cells_outlive_their_readers`). Each mechanism can only see its
own half, so a block that frees a cell before a read through it names a gap in
either.

### What the exemption keeps, a channel must still run

The exemption states its reason positively: the callee's own region keeps its place
in the dead block because the new activation takes the release over
(`defer_callee_release`). That is a claim about a *channel*, and it holds only where
the channel reaches the release in question. The deferral recognises a callee whose
region **demises at the call node** — the per-call local closure a body builds and
immediately calls, whose one use is the call. A letrec **member** the body tail-calls
does not fit that description: a sibling captures it, so its uses span the whole
letrec and the solver places its demise at the letrec's own scope end. The release
lands after the body — the same dead block the exemption is keeping it in — and no
channel runs it. The member's closure region strands once per call, and its
environment and captures strand behind it. The everyday shape is the mirror of the
forward-cell pair above:
`(letrec [helper (fn [x] …) go (fn [m] (helper m))] (helper (go n)))`, where the body
tail-calls the **captured sibling** rather than the capturer.

So the deferral reads the release's **placement**, not the call node alone: a tail
callee whose release the enclosing letrec emits at its scope end rides the same
channel, run once at the callee's normal completion.

The count argument is the ordering one, and it has nothing to bridge. The deferral is
a decref, not a free, and it runs *after* the callee's `Return` mint — the same
argument the cell-free self-recursive deferral makes for its own return admission
([selfrec.md](../selfrec.md)), where the
frame-exit relocation has to move a release *ahead* of the call and fund the gap. The
return facet is therefore funded, and only the **fiber** facet refuses, a parked frame
being free to hold an uncounted borrow the compiler never placed.

What the placement reading must still exclude is a release the frame does not own. A
**suppressed** decref belongs to the store or capture-adopt path that claimed the
region — deferring it decrements a count the frame never raised — which is the same
exclusion the demise reading makes through `suppressed_decref_regions`. A **closure-
cycle member** is released by the merge's own channel, which already covers every
stranding tail path of an admitted cycle ([letrec.md](letrec.md)). And the
marking is honoured only through a **non-upvalue** reference, for the reason the arena
channel is: a nested closure that captures the member completes its own activation
before the enclosing letrec's later uses, so deferring there frees the region early.

The sibling's forward **cell** is not the callee's own region, so it relocates like
any other holder and its cascade drops the `cell ⊇ closure` edge ahead of the call.
What the deferral drops afterwards is the frame's own slot reference, the last one
standing.

### A collector parameter takes the moved reference over itself

The exemption above is a claim about the **callee's owned-param release**: the
caller drops its release of a moved argument because the callee's release of
that parameter consumes the reference. A `&`, `&keys`, or `&named` parameter is
where that claim runs out. Its binding names the collected list or struct, not
the argument, and the collected value is built in a region of its own with its
own reference on each member (`alloc_obj`'s cross-region incref). So the
callee's one release frees the collection and drops the collection's reference —
never the caller's moved one. The move arrives and nothing consumes it: one
region per collected argument per call, plus its cascade.

The runtime closes it where the collection is built (`populate_env`,
src/vm/env.rs). On a **move** — a tail call or an FFI callback, the calls that
pass `own_params = false` — the surplus reference is released once the collected
value holds its own. The release is per collector *kind*-independent: what makes
the reference surplus is that the argument went into a collection rather than
into an env slot, which is true of all three.

Aliasing is what the release must not read past. One value in two argument
positions arrives with a single moved reference, and a fixed slot or an earlier
member may already consume it; a second release would free a value still in use.
So a value is released only where it occurs exactly once across the whole
argument list — leak-safe in the other direction, never mis-freeing.

An **owned** call (`own_params = true`, the ordinary non-tail call) is not this
case at all: the caller keeps its reference and releases it at the argument's own
last use, so releasing here would over-free.

`tests/elle/region-collector-arg-move.lisp` pins the rate for each collector kind
against a positional-parameter control.

