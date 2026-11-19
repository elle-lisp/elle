# Reads of a 1-slot container

<!-- audited: 2026-11-19 -->

What a read out of a reassigned binding's container takes, and which binder forms must emit the retain.

The container model is [bindings.md](bindings.md): a reassigned mutable binding
is a 1-slot container that releases what it held at every re-store. This
document owns what a *reader* of such a container takes. That is the counted
reference, what a branch or statement wrapper leaves its reader holding, how a
pattern reads one, and the binder forms that emit the retain. Read the model
first; every term below is defined there.

**A read of a 1-slot container takes a counted reference.** The container
releases what it held at every re-store, so a name bound to a read of it borrows
a reference that dies at the next overwrite. A *whole-value* read is a bare
`Var` of the container, or the `DerefCell` around one. An *element* read is a
`%get`, `%first` or `%rest` whose operand is a read of the container. The Rule 5
"new reference" pass-through ([rules.md](rules.md)) answers both. The read
mints a placeholder region, a call-result region, so the reader carries a
value-based release at its own last use. It takes an `IncrefValueRegion` at the
binder (`RegionInfo::counted_cell_read_sites`, `emit_counted_cell_read_retain`).
The retain names whatever value the read handed back, the container's content
or the element.

The predicate is **re-stored content**, not the container's realization
(`BindingInner::is_one_slot_container`). A captured cell re-stores through
`capture_store_with_rebind`, which decrefs the displaced prior; an uncelled
`@`-mutable local re-stores through the compiler's own drop-on-overwrite. Both
release the reference the read borrowed, so both expose a reader identically, and
splitting the rule by realization would leave the uncelled half relying on a
producer reference the cell may not even own. A reader that is itself celled is
exempt — its own store opcode owns its references.

**A loop parameter is a 1-slot container.** Functionalization turns a `while`
into a `Loop` whose parameter is a fresh version of each binding the body
assigns, and the body's `assign` targets the parameter. The predicate reads the
binding arena, so functionalization keeps the arena's record true: every
`Assign` target it emits reads `is_mutated`, and none reads `is_immutable`. A
name bound to a read of the parameter inside the loop is a counted reader. On
each pass after the first, it holds the value the previous pass stored.

**Every read counts, whatever the container holds.** The walk passes a loop body
once, so at a read inside the loop it knows only the regions the container held
before the loop. A container that starts as `nil` and stores a fresh array on
each pass shows no region at the read, yet each pass after the first reads the
array the pass before stored. So the count does not ask which regions the walk
has seen. A read of a container that only ever holds immediates takes the count
too: its retain and its release are no-ops on an immediate, and the placeholder
makes the reader's scope a live region.

Counting the read is also what keeps the **donation**
([bindings.md](bindings.md)) available. The reader holds a reference of its
own, so it is not a holder of the container's init region. That region's one
remaining claim is the cell's, which drop-on-overwrite and the content drop
already release. The reader's own release routes through the
reader's slot, which is bound once and never repointed, so it has an untainted
route by construction where the init region has none. The reference is the test:
`reassign_gate_counts_a_read_of_an_uncelled_cell` for the admission,
`reassign_gate_counts_an_aliased_init` for the alias that is *not* such a read and
so keeps the counted-init route, and
[region-cell-alias-after.lisp](../../../tests/impl/region-cell-alias-after.lisp) for the measured shape.

**An element read cannot wait for its container.** An uncounted element read
leaves the container as the element's only protection, and a borrow extends the
container's release over the reader's uses ([borrow.md](borrow.md)). A 1-slot
container's release is the drop-on-overwrite at its next store, which no
extension moves. In a loop, that store frees the value the previous pass stored,
and every element inside it. So the element read counts as a whole-value read
does, and the reader holds the placeholder and none of the container's regions.
The native `first`, `rest` and `get` take the Rule 5 pass-through retain
([rules.md](rules.md)) and need nothing from this rule.

**A branch is a read of whichever arms read.** What obliges the reader is the
value it ends up holding, not the syntax that selected it. In
`(let [k (if c r s)] …)` the name `k` is, on every path, a borrow out of a
container that re-stores. The retain the binder takes protects whichever
container's content arrived: `IncrefValueRegion` names the runtime value, so one
instruction covers every arm. The two containers each keep their donation, and
correctly: on the path that did not run, the reader never became a holder of that
container's init. An arm that is an element read is a reading arm too.

A **mixed** branch — one arm reading a container, another allocating — takes the
same retain, and the replacement it pays with is per-arm. The counted read
*replaces* the reader's source regions with the placeholder, which is how the
reader stops being a holder. For an allocating arm, those source regions alone
extend that value's last use out to the reader. Cutting them would put the
arm's own release ahead of the binder's retain. So the descent cuts
the regions of the arms that read a container and **keeps** every other arm's.

Both halves stay balanced, because one `IncrefValueRegion` names whatever value
arrived:

- on a reading path, the retain and the placeholder's release are the reader's
  whole account, and the container's own drop-on-overwrite releases what it
  holds;
- on an allocating path, the value carries two references, its birth and the
  retain. Two releases meet them: that arm's ordinary decref at the reader's
  last use, and the placeholder's.

The reader is left holding exactly what each half needs. It holds the
allocating arm's regions, so that value stays extended. It holds none of the
container's, so the container is its init's sole holder and the donation runs.

