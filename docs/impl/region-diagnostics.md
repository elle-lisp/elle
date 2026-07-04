# Region diagnostics and validation

Implementation-facing: the instruments that tell correct from broken, and the
test scaffolding (the exhaustive free-cascade pin and the leak suite) that keeps
the region rules ([region-rules.md](region-rules.md)) honest.

## Diagnostics — telling correct from broken

- `--trace=guardfree`: the use-after-free oracle. Freed pages are
  `mprotect(PROT_NONE)`'d and leaked, so the first real UAF faults at the exact
  deref; a handler attributes the address to the freeing region and site. Armed
  after stdlib init so benign init-time frees don't trip it. This is the only
  trustworthy UAF signal — plain free-site use checks have false positives, and
  `--trace=rc` perturbs timing enough to mask timing-dependent frees.
- **Generation panic** (debug builds, always on): `region_of` on a value whose
  region was freed panics deterministically at the deref, naming the page's
  stamped generation and the id's current one
  ([region-generations.md](region-generations.md)). The
  first instrument to consult: it needs no flag and no timing luck. Guardfree
  remains the oracle for derefs of *re-claimed* pages.
- **Edge-table equivalence oracle** (debug builds, always on): at each region free
  the recorded `outgoing` edge table is asserted multiset-equal to a one-time content
  scan (`find_region_cross_refs`) of the freed members
  ([region-model.md](region-model.md) § The outgoing edge table). The production
  reclamation path walks the table (O(edges), no heap scan); the content scan survives
  only as this oracle, so a missed store-funnel edge (a silent leak) or a double-record
  (a UAF) detonates at the free site, naming the region and both edge sets, instead.
- `--trace=free` / `--trace=freebt`: a free-log recording each `free_runtime_region_pages`'s pages
  and a reason; `freebt` adds a Rust backtrace at a `DecrefRegion` about to drop
  a region to 0.
- `(arena/dump)`: a Lisp-level leak localiser — prints every live mortal region
  (id, RC, object count, and the object *tags* it holds) to stderr. Where
  `arena/count` / `arena/region-count` say *that* memory grew across a loop, the
  per-region tags name *what* leaked (a stray `Fiber` / `Closure` region pinning
  an unfreed value). The companion `(arena/region-info)` returns the same id / RC
  / count as data (no tags) for assertions.
- **Direct vs cascade free** — the two have different fixes. A *direct* free of a
  still-live value is a liveness bug: a `decref_point` fired while the value was still
  reachable. A *cascade* free of a still-referenced region is a missing incref on
  the referrer: an escape site from Rule 5 was not covered.
- `arena.rs` tag/object mismatch = a UAF surfacing as a wrong-tag deref;
  `regionstore/refcount.rs` phantom/double-free assert (`decref_with_cascade`) =
  a `DecrefRegion` for a region never allocated or already freed.
- `--stats`: prints exit-time statistics, including a **page-claim size
  histogram** — one `[stats] page-claim size=<bytes> claims=<n> bytes=<n>` line
  per size class (`size=0` = the oversized one-off bucket). It measures how often
  geometric page growth (4 KiB doubling up to 4 MiB) escalates past `BASE_PAGE` —
  the precondition for the only place region attribution can be misled, a pointer
  deep inside a page larger than `BASE_PAGE` (`region_of_ptr`'s sub-alignment
  walk; the page-header magic and ownership validation make that search sound, so
  this is for *policy* analysis, not correctness). Off by default and zero-cost
  then. Each `elle test` worker aggregates into the one process-wide histogram
  (`elle test --stats …`); sum the lines across batched runs for a corpus-wide
  distribution. Measured baseline: ~99.9 % of claims are 4 KiB; large pages come
  overwhelmingly from large *single* allocations (`alloc_data` right-sizing a
  buffer), not from the doubling ladder.

## Validation

