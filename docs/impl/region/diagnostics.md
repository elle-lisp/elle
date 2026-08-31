# Region diagnostics and validation

Implementation-facing: the instruments that tell correct from broken, and the
test scaffolding (the exhaustive free-cascade pin and the leak suite) that keeps
the region rules ([rules.md](rules.md)) honest.

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
  ([generations.md](generations.md)). The
  first instrument to consult: it needs no flag and no timing luck. Guardfree
  remains the oracle for derefs of *re-claimed* pages.
- **Edge-table equivalence oracle** (debug builds, always on): at each region free
  the recorded `outgoing` edge table is asserted multiset-equal to a one-time content
  scan (`find_region_cross_refs`) of the freed members
  ([ownership.md](ownership.md) § The outgoing edge table). The production
  reclamation path walks the table (O(edges), no heap scan); the content scan survives
  only as this oracle, so a missed store-funnel edge (a silent leak) or a double-record
  (a UAF) detonates at the free site, naming the region and both edge sets, instead.
- `--trace=free` / `--trace=freebt`: a free-log recording each `free_runtime_region_pages`'s pages
  and a reason; `freebt` adds a Rust backtrace at a `DecrefRegion` about to drop
  a region to 0.
- `--trace=scrub`: zero a released page's body — the spans the dying region
  wrote, sparing the header — before the pool caches it
  ([model.md](model.md) § "Page recycling"). A read through a pointer that
  outlived its region then lands on an all-zero `HeapObject` slot, whose tag
  matches no live value, so `arena::deref` panics naming the deref site. The
  cheap member of the family: `guardfree` catches a stale read at any distance
  but costs a mapping per freed page, the generation check catches a stale
  region *resolution* only while the page is unclaimed, and scrub catches a
  stale *content* read for one `memset` per freed page. Reach for it when a
  program returns a well-typed wrong answer and the leak gauges are clean.

  **The scrub and its report are separate.** The `memset` runs in any build; the
  `arena::deref` panic that reads the zeros and names the site is
  `#[cfg(debug_assertions)]`. A plain release build therefore scrubs and says
  nothing — the stale read faults somewhere near its site instead of returning a
  wrong-typed value, which is an improvement but not a report. To get the report
  out of a release build, turn debug assertions on for it:
  `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true`. The macOS CI job pairs the two
  for exactly this reason.
- `(arena/dump)`: a Lisp-level leak localiser — prints every live mortal region
  (id, RC, object count, and the object *tags* it holds) to stderr. Where
  `arena/count` / `arena/region-count` say *that* memory grew across a loop, the
  per-region tags name *what* leaked (a stray `Fiber` / `Closure` region pinning
  an unfreed value). The companion `(arena/region-info)` returns the same id / RC
  / count as data (no tags) for assertions.
