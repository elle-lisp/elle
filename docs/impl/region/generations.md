# Region generations: stale derefs detonate in debug builds

Implementation-facing: the per-region generation counter and page stamping that
turn a stale region deref into a deterministic debug-build panic at the exact
deref site. Pairs with the `--trace=guardfree` oracle described in
[diagnostics.md](diagnostics.md).

Physical region ids are recycled (a freed id returns to the mint pool) and
freed pages are cached for reuse, so a stale reference — a `Value` or region
id that outlived its region — reads *plausible* memory: the plain VM reads
stale-but-intact contents and the defect surfaces far from the deref,
timing-dependently, if at all. Generations make the common case a
deterministic panic at the exact deref:

- `RegionStore` keeps a **generation counter per physical region id**,
  starting at 0 and bumped on every path that returns the id's pages — the
  RC-zero free and wholesale teardown alike. A recycled id mints its next
  region at the bumped generation.
- Every page a region claims is **stamped** `(region_id, generation,
  store id)` in its page header at claim time, in all builds (the stamp is
  eight bytes per page; release builds pay nothing else). The store id names
  the `RegionStore` that claimed the page — each store draws a process-unique
  id at construction.
- `region_of` — the single funnel through which every runtime RC decision
  reads a value's region — **checks** the stamp under `debug_assertions`: a
  pointer whose page header carries a generation other than the store's
  current generation for that region id is a stale deref, and panics right
  there, naming both generations. The declaration oracle calls
  `region_of` on every native-call result, so every native call is a
  checkpoint for free.

What it catches deterministically: any deref of a value whose region was
freed while the page sits unclaimed in the page cache — without the generation
check, a silent stale read on the plain VM and a timing-dependent fault under
`--trace=guardfree`.

What it cannot catch: a deref of a page already **re-claimed** by a new
region — the header is restamped at claim, so the stale pointer resolves
(wrongly but self-consistently) to the new region. That window is
`--trace=guardfree`'s domain: guardfree never re-claims a freed page, so the
two instruments compose — generations make the cached-page window loud in
every debug run; guardfree makes the re-claimed window loud in dedicated
runs.

## Finding the page base soundly (not a generation matter, but it shares the funnel)

`region_of` resolves a pointer to its region through `RegionStore::region_of_ptr`,
which must first find the pointer's **page base** — pages are variable-sized
(geometric growth to 4 MiB), so it masks the pointer to each candidate power-of-2
alignment and reads the header there. The header's `size_tag` carries a 24-bit
`PAGE_MAGIC` plus `log2(page_size)`; the true base is the alignment whose tag
validates. The magic is load-bearing: a *smaller* sub-alignment of a large page
masks to a base **mid-page**, on object/inline data, and a bare `log2` byte there
can coincidentally equal the smaller size's log2 — read as a false header
yielding a garbage `(region_id, stamp)`. The magic makes that ~`1/2^32` instead
of ~`1/256`.

`region_of_ptr` is **authoritative** beyond the magic: it accepts a candidate
base only when the region it names is live in *this* store and genuinely *owns*
the pointer. A mid-page coincidence names a region that does not own the pointer,
so the walk passes it over and resolves the true base — even for a pointer deep
inside a large page. (The free-time cross-ref scan reads headers without a store
in hand, so it relies on the magic plus its `valid_region` filter.)

`ensure_raw` then carries an **always-on backstop** (the generation and ownership
checks above are debug-only / store-bound; a release path could still, in
principle, hand it a garbage id from a stale or foreign read): an id past
`MAX_PLAUSIBLE_REGION_ID` is not a region to lazily create, because its lazy
`regions.resize_with(id + 1, …)` would grow the table to that id — hundreds of GB
— and abort on allocation failure far from the deref. It panics there, naming the
hazard, in every build. The region table is bounded by the max *concurrently-live*
regions — every id reaches `free_physical` again, freed and never-materialized
alike ([model.md](model.md) § "Physical id recycling") — so a real id never
approaches the bound. It is a backstop, not a detector: it makes the failure
*loud* instead of an opaque OOM — the deref site and the freeing region still
come from the generation check (debug) and `--trace=guardfree` / `--trace=free`.

The bound has a ceiling of its own. The panic can only fire if the table at that
id is **allocatable**: the check runs before `resize_with`, so a bound whose
table exceeds what the machine can supply lets the allocator abort one id below
it instead, and the program dies with a byte count and no diagnosis.

Those two requirements pull in opposite directions, and what leaves room between
them is a ratio rather than any absolute size. A live region owns at least one
OS page (Rule 6), which is 4 KiB at the smallest, so a program holding N regions
at once already holds at least 4N KiB of pages, while the table for those ids
costs N × `size_of::<Option<RegionEntry>>()` — about a twentieth as much. Set
`MAX_PLAUSIBLE_REGION_ID` past the live-region count a machine's memory permits,
and the table at that bound stays within what the same machine can allocate. At
`1 << 28` that is 1 TiB of pages against a ~56 GB table.

Do not read the bound as "no program can want this much memory". A machine large
enough to host such a program is the same machine that can allocate the table, so
the bound tracks the hardware; picking a smaller constant to make the table
cheaper only moves the tripwire into the range where real programs live. On a
machine too small for the table the allocator still aborts first — which is why
the assertion message names id exhaustion beside corruption rather than
asserting the second.

The free-time cascade scan (`find_object_cross_refs`) deliberately does NOT
check generations: teardown legitimately scans objects whose contained
values may already be dead (the `valid_region` filter handles them);
panicking there would turn tolerated teardown ordering into false positives.

