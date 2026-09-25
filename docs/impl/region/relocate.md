# A release past a frame-replacing tail call

<!-- audited: 2026-10-13 -->

Every release the lowerer emits after a `TailCall` is dead on the closure path,
and what it costs to move one ahead of that call.

## A release past a frame-replacing tail call is not a release

A tail call whose callee turns out to be a *closure* replaces the frame. Every
instruction the lowerer emits after the `TailCall` therefore belongs to the
**native fall-through**. A native pushes no bytecode frame, so on normal
completion the dispatch loop continues into that block and runs it
(`tail_call_inner` in
[src/vm/call/inner/tail.rs](../../../src/vm/call/inner/tail.rs)). A closure
callee never arrives there.

For a region the call's own **arguments** name, that is precisely the intent. It
is the ownership transfer the calling convention rests on ([rules.md](rules.md)
Rule 5, move-on-tail-call). The caller does not incref a moved argument, and the
release it never runs *is* the reference the callee's owned-param release
consumes. The callee's own region has the same story through a different
channel: the new activation takes over its release (`defer_callee_release`,
`deferred_release_slot`). That holds only where the channel reaches the release
(§ "What the exemption keeps, a channel must still run").

Every **other** release in that block has no such story: a parameter whose only
use is inside a closure the body builds, a parameter used nowhere at all, a
scope region the body allocated. Each has its release emitted where control
provably never arrives, and the frame's own reference is stranded. The cost is
one region per call plus everything its free cascade would have reclaimed. The
dominant witness is the stdlib helper whose body ends in a call to a local
walker. `(fn [dst src] (let [n (length src)] (letrec [go (fn [i] …)] (go 0))))`
strands both `dst` and `src` on every call, once per heap parameter the walker
captures.

The close is the one case where a release moves *earlier*: the same single
release the solver placed is emitted immediately **before** the `TailCall`
instead of after it. The scope-region half of this is already unconditional,
since `lower_call` emits the pending `DecrefRegion`s before every `TailCall` for
the same reason.

**Relocating an instruction is not by itself free of obligation.** It is
tempting to argue that nothing is added and nothing duplicated, so no count
argument is owed. That is false, and it is the per-arm route's category error
([the branch-arm window](window.md)). On the closure path the release did not
run before and now does. At runtime this *is* a new release, and it owes what
any new release owes: a reason to believe the frame holds the region's one
reference.

Two readings are therefore both required.

**What the call can reach** is the exemption, read off the call itself. Every
region named by the callee, an operand's own value, the call's own result, or
its deferred channels (`deferred_release_slot`) keeps its place in the dead
fall-through. There the ownership move and the deferred callee release own it.
The exemption is read over `alloc_region` and `binding_source_regions`, and
again over the emitted instructions. ANF is free to rewrite an operand into a
synthetic binding the syntax walk does not connect back to the call.

**What an operand names is its VALUE, not its syntax.** The reading descends
the value-transparent wrappers: a `Let`/`Letrec` body, a `Begin`/`Block` tail, a
branch arm, an `And`/`Or`, a `DerefCell`, a `Return`. It stops where the value
is produced, and records that node's own region, because that region *is* the
value handed over. It does **not** descend a `Call`'s callee or arguments, nor a
`Lambda`'s captures. A region reached only in there is one the operand's own
evaluation used and finished with before the tail call was made. Exempting it
leaves a release the frame still owes emitted where control never arrives.

`(f (g x))` hands the callee `g`'s **result**, and `g`'s own closure region is
not reachable from the call at all. What the produced value does still hold, it
holds by a **counted** (or owning) edge in each case. A call's result carries
exactly one minted reference ([the return mint](mechanism.md)). A closure's env
took the funnel's count when it was built ([the frame-held admission](held.md)).
So the frame's own release remains the only reference it drops.

An inline `%`-opcode that allocates (`%pair`, `%freeze`, `%thaw`) is such a
node too. Its result lives in the region it mints, and that result holds each
operand by the counted edge the allocation takes. Every other inline `%`-opcode
mints no region. Its heap result (`%first`/`%rest`/`%get`) is an uncounted
borrow living *in* its operand's region, so the operand is the value-producing
leaf and the descent continues through it. The closure-cycle merge's by-move
boundary reads the same question the same way, for the same reason
([cycletail.md](cycletail.md)).