A statement wrapper is descended for the same reason, with one path rather than
several. `(let [k (begin (log) r)] …)` leaves `k` holding exactly what the tail
read, because the walk gives a `Begin` its last expression's regions and nothing
else. A `begin` selects a value exactly as an arm does.

A branch NO arm of which reads a container is not a read of anything and declines
as any other init does. A path with no value at all, such as a `Cond` without an
else clause, is one of the arms that read nothing. It contributes no source
region to keep, and carries no reference for the retain or the placeholder
release to name, so both are no-ops on it. (A `Match` needs no else — an unmatched value signals
rather than falling through to one — so its arms *are* every value-producing
path.)

**A version of the container is not an alias of it.** The reader's own source
name is excluded from the whole-value arms, because functionalization's
`fresh_version` keeps the name. `(let [x (if c x x)] …)` is the SSA phi carrying
`x`'s content past a conditional `assign`, and every later read of `x` resolves
to it. [bindings.md](bindings.md) describes the same forwarding edge at a loop
parameter's init source. The versions hold **one** reference between them,
because a `Var` read mints nothing. Counting the phi would claim a second
reference for a single holding. The container's returned-binding suppression
would then run against a reader that had not paid for it.

So a version arm is one of the arms that read nothing, and the descent keeps its
regions. The phi hands its one reference along unchanged. The retain and the
placeholder release that name the runtime value on that path balance each
other. A user rebinding that shadows the container reads as a
version too. A branch of nothing but versions is a read of nothing and declines
whole: the reader keeps holding the container's region and the container keeps
the counted-init route, which costs promptness only. An element read is never a
version, whatever its name: `(let [x (%rest x)] …)` holds a new value out of `x`
and counts.

The reference is the test: `reassign_gate_counts_a_branch_read_of_a_container`
for the admission, `reassign_gate_counts_a_mixed_branch_init` for the mixed
branch whose allocating arm keeps its regions,
`reassign_gate_counts_a_begin_wrapped_read` for the statement wrapper, and
`reassign_gate_declines_a_branch_reading_no_container` for the decline. For the
phi that must stay uncounted it is
`reassign_gate_counts_a_phi_carried_returned_value`, and
[region-cell-alias-branch.lisp](../../../tests/impl/region-cell-alias-branch.lisp)
for the measured shape. For the loop parameter, the `nil` start and the element
read the tests are `every_assign_target_reads_mutated`,
`reassign_gate_counts_a_read_of_a_loop_parameter`,
`reassign_gate_counts_a_read_of_a_nil_started_container`,
`reassign_gate_counts_an_element_read_of_a_container` and
`reassign_gate_counts_a_branch_element_read`, with
[region-container-reader-uaf.lisp](../../../tests/impl/region-container-reader-uaf.lisp)
for the measured shapes.

**A pattern reads its value through a binding of its own.** A `destructure` and
a `match` bind their names out of their value with no binder. The lowerer keeps
the value in an anonymous slot, and each name is an element read that the next
overwrite can free. So a pass after functionalization, `bind_scrutinees`, gives
the value a binding first wherever the value reads a 1-slot container on some
path. A `match` becomes `(let [v c] (match v …))`. A `destructure` becomes a
`define` of `v` and then the `destructure` of `v`, so its names stay visible to
the forms after it. The init of `v` is a counted read, and every pattern name
holds `v`'s placeholder, so the release waits for each name's last use and runs
through `v`'s slot, which nothing repoints. A `let` destructuring form needs no
rewrite: the analyzer already binds its value to `__destructure_tmp`. The
reference is the test: `a_match_on_a_container_binds_its_scrutinee`,
`a_def_pattern_on_a_container_binds_its_scrutinee`,
`a_let_pattern_on_a_container_binds_its_scrutinee` for the `let` form, and
[region-container-element-uaf.lisp](../../../tests/impl/region-container-element-uaf.lisp)
for the element reads and the patterns.

**Every binder form that records the read must emit the retain.** The analysis
side is one function reached from the three binder arms of the walk, `Let`,
`Letrec` and `Define`. What it records is worth a *release*, the placeholder's
value-based decref at the reader's last use. At the container it is also worth
a *donation*, which the reader's own reference pays for. A binder that recorded
the read and emitted no retain would therefore run both halves of the bargain
against a reference nobody took. The container's first overwrite frees the
value under the reader, and the reader's own release then decrefs it a second
time. So `lower_let`, `lower_letrec` and `lower_define` each call
`emit_counted_cell_read_retain` at the same point: with the read value on the
operand-stack top, ahead of the slot store. The file-letrec binder that carries
a module-scope reader is covered exactly as a fn-local `let` is.

A `def` in a function body is a `Define`, and its reader is counted as a
`let`-bound one is. A captured container has no counted-init route: its binder
drops an allocating init's producer reference after the cell store
([cells.md](cells.md)). So a reader with no reference of its own holds nothing
the first overwrite leaves standing. The reference is the test:
[region-container-read-toplevel-uaf.lisp](../../../tests/impl/region-container-read-toplevel-uaf.lisp)
for the module-scope binder, and
[region-reassign-captured-cell-reader.lisp](../../../tests/impl/region-reassign-captured-cell-reader.lisp)
for the `def`-bound reader.