The store id scopes the check to pages the checking store actually stamped.
Generations from two different stores are unrelated numbers: a worker
thread's `region_of` on a value allocated by its parent's heap (the
spawn-closure path does this) would otherwise compare the parent's
stamp against the worker's counter and false-positive. A store-id mismatch
is definitive — pages never migrate between stores (each store owns its page
pool) — so the check skips them, preserving `region_of`'s
(unsound but tolerated) cross-thread behavior of attributing
the foreign page's region id to the local store.

## Uncounted-borrow check

Some references convey no reference count. A child fiber inherits its parent's
dynamic-parameter bindings as a baseline frame (`prim_fiber_new`,
`seed_child_inheritance`); each heap value in that frame — a scheduler reached
through a parameter, say — takes one seeding retain and a recorded
`fiber → value` content edge, released by the fiber object's own free
([owner.md](owner.md) § "A child's inherited parameter baseline is a counted
holder"). The check below is the oracle that the count holds: the borrowed
region must outlive the borrowing fiber, and generations make that checked
rather than assumed — a missing or displaced retain panics at the resume
boundary instead of surfacing as a stale read far from the seam.

When the baseline is seeded, each heap binding's `(parameter, region,
generation)` is recorded on the fiber (`param_borrows`, debug builds only). At
the borrow's use sites — every fiber resume (all recorded borrows) and
`resolve_parameter` when it resolves a baseline binding — the region's current
generation is compared against the recorded one. A mismatch means the region's
pages were freed since the borrow was taken: the borrow dangles, and the check
panics deterministically, naming the parameter, at the borrow site rather than
at a later stale read.

This closes the re-claimed-page window the page-stamp check cannot see.
`region_of_ptr` reads the borrowed value's page stamp, so once a freed page is
re-claimed and re-stamped at the current generation it passes; the recorded
generation is held apart from the page, so it still detects the staleness — and
it reads only the counter, never dereferencing the possibly-stale value. The
region and its generation are read from the one explicit heap, so the comparison
is within a single store. The check is debug-only — release builds record
nothing and compile the comparisons out. The pinning tests are in
`src/vm/fiber/borrow_tests.rs`.

### Two borrow shapes: recorded handle vs `region_of`-sited

The recorded handle pays off for a borrow that sits **idle in a persistent runtime
home** across a free, where the page can be reclaimed and re-stamped before the next
deref. Two borrows have that shape, and both carry the handle: the cross-fiber **param
snapshot** above, and the **suspended-frame** `activation_region_map` — the
static-slot→physical-region remap a parked `BytecodeFrame` holds across park/resume
(`src/value/fiber.rs`). The regions worth snapshotting are the suspended activation's
own **live** allocations, kept alive by its still-pending `DecrefRegion`s;
`BytecodeFrame::suspend` snapshots each such `(slot, region, generation)` into the
frame's `region_borrows` (`record_region_borrows`), and `resume_suspended` re-checks
them with the shared `first_stale_borrow` just before `restore_activation_region_map`
re-enters the body — so a region freed while the fiber was parked panics at the resume
boundary, naming the slot, instead of corrupting the resumed activation's allocs/
decrefs. Pinned by `suspended_frame_region_borrow_detects_freed_region`
(`src/vm/fiber/borrow_tests.rs`).

The map is not automatically dangling-free, which is what forces the snapshot to record
the **establish-generation** (`MappedRegion::gen`, the region's generation when the slot
was inserted) rather than the region's current generation. The map records
`slot → region` for every ALLOC-slot allocation and is cleared only by the matching
slot-based `DecrefRegion`. A region freed any other way — a value-based
`DecrefValueRegion`/`DecrefCellRegion` (capture cells), a cross-region cascade, a
subtree drop — leaves its entry behind, and the physical id it named is recycled to an
unrelated region. Stamping such a **dead leftover** with the id's *current* generation
would forge a live borrow of an incarnation the activation never owned, and the resume
check would then trip when that unrelated incarnation is freed — a stale-suspended-frame
false positive with no real UAF behind it (in release the guard is compiled out and the
leftover's dead `DecrefRegion` never reads it, so the program runs correctly). Recording
the establish-generation makes the two cases separable: `record_region_borrows` skips an
entry whose `gen` no longer matches the region's current generation (a dead leftover),
while an entry that still matches is a genuine live borrow whose free *while parked* still
trips the check. Pinned by `stale_leftover_map_entry_is_not_snapshotted_as_a_borrow`
(`src/vm/fiber/borrow_tests.rs`) and, at corpus scale, by
`signals_no_stale_suspended_frame_region_borrow` (`tests/integration/elle_scripts.rs`).

A **pass-through borrow** is the other shape and needs
no handle. The `%first`/`%rest`/`%get` intrinsics (`LirInstr::First`/`Rest`/`Get`)
hand back a value that aliases into the source collection's region with no incref —
an uncounted borrow — but it is a transient SSA value with a compile-time-bounded
lifetime and no persistent home to record a handle on. Its derefs route through
`region_of` like any other value, so the page-stamp check above already detonates it
the moment its source region is freed while the borrow is still held
(`--trace=guardfree` covers the reclaimed-page window). The forest-era refinement —
assert the borrowed value's owning chain to the root is alive — also lands at that
same `region_of`, never in a recorded handle.

A *native* `first`/`rest`/`get` is a different case again: its result is **counted**
by the pass-through retain in `dispatch_native_call`
(`pass_through_retain` → `EscapeSite::NativeCallResult`, `src/value/arena.rs`), so it
is no borrow at all and needs no check. Only the intrinsic form is uncounted. Pinned
by `pass_through_borrow_detonates_at_region_of` (`src/value/fiberheap/tests.rs`).

Vocabulary: these are **generations**, never "epochs" — the word *epoch*
belongs to the language migration system (docs/epochs.md).
