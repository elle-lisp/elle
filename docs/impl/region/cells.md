# Capture cells

<!-- audited: 2026-09-09 -->

How a captured binding's cell is realized, what a read through one borrows, and where the cell's own release lands.

A captured binding is materialized as a **cell**, and there are two realizations.
A compiled `MakeCaptureCell` sits in the binding's own stack slot; a
`populate_env` env cell sits in the closure environment and is reached by
`StoreCapture`/`LoadCapture`. Which one a binding takes is
`BindingInner::compiled_forward_cell`, and the two differ in everything below:
where the cell's region comes from, what releases it, and what a nested closure
loads when it captures the binding.

The reassign model the cell serves is [bindings.md](bindings.md) — a cell is the
1-slot container of that document, realized. Read it first; this document is
about the realization alone.

**Captured reassigned cells.** A captured (`needs_capture`) reassigned
binding is the same 1-slot container realized at runtime — the capture
cell's update increfs the new content and decrefs the displaced prior
unconditionally; there is no fallback to suppress, because the cell's RC
semantics live in the update opcode itself. Its readers are covered by the
general rule in [bindings.md](bindings.md) ("A whole-value read of a 1-slot
container takes a counted reference"): the overwrite-release is
`capture_store_with_rebind`'s here rather than the compiler's
drop-on-overwrite, and an uncounted alias would be freed under the reader by it
(the captured-alias double-free). The obligation is
scope-independent — a fn-local `is_restorable_capture_cell` read through an
upvalue by a nested closure (the std/process scheduler's `sched-run`
`(let [batch ready] (assign ready @[]) (each pid in batch …))`, where `ready`
is a `make-scheduler` local) is exactly as exposed as a top-level `def @cell`
read, and both are pinned by
`region-reassign-captured-cell-reader.lisp`.

The writer side owes one rule of its own, at the **init**. A compiled-cell
binding's slot holds the CELL, so routing the init value's release through that
slot makes `DecrefValueRegion` reload the slot and — via `result_region_of`,
which unwraps a capture cell — free whatever the cell holds when the release
fires. Once a reassignment has repointed the cell, that is a different, live
value (the capture-cell reassign UAF). So a reassigned captured binding drops
its init's producer reference off the value register at the define
(`store_captured_cell_init`) and the cell-slot routing is skipped; the cell's
own counted reference (taken by the store, `capture_store_with_rebind`) then
holds the init until the next overwrite or the cell's free cascade.

**The reassign is a fact about the BINDING, not about where the assign sits.**
`RegionInfo::captured_reassigned_bindings` names every captured binding some
`assign` repoints, wherever that `assign` appears — including inside a closure
the definition scope encloses. The binding `results` in `(begin (var results
(list)) (defn collect () (assign results (pair 55 results))) (collect))` has a
compiled cell (its define is outside any lambda) and is repointed from inside
`collect`; classifying it by the *assign site*'s scope would call it fn-local,
leave the cell-slot routing in place, and free the reassigned value under the
program that returns it (`region-capture-cell-closure-reassign-uaf.lisp`). A
genuinely fn-local captured binding — defined inside a lambda — is unaffected
either way: its cell is a `populate_env` env cell reached by `StoreCapture`, a
path that never consults this set.

**A read through an env cell is an uncounted borrow: the cell's last use is the
READER's.** `DerefCell` wraps every read of a captured binding, and it emits no
instruction of its own — `lower_deref_cell` delegates to the cell operand and
`lower_var` unwraps the `CaptureCell` through `LoadCapture`. The value it hands
back is still the cell's content, and the load raises no count on it, so the
cell's own lifetime is the borrow's only protection. The cell owns that content
outright (`AdoptCellRegion` links it into the cell's region), so releasing the
cell cascade-frees exactly what the read borrowed out of it.

That makes the wrapper transparent to last-use, not a consumer of it. A
`DerefCell` in operand position hands its `Var` the *deref's* effective last use
— the enclosing call, `let` binding, or statement that consumes the borrow —
which is the same value the identical read of an *uncaptured* local gets, where
no wrapper stands in between. Treating the wrapper as the consumer instead ends
the cell's life at the load, one node ahead of the reader, and the reader then
derefs a page the cell's free cascade already reclaimed. This is the env-cell
statement of the rule [rules.md](rules.md) Rule 4 makes for container reads, and
the mechanism is the one `uncounted_read_sites` uses for `%get`/`%first`/`%rest`:
the container's last use is the READER's, not the read's.

The hazard is latent by construction. A freed page keeps its bytes, so the stale
read returns the right answer and the program looks correct; `--trace=scrub`
blanks a released page's body and turns the same read into a panic at the deref
site ([diagnostics.md](diagnostics.md)). `tests/region_cell_borrow.rs` runs the
shapes with scrub armed, and `tests/elle/region-capture-cell-borrow.lisp` holds
them for the plain corpus.

**Env cells in loops: release once per activation, not per iteration.** A
captured local (`needs_capture` binding defined inside a lambda) and a captured
param are materialized as a per-value env cell by `populate_env` — a
`StoreCapture` into a cell pre-allocated from `capture_locals_mask`, NOT a
compiled `MakeCaptureCell`. (One carve-out: an immutable, never-mutated,
lambda-initialized binding — the recursive-closure shape, whether a `letrec`
binding or a local `defn` — compiles its forward cell as a `MakeCaptureCell` in a
plain stack slot even inside a lambda, the same route as top level, so the
closure-cycle merge can collapse it ([letrec.md](letrec.md));
`BindingInner::compiled_forward_cell` is the
predicate, and such a binding never has an env cell to release.) That cell is
minted **exactly once per activation**
(populate_env runs once when the activation is created), regardless of any loop
the binding's `def` sits in: a `def @s` re-executed each iteration only
re-stores the cell's *content* (StoreCapture: incref the new content's region,
decref the displaced prior). Its `DecrefCellRegion` (the release of the cell
box's own region) must therefore also fire **exactly once per activation**.

Which binder introduced the local decides nothing here — what the binding IS
does. `def @s` and `let [@s …]` inside a lambda are both `@`-mutable, so both are
outside the carve-out above and both are the same env cell, minted the same way
and released the same way; both binders record the cell placeholder that arms the
release — the
`Let` arm of [`region::infer::walk`](../../../src/hir/region/infer/walk.rs) and
`lower_let` beside their `Define` twins. A binder that records it for one and not
the other leaks one region and one object per activation per such local, which is
the cost of the closure-as-module idiom: a constructor returning a struct of
closures over its own mutable state pays it once per field, on every
construction (`tests/elle/region-let-capture-cell-leak.lisp`).

The binding-chain `decref_point` extension places a cell-release region's
release at the binding's last use. When the only use is a capture by a closure
built inside a loop, that last use sits *inside the loop body*, so the release
fires every iteration. For a closure that is **called in place and dies within
the iteration**, each iteration nets the cell box region `-1`: `+1` for the
closure's capture-incref, `-1` for the closure's free-time cascade, `-1` for the
per-iteration `DecrefCellRegion`. The box is allocated once (`+1`), so it is
freed at the end of iteration 1, and iteration 2 reads the freed (and recycled)
cell — a use-after-free (`as_capture_cell` deref tag mismatch under the plain
VM, a cascade free under `--trace=guardfree`). This bites whether the loop is
single or nested; a binding bound *between* two nested loops (the `(cap2)` shape,
`region-capture-cell-loop-uaf.lisp`) is just one instance. An *escaping* closure does not fault only because its
capture-incref outlives the iteration, masking the over-release as an
accidental balance.

The fix is a release-placement rule, not a new mechanism: a cell-release
region's `decref_point` is hoisted to the **outermost enclosing `While`/`Loop`
node**, which the lowerer emits *after* the loop (the proven post-loop emission
point the bound-outside `capture_loop_ext` extension already uses). The hoist is
sound for every env cell — the box is never re-allocated per iteration, so a
once-per-activation release can only over-keep (until the loop exits), never
mis-free. It composes with the closure-capture incref: an escaping closure's
reference keeps the box alive past the post-loop `DecrefCellRegion`, so the box
dies with the last surviving closure rather than at the loop. Contrast the
ordinary value-binding case: a value *bound inside* a loop IS re-allocated per
iteration, so its release must stay per-iteration — which is exactly why the
`capture_loop_ext` "bound outside" guard refuses to hoist non-cell regions. Env
cells are the exception that guard does not cover, because their allocation is
loop-independent.

The rule is about how many times the release RUNS, so it binds every mechanism
that places one. A branch whose arms each loop over the cell's holder gives the
cell a second placement: the arms that do not hold the region's `decref_point`
get a compensating release of their own, placed after that arm's last use
([`region::infer::compensate`](../../../src/hir/region/infer/compensate.rs)).
Where the arm's use is inside a loop, that point is per-iteration and frees the
once-per-activation box on iteration 1, exactly as the unhoisted `decref_point`
does. So the compensating release takes the same hoist, to the outermost
`While`/`Loop` **contained in that arm** — the loop the arm can host, since a
loop enclosing the whole branch is refused upstream by the loop-invariant guard.
`each` splices its body into one arm per sequence type, which is how ordinary
code reaches this shape; both are pinned by
`tests/elle/region-capture-cell-loop-uaf.lisp`.

The same "the box is not the slot" fact carries the cell's release past the other
placement rule it meets. A frame that ends in a closure tail call runs nothing the
lowerer emits after the `TailCall`, so a `DecrefCellRegion` landing there is
carried back ahead of the call under the frame-held admission
([mechanism.md](mechanism.md) § "A release past a frame-replacing tail call is not
a release"). That admission refuses a **mutated** holder — but only because a
value-routed release reads the holder's slot, and this release reads the box,
which no `assign` repoints (mechanism.md § "A mutated holder poisons its value
route, not its cell box"). So the env cell of a *reassigned* capture relocates
exactly as an unreassigned one does; refusing it strands one box per activation.

**A cell's release lands at or after every release routed through that cell.** A
captured binding whose init allocates owes two releases, and for an env-celled
binding both are addressed by the same env index. The init value's
`DecrefValueRegion` loads the cell RAW and lets `result_region_of` unwrap it to
the content, so it READS the cell's page; the box's `DecrefCellRegion` frees that
page. The value release therefore has to be emitted first.

Where the two land on one `decref_point` the release order already says so: a
`DecrefValueRegion` that unwraps a cell reads deepest and sorts ahead of the
`DecrefCellRegion` that frees the page ([rules.md](rules.md) Rule 4). Across two
points nothing does, and the two points genuinely diverge. Both regions ride the
binding's uses through the binding-chain extension, so they start together; the
value region then takes a second, later bound from its **allocation site's** last
use, which follows the `def` form's own value out to whatever consumes it. The
cell region is a phantom placeholder with no allocation site, so it keeps the
binding-use bound alone. The box is then freed at the capture while the value
release still has the enclosing statement to reach, and that release unwraps a
reclaimed page — a stray release of whatever region id the recycled page spells.

The rule is a clamp, run after every other `decref_point` pass: a cell-release
region's release lands at or after the release of the region a value route reads
through that cell. That is the region the binding's own binder ALLOCATED — the
one entry `record_region_slot` makes against the binder's slot, which for an
env-celled binding is the env index. A region the binding merely NAMES records no
slot and is released by id, reading no page: `(def @c n)` names its parameter's
phantom region and allocates nothing, so its box owes that region's release
nothing. The clamp is a maximum like every other pin, so it only moves the box
release later, and the point it produces is taken out of any enclosing loop by
the once-per-activation hoist above.