The difference shows in a recursion that threads an accumulator. In
`(step (%add i 1) n (%pair i acc))`, the call receives the new pair, and the pair
holds `acc` by its own count. Descending the `%pair` would exempt `acc`'s release
too, and nothing takes that release over, so each call would strand the
accumulator it displaced. The release reaches the recursive arm at all only
because `acc` is frame-held there: escape reads the pair's counted hold as no
containment ([escape analysis](../escape.md)).

```lisp
(def step (fn [i n acc] (if (%lt i n) (step (%add i 1) n (%pair i acc)) acc)))
(defn stranded-by [f]
  "How many regions 200 runs of f leave live."
  (def r0 (arena/region-count))
  (def @j 0)
  (while (%lt j 200) (f) (assign j (%add j 1)))
  (- (arena/region-count) r0))
(assert (< (stranded-by (fn [] (length (step 0 4 ())))) 50)
        "each displaced accumulator is released before the tail call")
```

Producing a value is not the same as producing a *fresh* one. A callee may hand
back an argument itself, or a value it read out of one ([adopt.md](adopt.md)).
That costs the reading nothing, because the mint is per *value*, not per
freshness. Whichever region the result turns out to live in, the callee raised
**that** region's count by exactly one on the way out
([the return mint](mechanism.md)). So the frame's own release still drops only
the frame's reference, and the moved value survives it. The one node with no
such count is the non-allocating inline `%`-opcode above, which is why the
descent passes through it to the operand that owns the page.

**A leaf is a part, not the whole.** The exemption withholds the frame's own
release because the callee's owned-parameter release stands in for it, so the
two must name one reference. A **destructured leaf** is where they come apart.
`(let [[a b] t] (f a b))` hands `f` the leaves. `t` itself never reaches the
call, and each leaf is an element with a region of its own, which is the region
the callee's release names. Yet a leaf carries the source's regions in
`binding_source_regions`, because a leaf MAY live inside the source. So the
naive reading withholds `t`'s release, and nothing takes it over: one region per
call. Every builder of the shape
`(let [[ft fl si pl] (make-…)] (send-frame s ft fl si pl))` pays it.

So a leaf argument (`RegionInfo::destructure_leaf_bindings`) is reconsidered
against the region's release ROUTE. The exemption stands only where the call
passes the very slot that route loads. A **retained alias** is reconsidered the
same way. It is a `let`, `letrec` or `def` binding that renames a borrowed value
(`borrowed_alias_bindings`), so the call mints it a reference of its own and the
callee's release consumes that mint ([rules.md](rules.md) Rule 5). The call
moves none of the frame's references through such an argument. Each region it
names keeps its exemption only where another operand passes that region's route
slot.

The everyday retained alias is a `default`ed `&named` parameter. Its version is
a phi with two arms: the leaf of the collected struct that holds the caller's
closure, and a closure the body built. Read as a whole, the phi exempts the
built closure's release, and the mint on that arm leaks it once per call
([region-tail-alias-borrow-uaf.lisp](../../../tests/impl/region-tail-alias-borrow-uaf.lisp)).

Every other binding that names a region names the whole value. An alias binder
is a second name for the reference the call moves, and hoisting its release
ahead of the call would free what the callee is about to take over. Stdlib
`zip`'s `arrs` is one, a second name for the array an inner `let` returned.
[tests/impl/region-tailcall-arg-transfer.lisp](../../../tests/impl/region-tailcall-arg-transfer.lisp)
pins this, and its alias case is the counter-factual for reading a leaf's rule
onto a whole.

**A slot comparison cannot see a value held under two slot names.** The route
reading above rests on one value having one slot. That is true of every leaf a
pattern projects, and false of the one it BUILDS. A rest name over an array or
a struct holds a collection the destructure made. The lowerer parks that
collection in a slot of its own, so the release has a stamped route the
abandoned-frame walk can read ([anchors.md](anchors.md)). The call still passes
the binding's slot, so the comparison finds two different slots and reads the
argument as one the call does not move.

