# Capture cells

<!-- audited: 2026-09-23 -->

How a captured binding's cell is realized, what a read through one borrows, and where the cell's own release lands.

A captured binding lives in a **cell**, and the cell has two realizations. A
compiled `MakeCaptureCell` sits in the binding's own stack slot. A
`populate_env` env cell sits in the closure environment, and the code reaches it
by `StoreCapture`/`LoadCapture`. `BindingInner::compiled_forward_cell` chooses
the realization for `def`, `var` and `letrec`; `let` takes a compiled cell
outside a lambda and an env cell inside one. The two realizations differ in
everything below: where the cell's region comes from, what releases it, and what
a nested closure loads when it captures the binding.

The reassign model the cell serves is [bindings.md](bindings.md) — a cell is the
1-slot container of that document, realized. Read it first; this document is
about the realization alone.

**Captured reassigned cells.** A captured (`needs_capture`) reassigned binding
is the same 1-slot container, realized at runtime. The capture cell's update
increfs the new content and decrefs the displaced prior itself. There is no
fallback to suppress, because the update opcode carries the cell's RC semantics.
The general rule in [reads.md](reads.md) covers its readers: "A whole-value read
of a 1-slot container takes a counted reference".

Here `capture_store_with_rebind` does the overwrite-release in place of the
compiler's drop-on-overwrite, and it frees an uncounted alias under the reader
(the captured-alias double-free). The obligation does not depend on scope. A
fn-local `is_restorable_capture_cell` read through an upvalue by a nested closure
is exactly as exposed as a top-level `def @cell` read. The fn-local shape is
`(let [batch ready] (assign ready @[]) …)`, where `ready` is a local of the
enclosing fn.
[region-reassign-captured-cell-reader.lisp](../../../tests/elle/region-reassign-captured-cell-reader.lisp)
pins both.

The writer side owes one rule of its own, at the **init**. A celled binding's
slot holds the CELL, so a release of the init value routed through that slot
makes `DecrefValueRegion` reload the cell. `result_region_of` unwraps a capture
cell, so the release frees whatever the cell holds when it fires. Once a
reassignment has repointed the cell, that is a different, live value (the
capture-cell reassign UAF). So the binder of a reassigned captured binding skips
the cell-slot routing.

**Both realizations route through the slot, so both owe the rule.** The question
the rule answers is what the slot names at the release, and neither cell answers
"the init" once an `assign` has run. The lowerer reloads a compiled cell with
`LoadLocal`, and an env cell RAW with `LoadCaptureRaw`, so the same unwrap
reaches the content (`emit_decrefs_for`). A rule stated for the compiled cell
alone leaves a `@`-mutable captured local, an env cell, freeing its live content
at the binding's last use.

The two realizations differ in what the binder does with the init after the
skip. A compiled cell's binder drops the init's producer reference off the value
register, in `store_captured_cell_init`. The cell's own counted reference, taken
by the `StoreCaptureCell` store through `capture_store_with_rebind`, then holds
the init until the next overwrite or the cell's free cascade. An env cell's
binder emits a bare `StoreCapture` and never calls `store_captured_cell_init`,
so for an env cell the skip is the whole rule. With no slot recorded,
`emit_decref_for_region` releases a call result off the result register only
where the call discards it, and releases any other region by region id.

**Every binder that mints the cell owes the rule too.** The slot is the
binding's, so the rule binds wherever the binding was bound: `def` and `var`
through the `Begin` pre-pass, `letrec` through its own, and `let`. A binder that
keeps the routing and takes no init drop frees the cell's content while the cell
still holds it. Any later reader of the cell then reads that freed content,
including a closure the defining scope encloses. A spawned fiber that writes a
captured `@`-mutable buffer to a port reaches this shape
([region-capture-cell-let-reassign-uaf.lisp](../../../tests/integration/fixtures/region-capture-cell-let-reassign-uaf.lisp)).

`let` mints its compiled cell holding NIL and stores the init through it with
`StoreCaptureCell`, the shape the other two binders take. So all three binders
reach `store_captured_cell_init`, and all three take the cell's membership
reference from that store. A cell minted with the init already in it would
consume the init's register, and the init drop would then have no operand.

**The reassign is a fact about the BINDING, not about where the assign sits.**
`RegionInfo::captured_reassigned_bindings` names every captured binding some
`assign` repoints, wherever that `assign` appears. That includes an `assign`
inside a closure the definition scope encloses. The binding `results` in
`(begin (var results (list)) (defn collect () (assign results (pair 55 results))) (collect))`
has a compiled cell, because its define is outside any lambda, and `collect`
repoints it. A classification by the *assign site*'s scope would call it
fn-local and leave the cell-slot routing in place. The routed release then frees
the reassigned value under the program that returns it
([region-capture-cell-closure-reassign-uaf.lisp](../../../tests/integration/fixtures/region-capture-cell-closure-reassign-uaf.lisp)).

