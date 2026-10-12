# Region generations: stale derefs detonate in debug builds

<!-- audited: 2026-10-12 -->

The per-region generation counter and page stamps that turn a stale region deref
into a debug-build panic at the deref site.

The `--trace=guardfree` oracle in [diagnostics.md](diagnostics.md) covers the
window this check cannot see.

Physical region ids are recycled, because a freed id returns to the mint pool,
and freed pages are cached for reuse. A stale reference, a `Value` or region id
that outlived its region, therefore reads *plausible* memory. The plain VM reads
stale but intact contents, and the defect surfaces far from the deref, at a time
that varies, if at all. Generations make the common case a deterministic panic
at the exact deref:

- `RegionStore` keeps a **generation counter per physical region id**. The
  counter starts at 0, and the store bumps it on every path that returns the
  id's pages: the RC-zero free and wholesale teardown alike. A recycled id
  mints its next region at the bumped generation.
- Every page a region claims is **stamped** `(region_id, generation,
  store id)` in its page header at claim time, in all builds. The stamp is
  eight bytes per page, and release builds pay nothing else. The store id names
  the `RegionStore` that claimed the page; each store draws a process-unique id
  at construction.
- `region_of` is the single funnel through which every runtime RC decision
  reads a value's region, and under `debug_assertions` it **checks** the stamp.
  A page header whose generation differs from the store's current generation
  for that region id marks a stale deref. `region_of` panics right there,
  naming both generations. The declaration oracle calls `region_of` on every
  native-call result, so every native call is a checkpoint for free.

What it catches deterministically: any deref of a value whose region was freed
while the page sits unclaimed in the page cache. Without the generation check,
that deref is a silent stale read on the plain VM, and a fault under
`--trace=guardfree` whose timing varies.

What it cannot catch: a deref of a page already **re-claimed** by a new region.
The claim restamps the header, so the stale pointer resolves, wrongly but
self-consistently, to the new region. That window is `--trace=guardfree`'s
domain, because guardfree never re-claims a freed page. So the two instruments
compose: generations make the cached-page window loud in every debug run, and
guardfree makes the re-claimed window loud in dedicated runs.

## Finding the page base soundly (not a generation matter, but it shares the funnel)

`region_of` resolves a pointer to its region through
`RegionStore::region_of_ptr`. A pointer into a hydrated image resolves through
the store's interval table, with a range check and no header read
([image.md](../image.md)). Every other pointer needs its **page base** first.
Pages are variable-sized (geometric growth to 4 MiB), so the walk masks the
pointer to each candidate power-of-2 alignment and reads the header there.
The header's `size_tag` carries a 24-bit `PAGE_MAGIC` plus `log2(page_size)`,
and the true base is the alignment whose tag validates.

Without the magic the search is unsound. A *smaller* sub-alignment of a large
page masks to a base **mid-page**, on object or inline data. A bare `log2` byte
there can equal the smaller size's log2 by coincidence, and the walk then reads
a false header that yields a garbage `(region_id, stamp)`. The magic makes that
~`1/2^32` instead of ~`1/256`.

`region_of_ptr` prefers ownership over the magic. A candidate base whose header
names a live region of *this* store that genuinely *owns* the pointer is the
authoritative answer, and the walk stops there. A header that validates but
owns nothing also stops the walk. It is a real base of a foreign store or of a
region since freed, and masking further would mask *below* this page, into
memory nothing mapped. So its id is the fallback answer.

The allocation funnel and the free-time cross-ref scan resolve through
`owning_region` instead, which
reads the same table and takes the same walk with no generation check. It
answers no region where no live region of this store owns the pointer.

### Every byte the magic screens has to be written

The magic bounds a *coincidence*, so it holds only over bytes something chose.
A claimed page's body is whatever its last occupant left ([model.md](model.md)
§ "Page recycling"). The walk reads the word at offset 12 of every
`base_page`-aligned address inside a large page. A 16-byte record that lands on
one of those addresses, and leaves its last four bytes as padding, does not
overwrite what was there: it publishes it. What was there can be a real
`size_tag`, and then the walk stops at a forged base mid-page and the odds the
magic bought never apply.

`RegionSlice` is that record: a pointer and a `u32` length, with the four bytes
a `size_tag` occupies left over. It carries them as a field written to zero. So
writing a slice header clears those four bytes instead of leaving them, and no
live page holds a forged base at a slice header's address. A `size_tag` always
carries the magic in its high 24 bits, so a zero word never reads as one. The
same rule binds the next 16-byte record a region writes inline.