Nothing then takes the release over, and the relocation carries it ahead of the
call: `(let [[x y & r] a] (length r))` releases the collection two instructions
before `length` derefs it. The failure is a use-after-free rather than the leak
the leaf reading was written to close, so the two cases need different answers,
and one comparison cannot give both.

The answer is to ask whether the name reaches a collection the destructure
BUILT before asking which slot holds it. Such a collection is routed through a
slot of the lowerer's own that no call passes, so the comparison can only read
it as unmoved. The exemption therefore stands on that region whatever its route
loads (`RegionInfo::names_built_rest_collection`). Every other region the name
carries is the scrutinee's, which the name only borrows, and those keep the slot
comparison. One name is both things at once, which is why the reconsideration is
per region rather than per binding.

What the exemption buys differs between the two kinds of name that reach a
collection, and it is legal for both. For the **rest name**, the call receives
the collection itself. The callee's owned-param release consumes the reference,
and the exemption is the ownership move.

For a name the rest **sub-pattern** bound, `p` in `[a & [p q]]`, the call
receives an element instead. The caller holds no counted reference on an
element to move, so the two answers available are a strand and an over-free.
Relocating drops the collection's reference on the element, and the callee's
owned-param release then takes the element's last one, under its own read. The
strand is the answer. The collection is held to fiber teardown on the closure
path, at one region per call, and the destructure keeps the release it already
had on every other path ([anchors.md](anchors.md)).
[tests/impl/region-rest-pattern-slice-uaf.lisp](../../../tests/impl/region-rest-pattern-slice-uaf.lisp)
pins the fault, and
[lir::lower::tests::release::restpattern](../../../src/lir/lower/tests/release/restpattern.rs)
pins the placement.

**Whether the frame holds the region alone** is the second reading, the
admission. [The frame-held admission](held.md) owns it, and escape is its sole
authority. The exemption above is a statement about *arguments*, and arguments
are not the only path into a callee. A tail callee reaches its **captured
environment** too, which no argument names and no callee region describes.
`push-all`'s walker is that shape: `(letrec [go (fn [i] … dst)] (go 0))` names
`dst` only through `go`'s env. That path needs no enumeration and no refusal of
its own. The env's hold is a counted (or owning) edge the funnel took when the
closure was built, so a release of the frame's reference leaves the callee's
standing.

The window and this relocation read one predicate,
`RegionInfo::frame_held_regions`, and [held.md](held.md) states what it admits.

So this close covers a parameter or local the frame alone owns, captured by a
locally-called closure or not, whose release lands at the body's scope exit. It
also covers the **env cell** of a captured local, whose `DecrefCellRegion` lands
in the same dead block. The next section says why the return facet rides along
rather than refusing.

### The callee's return mint, and why the point owes it nothing

The shape that makes the return facet look like a refusal is the same walker
one parameter over: `push-all` returns `dst` through `go`. A relocated release
is safe when the reference it drops is not the region's last *live* one. For a
value the callee merely **reads**, the walker's `src`, the frame's release is the
last one, and nothing reads the region after the frame is gone. For a value the
callee **returns**, the caller does read it afterwards, through a reference the
callee's own `Return` mints. That mint fires *after* the relocated release.
Between the two, the count must not reach zero.

Nothing can put it there, for the reason the exemption already rests on. A
callee reaches a value this frame owns by exactly two routes. One is as an
**operand**, where the release stays in the dead block and is the ownership
move. The other is through its **captured environment**, where the funnel took
a counted (or owning) edge when the closure was built. Both ends of that
enumeration are safe:

- a callee neither route reaches cannot name the region at all. Its `Return`
  mints nothing against it, and the frame's release is the region's last.
- a callee the second route reaches holds a count that the closure region's
  free-time cascade drops only at the callee's *completion*, after its
  `Return`. So the order over one call is: env edge taken, frame release, callee
  mint, env edge falls away. The reference left standing is the caller's.