A fn-local captured binding, defined inside a lambda, enters the set on the same
terms. `lower_define`, `lower_let` and `lower_letrec` read the set before they
store the init, whichever realization the binding takes. For an env cell, the
set decides whether the binder records the init against the env slot. The
cell-release clamp below skips every member of the set.

**A read through an env cell is an uncounted borrow: the cell's last use is the
READER's.** `DerefCell` wraps every read of a captured binding and emits no
instruction of its own: `lower_deref_cell` delegates to the cell operand, and
`lower_var` unwraps the `CaptureCell` through `LoadCapture`. The value it hands
back is still the cell's content, and the load raises no count on it, so the
cell's own lifetime is the borrow's only protection. The cell holds a counted
reference to that content. `StoreCapture` takes it through
`capture_store_with_rebind`, which increfs the content's region and records an
outgoing edge from the cell's region, and the cell's free cascade releases it.
So releasing the cell can free exactly what the read borrowed from it.

The wrapper therefore forwards last use and does not consume it. A
`DerefCell` in operand position hands its `Var` the *deref's* effective last use:
the enclosing call, `let` binding, or statement that consumes the borrow. The
identical read of an *uncaptured* local, where no wrapper stands between, gets
the same value. A wrapper treated as the consumer ends the cell's life at the
load, one node ahead of the reader, which then derefs a page the cell's free
cascade already reclaimed. This is the env-cell statement of the rule
[rules.md](rules.md) Rule 4 makes for container reads, and `uncounted_read_sites`
uses the same mechanism for `%get`/`%first`/`%rest`.

The hazard is latent by construction. A freed page keeps its bytes, so the stale
read returns the right answer and the program looks correct. `--trace=scrub`
blanks a released page's body and turns the same read into a panic at the deref
site ([diagnostics.md](diagnostics.md)).
[region_cell_borrow.rs](../../../tests/region_cell_borrow.rs) runs the shapes with
scrub armed, and
[region-capture-cell-borrow.lisp](../../../tests/elle/region-capture-cell-borrow.lisp)
holds them for the plain corpus.

**Env cells in loops: release once per activation, not per iteration.** A
captured local (a `needs_capture` binding defined inside a lambda) and a captured
param each get a per-value env cell from `populate_env`, NOT a compiled
`MakeCaptureCell`. `populate_env` mints a captured local's cell holding NIL,
under `capture_locals_mask`, and the binder stores the init with `StoreCapture`.
It wraps a captured param's argument directly in its cell, under
`capture_params_mask`, with no `StoreCapture`. `populate_env` runs once, when the
VM creates the activation, so it mints the cell **exactly once per activation**,
whatever loop the binding's `def` sits in. A `def @s` that runs again each
iteration only re-stores the cell's *content* (`StoreCapture`: incref the new
content's region, decref the displaced prior).

The cell's `DecrefCellRegion`, the release of the cell box's own region, must
therefore also fire **exactly once per activation**. One carve-out applies. An
immutable, never-mutated, lambda-initialized `letrec` binding or local `defn` —
the recursive-closure shape — compiles its forward cell as a `MakeCaptureCell`
in a plain stack slot, even inside a lambda. That is the top-level route, and it
lets the closure-cycle merge collapse the cell ([letrec.md](letrec.md)).
`BindingInner::compiled_forward_cell` is the predicate, and such a binding never
has an env cell to release.

The binder that introduced the local decides nothing here; what the binding IS
decides. `def @s` and `let [@s …]` inside a lambda are both `@`-mutable, so both
are outside the carve-out above. Both are the same env cell, minted the same way
and released the same way. Both binders record the cell placeholder that arms
the release: the `Let` arm of
[`region::infer::walk`](../../../src/hir/region/infer/walk.rs) and `lower_let`,
beside their `Define` twins. A binder that records it for one and not the other
leaks one region and one object per activation per such local. That is the cost
of the closure-as-module idiom: a constructor returning a struct of closures over
its own mutable state pays it once per field, on every construction
([region-let-capture-cell-leak.lisp](../../../tests/elle/region-let-capture-cell-leak.lisp)).

The binding-chain `decref_point` extension places a cell-release region's
release at the binding's last use. When the only use is a capture by a closure
built inside a loop, that last use sits *inside the loop body*, so the release
fires every iteration. Take a closure that is **called in place and dies within
the iteration**. Each iteration nets the cell box region `-1`: `+1` for the
closure's capture-incref, `-1` for the closure's free-time cascade, `-1` for the
per-iteration `DecrefCellRegion`. `populate_env` allocates the box once (`+1`),
so the box dies at the end of iteration 1, and iteration 2 reads the freed and
recycled cell.