- `(arena/page-claims)`: the live count of pages this heap's `RegionStore` has
  claimed from its page pool, monotonic and never decremented on release. A
  delta across a fixed window is the *page* cost of a shape, the dimension
  `arena/count` and `arena/region-count` do not show: three regions holding one
  object each own three pages, so a shape can be leak-free by object count and
  still claim a page per call ([model.md](model.md) § "Page recycling",
  [regions/performance.md](../../regions/performance.md) § "A call into a
  variadic stdlib operator allocates"). `tests/elle/region-page-recycle.lisp`
  reads it. Immediate, so sampling it allocates nothing and does not perturb
  the measurement.
- `(arena/region-ids)` and `(arena/region-table)`: the *id* dimension, which no
  other gauge can show. A minted id that never allocates holds no object, no
  page, and no reference count, so `arena/count`, `arena/bytes`,
  `arena/page-claims`, and `arena/region-count` all read flat while it strands
  ([model.md](model.md) § "Physical id recycling"). Reach for `region-ids` to
  *detect* the leak and `region-table` to size it:
  - `arena/region-ids` is `next_physical`, one past the largest id ever minted
    from scratch. A mint that finds an id on the free list leaves it alone, so a
    steady-state loop holds it flat and every unit of growth is an id that did
    not come back. A delta across a fixed window of such a loop must be zero.
  - `arena/region-table` is `regions.len()`, one past the largest id ever made
    *live*, which times the slot size is what the table costs resident. It
    **lags**: a stranded id is never materialized, so it inflates the table only
    once some later mint reaches its range. A loop whose calls allocate nothing
    can leak ids at full rate and leave this gauge flat, which is why it is the
    wrong one to assert on.

  The `id-*` probes of `tests/elle/oracle.lisp` read `region-ids`, beside a
  live-growth discriminator that proves it moves. They do not read
  `region-table`, and the reason is stronger than the lag: physical ids reach
  the store from two independent sources — the per-heap `next_physical` counter
  and raw static-slot ids (`RegionStore::new_runtime_region`) — and the table is
  sized by the largest id ever made live from *either*. The static range sits
  far above the counter, so the table's high-water mark is already past anything
  a loop driving the counter can reach, and it cannot move for such a loop at
  all. Reach for it to *size* a leak `region-ids` has already found. Both are
  Immediate, so sampling allocates nothing.
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
  geometric page growth (the base page doubling up to 4 MiB) escalates past
  `base_page()` — the precondition for the only place region attribution can be
  misled, a pointer deep inside a page larger than `base_page()`
  (`region_of_ptr`'s sub-alignment walk; the page-header magic and ownership
  validation make that search sound, so this is for *policy* analysis, not
  correctness). Off by default and zero-cost then. Each `elle test` worker
  aggregates into the one process-wide histogram (`elle test --stats …`); sum the
  lines across batched runs for a corpus-wide distribution. The size classes
  scale with the host page, so compare distributions across hosts by class, not
  by byte count ([model.md](model.md) § "The base page is the OS page").
  Measured baseline on a 4 KiB-page host: ~99.9 % of claims are one base page;
  large pages come overwhelmingly from large *single* allocations (`alloc_data`
  right-sizing a buffer), not from the doubling ladder.

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
one representative shape per residual class in a loop with a heap gauge sampled *by
the program* — `arena/count`, `arena/region-count`, `arena/bytes` or
`arena/region-ids`, chosen for the dimension the class leaks in — and prints a
per-class **closed (bounded) / open (leaking)** verdict with a measured
per-op rate. The former scattered per-pattern leak files are folded into it, so leak
state is read in one place, not a scattered suite.

The only trustworthy measurement is steady-state **residue growth** across loop
iterations: a reclaimed class is **bounded** — its `arena/count` slope is 0 — while a
leaking class grows per iteration, slope k > 0 meaning k objects leaked per iteration.
A built-in **discriminator** (a shape that legitimately retains every iteration) must
itself read *open*: a near-zero rate is real reclamation only when the discriminator
slopes up, proving the gauge is not dead. There is one per gauge, and the count is
gated, because a discriminator answers for the gauge it was measured on and for no
other: a module-level sink moves the object count and the physical-id counter on
different events, so either can be dead while the other is live. The estimator is
variance-adaptive (an
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

## The backend-tier gauge

The arena gauges (`arena/count`, `arena/region-count`, `arena/bytes`,
`arena/page-claims` — src/primitives/arena.rs) are **host-side and
tier-transparent**: a primitive call
executes on the host against the driving instance's own heap on every tier — the
VM and JIT natively, the WASM host through `call_primitive` with a `NativeCtx`
built on `vm.heap_ptr` (src/wasm/host.rs), and the MLIR tier admits no calls at
all (below). So a program that samples the gauge measures the same `RegionStore`
no matter which tier executes it, and an interpreter oracle probe ports to a
backend tier by running the same shape under the tier's flag (`--wasm=full`,
`--wasm=N`, `--mlir=eager`).

Per-tier region-reclamation state, each with its pinning test:

- **VM / JIT** — the region runtime proper; state is the oracle's closed/open
  split (`tests/elle/oracle.lisp`).
- **MLIR CPU / GPU (SPIR-V)** — **allocation-free by construction.** The
  eligibility gate (`is_gpu_eligible`, src/lir/types/mod.rs `is_gpu_instruction`)
  whitelists numeric instructions only: every instruction that can put a heap
  value in a register is refused, and with it every region instruction except
  the two value-targeted RC ops (no-ops on unboxed scalars, so admitting them
  can never unbalance a real region). No region-managed value ever lives on
  this tier; the program's heap stays with the VM, which reclaims as usual.
  Pinned by the `gpu_eligibility_*` tests in `lir::types::func::tests`;
  measured bounded by the gauge probe under `--mlir=eager`.
- **WASM full-module (`--wasm=full`)** — **a program-duration over-keep,
  pinned shrink-only.** Every region instruction is a structural no-op in the
  emitter (src/wasm/instruction/dispatch.rs), and the host mints a fresh region
  per boundary call (`rt_data_op` in src/wasm/linker/dataop.rs,
  `call_primitive`, the closure-env cell builders). A mint alone costs nothing —
  region entries materialize lazily on first allocation
  (regionstore/alloc.rs) — so the strand rate is per **allocating** boundary
  call, not per host call: every heap allocation the run makes (data-op
  results, native results, call scaffolding such as variadic rest-lists and
  capture cells) lives until process teardown. The `HandleTable`
  (src/wasm/handle.rs) is the same over-keep on the host side: a handle is
  never removed during a run, so every heap value that crosses the boundary
  pins an entry for the store's lifetime. Pinned by the `wasm::tests` gauge
  pins (`wasm_full_*`); realizing region release on this tier shrinks them
  toward the VM's zero.
- **WASM tiered (`--wasm=N`)** — the VM keeps region authority: all region
  bytecode runs interpreted, and only closures that pass the standalone
  emission gate (src/wasm/emit.rs `standalone_emittable` — no tail calls, no
  signal emission, no suspending calls, no module-less `MakeClosure`) move to
  WASM. A compiled leaf's internal allocations strand exactly as in
  full-module mode (same no-op emitter), bounded by the leaf's body size per
  call; the gate is pinned by `wasm::tests::standalone_emission_refuses_*`.

The gauge probes measure **region reclamation**, not wall-clock or handle-table
growth; the handle table is host-side Rust memory invisible to `arena/*`, named
above so its growth is not mistaken for a gauge artifact.

## The squelch/abort discard

Abandoning suspended work routes through one chokepoint, `VM::discard_suspended_frames`
(src/vm/core.rs), on every tier — the interpreter's `enforce_squelch`, `compile/run-on`'s
squelch enforcement, and the JIT call paths. The chokepoint subtree-drops each discarded
frame's parked activation owner node
([owner.md](owner.md) § "Owner nodes") and releases nothing else: the
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
([owner.md](owner.md) § "Owner nodes" — "Fiber teardown frees everything
the fiber owns"): every still-parked frame's activation owner node plus the fiber owner
node, gathered under the fiber node (`reparent_owned_children`) so the teardown is one
set-drop. An `:error` fiber is NOT torn down — it is resumable (restarts), so its parked
state must survive the promotion. The pin is two-sided, exactly as the discard's:
`runtime::tests::ownership::fiber_owner_node_*` prove the owned set IS freed at each
terminal transition (generation bumps, bounded over repeated cycles), and
`tests/elle/region-fiber-cancel.lisp` — cancel of parked fibers and abort of new ones in
a loop — under `--trace=guardfree` with the full stdlib proves the teardown frees nothing
a live frame counts on (panic-clean, bounded slope sampled by the program).