The admission is therefore a fact about the **region**, not about the point. A
region that escapes by no containment facet is relocated wherever its release
lands, and the point still decides the exemption alone. The callee's
captures are usually unknowable, which is why reading both routes is the right
reading rather than a weaker one. This compilation resolves a `Var` callee to a
lambda in this unit and no further, so an imported or parameter callee's
captures are invisible. A capture is counted however little of it the compiler
can see.

The other refusals stand. A region whose own **route** binding is mutated has a
release that frees whatever the slot holds then, except where the release names
the cell box the mutation leaves alone ([the branch-arm window](window.md)). A
holder captured by a closure that **escapes** leaves with it, which is escape's
containment facet. A value emitted or sent with no binding to name it is judged
by no holder. The admission reads `EscapeInfo::binding_escapes_by_containment`
per holder, so the return facet and a fiber crossing refuse nothing
([the frame-held admission](held.md)).

The everyday shape this reaches is the index-walk fold driver, `core-fold-step`
in [src/core.lisp](../../../src/core.lisp). `fold`, `reduce` and `concat` all
walk with it. A copy of it runs here:

```lisp
(def fold-step
  (fn [f arr n i acc]
    (if (%lt i n)
      (fold-step f arr n (%add i 1) (f acc (get arr i)))
      acc)))

(assert (= (fold-step + [1 2 3] 3 0 0) 6))
```

The base arm returns `acc`, so the region is on the return frontier. The
recursive arm hands the callee the *combiner's* result rather than `acc` itself,
so neither route reaches the accumulator at that point, and the frame's release
is its last. That frees each displaced accumulator per step rather than per
call.

### A compiled capture cell is frame-held exactly as its binding is

The admission reads the frame's holders through `binding_source_regions`, so a
region **no binding names** offers nothing to judge and is refused. A compiled
**capture cell** (`begin_cell_regions`) is exactly such a region. It is minted
at the scope that prebinds it, the `Letrec` of a binding some *sibling* closure
captures ([letrec.md](letrec.md)). The binding names the closure region the cell
points *at*, never the cell's own. So the cell's `DecrefRegion`, which the
solver places at that binding scope, is stranded whenever the scope's body ends
in a frame-replacing tail call. It takes the closure down with it. The cell's
reference keeps that closure's region off zero, so the closure leaks *behind*
the cell even where its own release relocated cleanly.

The everyday shape is a pair of local helpers where one calls the other and the
body tail-calls the caller:
`(letrec [helper (fn [x] …) go (fn [m] (helper m))] (go n))`. Where that caller
is also **self-recursive**, the projection is not what reclaims the cell. The
ownership forest's capture adopt claims the cell into the capturer's closure
region and suppresses its own decref (`capture_adopt_edges`). The capturer's
stranded-self deferral then takes the pair down together. Both are pinned, so
neither channel can quietly become the other's.

The cell's holders are its binding's holders, one indirection out, and that
settles it. The frame holds the cell through its own static slot. Every other
holder is a closure that captures the binding, and that hold is the counted (or
owning) edge the funnel takes at the cell store
([the frame-held admission](held.md)). No route reaches the cell that does not
reach the binding, since a `DerefCell` read goes *through* the cell to get at
the closure. So whatever escape says about the binding's regions, it says about
the cell's, by every facet and by the mutated-holder reading alike.

Projecting each binding's single compiled cell region
(`RegionInfo::single_cell_region_of`) alongside its `binding_source_regions`
therefore asserts no admission the predicate was not already making. It names a
region the predicate could not see. Without the projection, the cell is refused
for want of a holder and strands the closure it holds. That is one region short
of the cascade, so the helper pair leaks whole.

A binding with more than one compiled cell, a file-body/nested-`begin`
double-declare, is refused. `single_cell_region_of` yields `None`, so the
admission agrees with the `AdoptCellRegion` emit to refuse rather than guess
which physical cell a given closure holds.

This is the cell of a **prebound forward reference**, not the env cell of a
reassigned capture. That one is a `cell_release_regions` member whose release
names the box through `LoadCaptureRaw` + `DecrefCellRegion`. It is already
frame-held, because the binding names its own region
([the frame-held admission](held.md)).