That use-after-free shows as an `as_capture_cell` deref tag mismatch under the
plain VM, and as a cascade free under `--trace=guardfree`. It occurs whether the
loop is single or nested. A binding bound *between* two nested loops (the
`(cap2)` shape,
[region-capture-cell-loop-uaf.lisp](../../../tests/elle/region-capture-cell-loop-uaf.lisp))
is one instance. An *escaping* closure does not fault, but only because its
capture-incref outlives the iteration and masks the over-release as an
accidental balance.

The rule is a release placement and needs no new mechanism. A cell-release
region's `decref_point` moves to the **outermost enclosing `While`/`Loop`
node**, which the lowerer emits *after* the loop. That is the post-loop emission
point the bound-outside `capture_loop_ext` extension already uses. The hoist is
sound for every env cell: `populate_env` never re-allocates the box per
iteration, so a once-per-activation release can only over-keep until the loop
exits, never mis-free. It composes with the closure-capture incref: an escaping
closure's reference keeps the box alive past the post-loop `DecrefCellRegion`,
so the box dies with the last surviving closure rather than at the loop.

Contrast the ordinary value binding. A loop re-allocates a value *bound inside*
it on every iteration, so that value's release must stay per-iteration. That is
why the `capture_loop_ext` "bound outside" guard refuses to hoist non-cell
regions. Env cells are the exception that guard does not cover, because their
allocation does not depend on the loop.

The rule is about how many times the release RUNS, so it binds every mechanism
that places one. A branch whose arms each loop over the cell's holder gives the
cell a second placement. The arms that do not hold the region's `decref_point`
get a compensating release of their own, placed after that arm's last use
([`region::infer::compensate`](../../../src/hir/region/infer/compensate.rs)).
Where the arm's use is inside a loop, that point is per-iteration and frees the
once-per-activation box on iteration 1, exactly as the unhoisted `decref_point`
does. So the compensating release takes the same hoist, to the outermost
`While`/`Loop` **contained in that arm**.

A loop that encloses the whole branch is not a point the arm can host, and it
never reaches this route. The loop-invariant guard refuses such a loop only where
the loop does not also enclose the holder's def. For an env cell the guard reads the anchor
set, which holds that def, so a loop around both the def and the branch passes
the guard. The env-cell loop hoist above excludes that case instead: it moves the
cell's `decref_point` to the loop's own node, outside every arm of the branch, so
compensation finds no arm that holds it. `each` splices its body into one arm per
sequence type, which is how ordinary code reaches this shape;
[region-capture-cell-loop-uaf.lisp](../../../tests/elle/region-capture-cell-loop-uaf.lisp)
pins both the direct form and the `each` form.

The same fact, that the box is not the slot, decides the other placement rule the
cell's release meets. A frame that ends in a closure tail call runs nothing the
lowerer emits after the `TailCall`. So the relocation moves a `DecrefCellRegion`
that lands there ahead of the call, under the frame-held admission
([relocate.md](relocate.md)). That admission refuses a **mutated** holder, but
only because a value-routed release reads the holder's slot. This release reads
the box, which no `assign` repoints ([window.md](window.md)). So the env cell of a
*reassigned* capture relocates exactly as an unreassigned one does; refusing it
strands one box per activation.

**A cell's release lands at or after every release routed through that cell.** A
captured binding whose init allocates owes two releases, and for an env-celled
binding the same env index addresses both. The init value's `DecrefValueRegion`
loads the cell RAW and lets `result_region_of` unwrap it to the content, so it
READS the cell's page. The box's `DecrefCellRegion` frees that page. The lowerer
therefore has to emit the value release first.

Where the two land on one `decref_point`, the release order already says so: a
`DecrefValueRegion` that unwraps a cell reads deepest and sorts ahead of the
`DecrefCellRegion` that frees the page ([rules.md](rules.md) Rule 4). Across two
points nothing does, and the two points do diverge. Both regions ride the
binding's uses through the binding-chain extension, so they start together. The
value region then takes a second, later bound from its **allocation site's**
last use, which follows the `def` form's own value to whatever consumes it.
The cell region is a phantom placeholder with no allocation site, so it keeps
the binding-use bound alone.

The box release then fires at the capture while the value release still has the
enclosing statement to reach. That value release unwraps a reclaimed page, and
so releases whatever region id the recycled page spells.

The rule is a clamp, run after every other `decref_point` pass: a cell-release
region's release lands at or after the release of the region a value route reads
through that cell. That is the region the binding's own binder ALLOCATED — the
one entry `record_region_slot` makes against the binder's slot, which for an
env-celled binding is the env index. A region the binding merely NAMES records no
slot, and its release by id reads no page. `(def @c n)` names its parameter's
phantom region and allocates nothing, so its box owes that region's release
nothing. A captured reassigned binding records no slot for its init either, so
the clamp skips it. The clamp is a maximum like every other pin, so it only moves
the box release later, and it applies the once-per-activation hoist above to the
point it produces.