The free-time cross-ref scan is where a forged base costs the most and says the
least. It resolves a pointer through this same walk and then filters the answer
by ownership. So the scan drops a forged id, and the real cross-region edge
behind it goes with the id, while the table recorded at allocation still
carries the edge. In a debug build the edge-table oracle reports the
disagreement at the free ([ownership.md](ownership.md) § "The outgoing edge
table"). In a release build the edge is never released, and the target region
is held to teardown.

`ensure_raw` carries an **always-on backstop**. The generation and ownership
checks above are debug-only or bound to a store, so a release path could still
hand `ensure_raw` a garbage id from a stale or foreign read. An id past
`MAX_PLAUSIBLE_REGION_ID` is not a region to create lazily: the lazy
`regions.resize_with(id + 1, …)` would grow the table to that id, hundreds of
GB, and abort on allocation failure far from the deref. So `ensure_raw` panics
there, naming the hazard, in every build. The region table is bounded by the
most regions live at once, because every id reaches `free_physical` again,
freed and never-materialized alike ([model.md](model.md) § "Physical id
recycling"). A real id therefore never approaches the bound.

The backstop is not a detector. It makes the failure *loud* instead of an
opaque OOM, and the deref site and the freeing region still come from the
generation check (debug) and from `--trace=guardfree` and `--trace=free`.

The bound has a ceiling of its own. The panic can only fire if the table at
that id is **allocatable**. The check runs before `resize_with`, so under a
bound whose table exceeds what the machine can supply, the allocator aborts one
id below the bound, and the program dies with a byte count and no diagnosis.

Those two requirements pull in opposite directions, and a ratio, not an
absolute size, leaves room between them. A live region owns at least one OS
page (Rule 6), which is 4 KiB at the smallest. So a program holding N regions
at once already holds at least 4N KiB of pages. The table for those ids costs
N × `size_of::<Option<RegionEntry>>()`, about a twentieth as much. Set
`MAX_PLAUSIBLE_REGION_ID` past the live-region count a machine's memory
permits, and the table at that bound stays within what the same machine can
allocate. At `1 << 28` that is 1 TiB of pages against a ~56 GB table.

Do not read the bound as "no program can want this much memory". A machine
large enough to host such a program is the same machine that can allocate the
table, so the bound tracks the hardware. A smaller constant makes the table
cheaper only by moving the tripwire into the range where real programs live. On
a machine too small for the table, the allocator still aborts first. That is
why the assertion message names id exhaustion beside corruption rather than
asserting the second.

The free-time cascade scan (`find_object_cross_refs`) deliberately does NOT
check generations. Teardown legitimately scans objects whose contained values
may already be dead, and the ownership check in `owning_region` drops them. A
panic there would turn tolerated teardown ordering into false positives.

The store id scopes the check to pages the checking store actually stamped.
Generations from two different stores are unrelated numbers. A worker thread's
`region_of` on a value its parent's heap allocated, which the spawn-closure
path does, would otherwise compare the parent's stamp against the worker's
counter and report a false positive. A store-id mismatch is definitive, because
pages never migrate between stores (each store owns its page pool). So the
check skips such pages, and keeps `region_of`'s cross-thread behavior, unsound
but tolerated, of attributing the foreign page's region id to the local store.

## Uncounted-borrow check

Some references convey no reference count. A child fiber inherits its parent's
dynamic-parameter bindings as a baseline frame (`prim_fiber_new`,
`seed_child_inheritance`). Each heap value in that frame, for example a
scheduler reached through a parameter, takes one seeding retain and a recorded
`fiber → value` content edge. The fiber object's own free releases both
([park.md](park.md) § "A child's inherited parameter baseline is a counted
holder"). The check below is the oracle that the count holds: the borrowed
region must outlive the borrowing fiber. Generations make that checked rather
than assumed, so a missing or displaced retain panics at the resume boundary
instead of surfacing as a stale read far from the seam.

When the baseline is seeded, the fiber records each heap binding's
`(parameter, region, generation)` (`param_borrows`, debug builds only). The
borrow's use sites compare the region's current generation against the
recorded one. Those sites are every fiber resume, which checks all recorded
borrows, and `resolve_parameter` when it resolves a baseline binding. A
mismatch means the region's pages were freed since the borrow was taken, so
the borrow dangles. The check then panics deterministically, naming the
parameter, at the borrow site rather than at a later stale read.

This closes the re-claimed-page window the page-stamp check cannot see.
`region_of_ptr` reads the borrowed value's page stamp, so a freed page that is
re-claimed and re-stamped at the current generation passes it. The recorded
generation is held apart from the page, so it still detects the staleness, and
it reads only the counter, never dereferencing the possibly-stale value. The
region and its generation are read from the one explicit heap, so the
comparison is within a single store. The check is debug-only: release builds
record nothing and compile the comparisons out.

The pinning tests are in
[borrow_tests.rs](../../../src/vm/fiber/borrow_tests.rs).

### Two borrow shapes: recorded handle vs `region_of`-sited

The recorded handle pays off for a borrow that sits **idle in a persistent
runtime home** across a free, where the page can be reclaimed and re-stamped
before the next deref. Two borrows have that shape, and both carry the handle.
One is the cross-fiber **param snapshot** above. The other is the
**suspended-frame** `activation_region_map`: the static-slot→physical-region
remap a parked `BytecodeFrame` holds across park and resume
([frame.rs](../../../src/value/fiber/frame.rs)).

The regions worth snapshotting are the suspended activation's own **live**
allocations, kept alive by its still-pending `DecrefRegion`s.
`BytecodeFrame::suspend` snapshots each such `(slot, region, generation)` into
the frame's `region_borrows` (`record_region_borrows`). `resume_suspended`
re-checks them with the shared `first_stale_borrow`, just before
`restore_activation_region_map` re-enters the body. So a region freed while the
fiber was parked panics at the resume boundary instead of corrupting the
resumed activation's allocations and releases.
`suspended_frame_region_borrow_detects_freed_region`
([borrow_tests.rs](../../../src/vm/fiber/borrow_tests.rs)) pins it.

The panic names the parked activation beside the slot and the physical region:
the function, its position in the replay chain, and the source location of the
resume point. A slot number and a physical region id are per-run values that
name no code. So a panic carrying only those says nothing about which program
parked. Where the resume point has no location entry of its own, the panic
falls back to the function's first recorded line, because the file is most of
the answer. `ParkSite` builds the text
([frame.rs](../../../src/value/fiber/frame.rs)), and
`stale_borrow_message_names_the_parked_site`
([borrow_tests.rs](../../../src/vm/fiber/borrow_tests.rs)) pins it.

The map can hold dangling entries. That is what forces the snapshot to record
the **establish-generation** (`MappedRegion::gen`, the region's generation when
the slot was inserted) rather than the region's current generation. The map
records `slot → region` for every ALLOC-slot allocation, and only the matching
slot-based `DecrefRegion` clears an entry. A region freed any other way leaves
its entry behind: a value-based `DecrefValueRegion` or `DecrefCellRegion`
(capture cells), a cross-region cascade, or a subtree drop. The physical id the
entry named is then recycled to an unrelated region.

Stamping such a **dead leftover** with the id's *current* generation would
forge a live borrow of an incarnation the activation never owned. The resume
check would then trip when that unrelated incarnation is freed: a
stale-suspended-frame false positive with no real UAF behind it. In release the
guard is compiled out, the leftover's dead `DecrefRegion` never reads the
entry, and the program runs correctly. The establish-generation separates the
two cases. `record_region_borrows` skips an entry whose `gen` no longer matches
the region's current generation, a dead leftover. An entry that still matches
is a genuine live borrow, and its free *while parked* still trips the check.

`stale_leftover_map_entry_is_not_snapshotted_as_a_borrow`
([borrow_tests.rs](../../../src/vm/fiber/borrow_tests.rs)) pins it.
[signals.lisp](../../../tests/lang/signals.lisp) pins it at corpus scale: its
squelch, silence and yield churn recycles ids fast enough to leave such an
entry. The check is debug-only, so the CI jobs that compile debug assertions in
arm it ([ci](../../analysis/ci.md)).

The generation separates the two cases only once the region has been freed. A
leftover whose region is still live reads exactly like a borrow, because the
free that would move the generation has not happened yet. The check then
reports the free *after* the park. So the snapshot asks the function as well as
the heap: a slot the function never releases by id holds no pending
`DecrefRegion`, whatever its generation says. `Code::frame_release_regions` is
that list, the static slots this function's slot-routed releases name, and
`record_region_borrows` records an entry only when the slot appears in it.

The entries this excludes belong to a region whose release is VALUE-routed,
which frees the region without clearing the slot. The activation reads that map
entry through no release at all, so a free while parked corrupts nothing
through it. `region_of`'s page stamp covers what the activation still holds in
its stack or its environment, where the activation reads it.
`a_slot_with_no_slot_routed_release_is_not_a_borrow`
([borrow_tests.rs](../../../src/vm/fiber/borrow_tests.rs)) pins it.

A **pass-through borrow** is the other shape and needs no handle. The `%first`,
`%rest` and `%get` intrinsics (`InstrRef::First`, `Rest`, `Get`) hand back a
value that aliases into the source collection's region with no incref, an
uncounted borrow. But it is a transient SSA value with a compile-time-bounded
lifetime and no persistent home to record a handle on. Its derefs route through
`region_of` like any other value, so the page-stamp check above detonates it
the moment its source region is freed while the borrow is still held.
`--trace=guardfree` covers the reclaimed-page window. A check that the borrowed
value's owning chain to the root is alive belongs at that same `region_of`,
never in a recorded handle.

A *native* `first`, `rest` or `get` is a different case again. The
pass-through retain in `dispatch_native_call` counts its result
(`pass_through_retain` → `EscapeSite::NativeCallResult`,
[arena.rs](../../../src/value/arena.rs)), so it is no borrow at all and needs no
check. Only the intrinsic form is uncounted.
`pass_through_borrow_detonates_at_region_of`
([tests.rs](../../../src/value/fiberheap/tests.rs)) pins it.

Vocabulary: these are **generations**, never "epochs". The word *epoch* belongs
to the language migration system ([epochs](../../epochs.md)).