The free-cascade scan is pinned exhaustively
(`exhaustive_scan_finds_cross_region_refs_in_every_variant`,
regionpool/introspect.rs): one of each `HeapObject` variant is constructed
with a cross-region `Value` in every channel it has (contents and `traits`
alike) and the scan must report the edge; variants with no channel must
report none. The construction is an exhaustive `match` — a new variant does
not compile without a scan decision, and a wrong decision fails the pin, not
review (Rule 7's "complete and symmetric" made mechanical). Known boundary:
an `External`'s `Rc<dyn Any>` payload is opaque by construction — a plugin
that stores region `Value`s inside it hides them from the scan; `External`
participates only through its `traits` edge. This same scan is, in debug builds,
the **edge-table equivalence oracle**'s reference (above): its exhaustiveness over
every variant is what makes the recorded-`outgoing`-vs-scan assertion at free a
*complete* check, not a partial one — a content edge the scan can see but the
recorder forgot is caught the moment that region frees.

The **leak state** lives in one runnable dashboard, `tests/elle/oracle.lisp`. It runs
one representative shape per residual class in a loop with `arena/count` (and
`arena/bytes`) sampled *by the program*, each beside a known-live-growth discriminator,
and prints a per-class **closed (bounded) / open (leaking)** verdict with a measured
per-op rate. The former scattered per-pattern leak files are folded into it, so leak
state is read in one place, not a scattered suite.

The only trustworthy measurement is steady-state **residue growth** across loop
iterations: a reclaimed class is **bounded** — its `arena/count` slope is 0 — while a
leaking class grows per iteration, slope k > 0 meaning k objects leaked per iteration.
A built-in **discriminator** (a shape that legitimately retains every iteration) must
itself read *open*: a near-zero rate is real reclamation only when the discriminator
slopes up, proving the gauge is not dead. The estimator is variance-adaptive (an
empirical-Bernstein sequential bound) and block-size invariant, so a reported rate is a
true per-op rate, not a block-boundary artifact — no two-scale warmup subtraction needed.

A verdict is **shrink-only**: a class moves open → closed (or its rate shrinks) as its
mechanism lands, never the reverse, terminating at rate 0 (the boundedness assertion the
class becomes once reclaimed). The growth remains a defect by Rule 8 until then; the
oracle documents and tracks it without hiding any *other* regression behind a
known-failing test, and the gate stays green. Every fix lands with a counterfactual that
*fails before the fix* — for a leak class the oracle's probe is that test: the fix moves
its verdict and the new rate is the post-fix pin. A probe is written from the rule it
enforces, not the implementation's current output; its *magnitude* is necessarily
measured, but its *shape* — slope-based, shrink-only — is the rule.

UAF is a separate axis, gated by `--trace=guardfree` under the full stdlib (the only
trustworthy UAF oracle — plain-VM green is not evidence), not by the slope verdict.

## The squelch/abort discard

Abandoning suspended work routes through one chokepoint, `VM::discard_suspended_frames`
(src/vm/core.rs), on every tier — the interpreter's `enforce_squelch`, `compile/run-on`'s
squelch enforcement, and the JIT call paths. The chokepoint subtree-drops each discarded
frame's parked activation owner node
([region-model.md](region-model.md) § "Owner nodes") and releases nothing else: the
frame's `activation_region_map` is a borrowed view of regions that may be shared with an
outer, non-discarded frame or with the activation that catches the squelch, so a per-slot
release there over-frees (the historical squelch double-free — a non-unwinding abort in
scheduler-heavy programs); the node's members are exactly the regions adoption moved in,
so their release at the discard is sound by construction. The pin is two-sided:
`runtime::tests::ownership::discard_frees_parked_activation_owner_node` proves the node
and its members ARE freed at the discard (bounded, generation bump), and the squelch
corpus (`region-squelch-nested.lisp`, `region-loop-capture-squelch.lisp`, and the
redis-driven `redis.lisp` scheduler shape when a live Redis is present) under
`--trace=guardfree` with the full stdlib proves the discard frees nothing more
(panic-clean).

## The terminal-fiber teardown

The discard chokepoint serves the LIVE fiber (a squelch/abort abandons its own parked
chain; the fiber runs on). A fiber that reaches a **terminal** state instead — completion,
halt, `fiber/cancel`, `fiber/abort` of a not-yet-started fiber — releases everything it
owns through `take_fiber_owned` / `release_fiber_owned`
([region-model.md](region-model.md) § "Owner nodes" — "Fiber teardown frees everything
the fiber owns"): every still-parked frame's activation owner node plus the fiber owner
node, gathered under the fiber node (`reparent_owned_children`) so the teardown is one
set-drop. An `:error` fiber is NOT torn down — it is resumable (restarts), so its parked
state must survive the promotion. The pin is two-sided, exactly as the discard's:
`runtime::tests::ownership::fiber_owner_node_*` prove the owned set IS freed at each
terminal transition (generation bumps, bounded over repeated cycles), and
`tests/elle/region-fiber-cancel.lisp` — cancel of parked fibers and abort of new ones in
a loop — under `--trace=guardfree` with the full stdlib proves the teardown frees nothing
a live frame counts on (panic-clean, bounded slope sampled by the program).
