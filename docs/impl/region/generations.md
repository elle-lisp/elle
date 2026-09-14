# Region generations: stale derefs detonate in debug builds

<!-- audited: 2026-09-14 -->

The per-region generation counter and page stamps that turn a stale region deref
into a debug-build panic at the deref site. Pairs with the `--trace=guardfree` oracle described in
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
validates. Without the magic the search is unsound: a *smaller* sub-alignment of a large page
masks to a base **mid-page**, on object/inline data, and a bare `log2` byte there
can coincidentally equal the smaller size's log2 — read as a false header
yielding a garbage `(region_id, stamp)`. The magic makes that ~`1/2^32` instead
of ~`1/256`.

`region_of_ptr` prefers ownership over the magic: a candidate base whose header
names a live region of *this* store that genuinely *owns* the pointer is the
authoritative answer, and the walk stops there. A header that validates but owns
nothing is still where the walk stops — it is a real base of a foreign store or
of a region since freed, and masking further would mask *below* this page, into
memory nothing mapped — so its id is the fallback answer. The free-time cross-ref
scan reads headers without a store in hand, so it has only the magic and its
`valid_region` filter.

### Every byte the magic screens has to be written

The magic bounds a *coincidence*, so it holds only over bytes something chose.
A claimed page's body is whatever its last occupant left ([model.md](model.md)
§ "Page recycling"), and the walk reads the word at offset 12 of every
`base_page`-aligned address inside a large page. A 16-byte record that lands on
one of those addresses and leaves its last four bytes as padding does not
overwrite what was there — it publishes it. What was there can be a real
`size_tag`, in which case the walk stops at a forged base mid-page and the odds
the magic bought never apply.

`RegionSlice` is that record: a pointer and a `u32` length, with the four bytes
a `size_tag` occupies left over. It carries them as a field written to zero, so
writing a slice header clears those four bytes instead of leaving them, and no
live page holds a forged base at a slice header's address. A `size_tag` always
carries the magic in its high 24 bits, so a zero word never reads as one. The
same rule binds the next 16-byte record a region writes inline.

The free-time cross-ref scan is where a forged base costs the most and says the
least. It resolves a pointer through this same walk and then filters the answer
by ownership, so a forged id is dropped — and the real cross-region edge behind
it is dropped with it, while the table recorded at allocation still carries it. In
a debug build the edge-table oracle reports the disagreement at the free
([ownership.md](ownership.md) § "The outgoing edge table"); in a release build
the edge is simply never released, and the target region is held to teardown.

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
boundary instead of corrupting the resumed activation's allocs/decrefs. Pinned by
`suspended_frame_region_borrow_detects_freed_region` (`src/vm/fiber/borrow_tests.rs`).

The panic names the parked activation beside the slot and the physical region: the
function, its position in the replay chain, and the source location of the resume
point. A slot number and a physical region id are per-run values that name no code, so
a panic carrying only those says nothing about which program parked. The location
falls back to the function's first recorded line where the resume point has no entry of
its own, because naming the file is most of the answer. `ParkSite` builds the text
(`src/value/fiber/frame.rs`), pinned by `stale_borrow_message_names_the_parked_site`.

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

The generation separates the two cases only once the region has been freed. A leftover
whose region is still live reads exactly like a borrow, because the free that would move
the generation has not happened yet — and it is the free *after* the park that the check
then reports. So the snapshot asks the function as well as the heap: a slot the function
never releases by id holds no pending `DecrefRegion`, whatever its generation says.
`Code::frame_release_regions` is that list — the static slots this function's slot-routed
releases name — and `record_region_borrows` records an entry only when the slot appears in
it. The entries this excludes belong to a region whose release is VALUE-routed, which
frees the region without clearing the slot; the activation reads that map entry through no
release at all, so a free while parked corrupts nothing through it. What the activation
still holds in its stack or its environment is covered where it is read, by `region_of`'s
page stamp. Pinned by `a_slot_with_no_slot_routed_release_is_not_a_borrow`
(`src/vm/fiber/borrow_tests.rs`).

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
