# Reads of a 1-slot container

<!-- audited: 2026-09-21 -->

What a whole-value read of a reassigned binding's container takes, and which binder forms must emit the retain.

The container model is [bindings.md](bindings.md): a reassigned mutable binding
is a 1-slot container that releases what it held at every re-store. This
document owns what a *reader* of such a container takes — the counted
reference, what a branch or statement wrapper leaves its reader holding, and
the binder forms that emit the retain. Read the model first; every term below
is defined there.

**A whole-value read of a 1-slot container takes a counted reference.** The
container releases what it held at every re-store, so a name bound to a bare
`Var` (or `DerefCell`-wrapped) read of it borrows a reference that dies at the
next overwrite. Rule 5's "new reference" pass-through ([rules.md](rules.md))
answers it: the read mints a placeholder region — a call-result region, so the
reader carries a value-based release at its own last use — and takes an
`IncrefValueRegion` at the binder (`RegionInfo::counted_cell_read_sites`,
`emit_counted_cell_read_retain`).

The predicate is **re-stored content**, not the container's realization
(`BindingInner::is_one_slot_container`). A captured cell re-stores through
`capture_store_with_rebind`, which decrefs the displaced prior; an uncelled
`@`-mutable local re-stores through the compiler's own drop-on-overwrite. Both
release the reference the read borrowed, so both expose a reader identically, and
splitting the rule by realization would leave the uncelled half relying on a
producer reference the cell may not even own. A reader that is itself celled is
exempt — its own store opcode owns its references.

Counting the read is also what keeps the **donation**
([bindings.md](bindings.md)) available. The reader holds a reference of its
own, so it is not a holder of the container's init region, and
that region's one remaining claim is the cell's — which drop-on-overwrite and the
content drop already release. The reader's own release routes through the
reader's slot, which is bound once and never repointed, so it has an untainted
route by construction where the init region has none. The reference is the test:
`reassign_gate_counts_a_read_of_an_uncelled_cell` for the admission,
`reassign_gate_counts_an_aliased_init` for the alias that is *not* such a read and
so keeps the counted-init route, and
`tests/elle/region-cell-alias-after.lisp` for the measured shape.

An **element** read (`first`/`get`/destructuring) is not a whole-value read and
needs no counting: an element's region is independently counted by its parent's
alloc-time scan, so the parent's demise cascades rather than freeing the element
under the reader.

**A branch is a read of whichever arms read.** What obliges the reader is the
value it ends up holding, not the syntax that selected it: in
`(let [k (if c r s)] …)` the name `k` is, on every path, a borrow out of a
container that re-stores, and the retain the binder takes protects whichever
container's content arrived — `IncrefValueRegion` names the runtime value, so one
instruction covers every arm. The two containers each keep their donation, and
correctly: on the path that did not run, the reader never became a holder of that
container's init.

A **mixed** branch — one arm reading a container, another allocating — takes the
same retain, and the replacement it pays with is per-arm. What the counted read
does is *replace* the reader's source regions with the placeholder, which is how
the reader stops being a holder; for an allocating arm those source regions are
the only thing extending that value's last use out to the reader, so cutting them
would put the arm's own release ahead of the binder's retain. So the descent cuts
the regions of the arms that read a container and **keeps** every other arm's.
Both halves stay balanced, because one `IncrefValueRegion` names whatever value
arrived: on a reading path the retain and the placeholder's release are the
reader's whole account, and the container's own drop-on-overwrite releases what
it holds; on an allocating path the value carries two references — its birth and
the retain — against two releases, that arm's ordinary decref at the reader's
last use and the placeholder's. What the reader is left holding is exactly what
each half needs: the allocating arm's regions, so its value stays extended, and
none of the container's, so the container is its init's sole holder and the
donation runs.

A statement wrapper is descended for the same reason, with one path rather than
several: `(let [k (begin (log) r)] …)` leaves `k` holding exactly what the tail
read, because the walk gives a `Begin` its last expression's regions and nothing
else. What obliges the reader is the value it ends up holding, not the syntax
that selected it, and a `begin` selects one exactly as an arm does.

A branch NO arm of which reads a container is not a read of anything and declines
as any other init does. A path with no value at all — a `Cond` without an else
clause — is one of the arms that read nothing: it contributes no source region to
keep, and carries no reference for the retain or the placeholder release to name,
so both are no-ops on it. (A `Match` needs no else — an unmatched value signals
rather than falling through to one — so its arms *are* every value-producing
path.)

**A version of the container is not an alias of it.** The reader's own source
name is excluded from the arms, because functionalization's `fresh_version` keeps
the name: `(let [x (if c x x)] …)` is the SSA phi carrying `x`'s content past a
conditional `assign`, and every later read of `x` resolves to it. That is the
same forwarding edge "A loop parameter's init source is not a second holder"
([bindings.md](bindings.md)) describes at a loop — the versions hold **one**
reference between them because a `Var` read mints nothing — so counting the phi
would claim a second reference for a single holding, and the container's returned-binding
suppression would then run against a reader that had not paid for it. So a
version arm is one of the arms that read nothing, and its regions are among the
ones the descent keeps — the phi hands its one reference along exactly as before,
and the retain and the placeholder release that name the runtime value on that
path balance each other. A user rebinding that shadows the container reads as a
version too. A branch of nothing but versions is a read of nothing and declines
whole: the reader keeps holding the container's region and the container keeps
the counted-init route, which costs promptness only.

The reference is the test: `reassign_gate_counts_a_branch_read_of_a_container`
for the admission, `reassign_gate_counts_a_mixed_branch_init` for the mixed
branch whose allocating arm keeps its regions,
`reassign_gate_counts_a_begin_wrapped_read` for the statement wrapper,
`reassign_gate_declines_a_branch_reading_no_container` for the decline,
`reassign_gate_counts_a_phi_carried_returned_value` for the phi that must stay
uncounted, and `tests/elle/region-cell-alias-branch.lisp` for the measured
shape.

**Every binder form that records the read must emit the retain.** The analysis
side is one function reached from both binder arms of the walk, and what it
records is worth a *release* — the placeholder's value-based decref at the
reader's last use — plus, at the container, a *donation* the reader's own
reference is what pays for. A binder that recorded the read and emitted no retain
would therefore run both halves of the bargain against a reference nobody took:
the container's first overwrite frees the value under the reader, and the
reader's own release then decrefs it a second time. So `lower_let` and
`lower_letrec` each call `emit_counted_cell_read_retain` at the same point — with
the read value on the operand-stack top, ahead of the slot store — and the
file-letrec binder that carries a module-scope reader is covered exactly as a
fn-local `let` is. `Define` records no read site at all, so a `def`-bound reader
stays a holder of the container's init region and the container keeps the
counted-init route. The reference is the test:
`region_container_read_toplevel_uaf` for the module-scope binder.