### A move that crosses a read through the cell it frees is declined

A captured binding's value and its env cell are addressed by one env index, and
they are two **regions**. The relocation decides per region, so the pair can get
different answers, and moving one of them alone inverts the order between them.
The value release loads the box RAW and lets `result_region_of` unwrap it, so it
READS the page the box's `DecrefCellRegion` frees ([bindings.md](bindings.md)).
Move the cell's release ahead of the `TailCall` while the value's stays behind,
and the unwrap reads a reclaimed page.

The everyday split is the admission's own holder rule. It judges a region
through the bindings that name it. A value region **no binding names** is
refused for want of a holder, while the cell region is admitted on its binding's
verdict (§ "A compiled capture cell is frame-held exactly as its binding is").
An `Immediate`-effect native's result is such a region. The walk records no
result region for the call, and the lowerer still routes the binding's release
through the env index.

Neither reading the relocation already makes can see the inversion. The
exemption asks what the CALL names, and the admission asks whether the frame
holds the region alone. This obligation holds between two regions rather than
between a region and the call. So the relocation asks one more question, of the
window the move crosses rather than of the region. A run spliced from after the
`TailCall` to before it crosses every instruction now between the two positions,
and those instructions say whether the move inverts anything. A
`DecrefCellRegion` naming an env index that some instruction in the window still
reads, a `LoadCapture` or `LoadCaptureRaw` at that index, declines the move and
stays where the lowerer put it.

Reading the window is enough, because the clamp already fixed the emission
order. It puts the cell's `decref_point` at or after the value release's. Where
both land on one point, the release order sorts the deepest read first
([rules.md](rules.md) Rule 4). So a value release that reads through the cell
is already in the window when the cell release asks to move.

Declining strands the box on the closure path. That is the bounded,
always-legal fallback the relocation takes for every region it refuses, and it
costs one box per activation rather than a page freed under its own reader. The
everyday shape is the closure-as-module whose last form is a struct literal over
the closures its captured defs built:
`(fn [] (def a (ptr/from-int 0)) (defn p [] a) {:p p})`.
`a_cell_release_declines_a_move_across_a_read_through_it` pins it, in
[lir::lower::tests::release::frameexit::cells](../../../src/lir/lower/tests/release/frameexit/cells.rs).
Beside it is the admitted face,
`reassigned_env_cell_release_precedes_the_frame_replacing_tail_call`, whose cell
holds an immediate and so has no release routed through it at all.

The order the clamp and this decline together hold is stated once more over the
finished emission, as a debug-only walk of every block
(`lir::lower::assert_cells_outlive_their_readers`). Each mechanism can only see
its own half, so a block that frees a cell before a read through it names a gap
in either.

### What the exemption keeps, a channel must still run

The exemption states its reason positively. The callee's own region keeps its
place in the dead block because the new activation takes the release over
(`defer_callee_release`). That is a claim about a *channel*, and it holds only
where the channel reaches the release in question. The deferral recognizes a
callee whose region **demises at the call node**: the per-call local closure a
body builds and immediately calls, whose one use is the call.

A letrec **member** the body tail-calls does not fit that description. A
sibling captures it, so its uses span the whole letrec, and the solver places
its demise at the letrec's own scope end. The release lands after the body, in
the same dead block the exemption is keeping it in, and no channel runs it. The
member's closure region strands once per call, and its environment and captures
strand behind it. The everyday shape is the mirror of the forward-cell pair
above: `(letrec [helper (fn [x] …) go (fn [m] (helper m))] (helper (go n)))`,
where the body tail-calls the **captured sibling** rather than the capturer.

So the deferral reads the release's **placement**, not the call node alone. A
tail callee whose release the enclosing letrec emits at its scope end rides the
same channel, run once at the callee's normal completion.

The count argument is the ordering one, and it has nothing to bridge. The
deferral is a decref, not a free, and it runs *after* the callee's `Return`
mint. The cell-free self-recursive deferral makes the same argument for its own
return admission ([selfrec.md](../selfrec.md)). The frame-exit relocation, by
contrast, moves a release *ahead* of the call and has to fund the gap. The
return facet is therefore funded. Only the **fiber** facet refuses, since a
parked frame is free to hold an uncounted borrow the compiler never placed.

The placement reading must still exclude a release the frame does not own. A
**suppressed** decref belongs to the store or capture-adopt path that claimed
the region. Deferring it decrements a count the frame never raised, and the
demise reading makes the same exclusion through `suppressed_decref_regions`. A
**closure-cycle member** is released by the merge's own channel, which already
covers every stranding tail path of an admitted cycle
([cycletail.md](cycletail.md)). The marking is honored only through a
**non-upvalue** reference, for the reason the arena channel is. A nested
closure that captures the member completes its own activation before the
enclosing letrec's later uses, so deferring there frees the region early.

The sibling's forward **cell** is not the callee's own region. It relocates like
any other holder, and its cascade drops the `cell ⊇ closure` edge ahead of the
call. What the deferral drops afterwards is the frame's own slot reference, the
last one standing.

### A channel built in compiled code hands its release forward

The exemption reads the same on every tier, so the channel must run on every
tier. The interpreter records a deferral on the activation slot the tail call is
built in, and the callee reuses that activation ([owner.md](owner.md)). A
compiled caller reuses nothing. `jit_tail_call_inner` sets the pending call, and
the compiled body then pops its own region-remap frame and returns the tail-call
sentinel. The slot the helper would have recorded on is gone before the callee
starts, so a release recorded there is a release nothing runs.

The compiled tier writes both channels into a one-shot
(`VM::pending_tail_deferrals`) instead: the callee closure's region, and the
merged arena the `deferred_release_slot` resolves to.
`push_activation_region_map` drains the one-shot into the fresh dues. The next
push is always the callee's. Every sentinel consumer takes the pending call and
enters the callee through `execute_bytecode_saving_stack`, whose push is the
first one after the helper returns. The helper writes the one-shot only once the
pending call is set, so a path that fails earlier writes nothing. A hand-off no
push collects holds the region to the caller's own teardown, which is an
over-keep and never an over-free.

The everyday shape is the closure-as-module factory whose member is called back
through the struct it handed out. The caller's tail call into the member
strands the merged cycle arena, and the whole cycle plus the state it captures
grows once per call.
`runtime::tests::ownership::jit::region_ownership_defn_module_member_call_under_jit`
pins it. Its compiled-caller reading sits beside the interpreter's in
`runtime::tests::ownership::selfrec::region_ownership_reclaims_defn_module_factory_per_call`,
and the oracle's `defn-module-factory` probe gauges it per op.

### A collector parameter takes the moved reference over itself

The exemption above is a claim about the **callee's owned-param release**: the
caller drops its release of a moved argument because the callee's release of
that parameter consumes the reference. A `&`, `&keys`, or `&named` parameter is
where that claim runs out. Its binding names the collected list or struct, not
the argument. The collected value is built in a region of its own, with its own
reference on each member (`alloc_obj`'s cross-region incref). So the callee's one
release frees the collection and drops the collection's reference, never the
caller's moved one. The move arrives and nothing consumes it: one region per
collected argument per call, plus its cascade.

The runtime closes it where the collection is built (`populate_env` in
[src/vm/env.rs](../../../src/vm/env.rs)). On a **move**, the surplus reference
is released once the collected value holds its own. The calls that move are a
tail call and an FFI callback, which pass `own_params = false`. The release does
not depend on the collector kind. The reference is surplus because the argument
went into a collection rather than into an env slot, which is true of all three.

Aliasing is what the release must not read past. One value in two argument
positions arrives with a single moved reference, and a fixed slot or an earlier
member may already consume it. A second release would free a value still in use.
So a value is released only where it occurs exactly once across the whole
argument list. That is leak-safe in the other direction, and never mis-frees.

An **owned** call (`own_params = true`, the ordinary non-tail call) is not this
case at all. The caller keeps its reference and releases it at the argument's
own last use, so releasing here would over-free.

[tests/impl/region-collector-arg-move.lisp](../../../tests/impl/region-collector-arg-move.lisp)
pins the rate for each collector kind against a positional-parameter control.
