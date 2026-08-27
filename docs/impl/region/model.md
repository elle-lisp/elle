# Region representation — id-spaces, per-execution model, layout

Implementation-facing. How the compiler and runtime represent regions: the two
id-spaces, the per-activation physical-region model, the page layout, and how an
object's inline payload shares its region. The correctness obligations these
serve are in [rules.md](rules.md); the consumer model is in
[docs/regions.md](../../regions.md).

## Two id-spaces: static and runtime

A region id means one of two different things depending on where it came from,
and conflating them is a class of UAF. Name them:

- **Static region ids** (`new_static_region`, `lir/lower`): compile-time *slot*
  numbers baked into bytecode. A static id is a per-function slot, **not** a live
  region. It is never used directly as a physical region.
- **Runtime physical ids** (`new_runtime_region`, per-heap `RegionStore`): the actual
  pages-owning regions, minted per allocation *execution* and recycled on free.

These two id-spaces are **types**, not conventions. A runtime physical id is a
`RuntimeRegion(NonZeroU32)`; a compile-time slot is a `StaticRegion(NonZeroU32)`.
`NonZeroU32` on both makes region/slot 0 *unrepresentable* (Rule 1 by
construction, not by a runtime `!= 0` assert), and being distinct newtypes means
a static slot cannot be passed where a runtime region is expected — "never index
a static id into `RegionStore`" is a compile error. "No region active" / "this
value has no region" is `Option<RuntimeRegion>` (`None`), never a sentinel `0`.

Crucially, a region is **not** a uniform optional field hung on every
instruction (which would let an allocation exist with no region — the invalid
state spelled `None`). It is a mandatory `region: StaticRegion` field on exactly
the LIR instruction *variants* that allocate or route a per-call region, and
absent from the structure of those that don't. "Region not applicable here" is
encoded by the field's absence; an allocation with no region is unconstructable.
Serialized into bytecode a `StaticRegion` becomes a raw `u32`; the VM decodes
that `u32` slot and resolves it to a `RuntimeRegion` — the two never meet as one
type.

Both index the single `RegionStore`, so two soundness guards keep them from
colliding:

- a static slot is resolved to a physical id through the current activation's
  `activation_region_map` (`runtime_region_for_alloc_slot`/`new_runtime_region_for_call_slot`/`take_runtime_region_for_drop_slot`), never indexed into
  the store as if it were physical;
- `new_runtime_region` never reissues an id that currently names a live region.

Drop either guard and two logical regions land on one physical id — a torn read
when one is freed under the other.

## The per-execution region model

A static region id is a per-function slot; every activation mints its own
physical region for it. `runtime_region_for_alloc_slot` records the slot→physical mapping
in the activation's frame so the matching `DecrefRegion` frees the same physical
region; `take_runtime_region_for_drop_slot` clears the slot so the next loop iteration mints
fresh. This is what makes deep recursion and loops run in bounded memory: one
static id names a per-function slot, never a single live physical region shared
across activations.

The model carries an emission-side obligation — **one allocation execution per
slot between drops**. `runtime_region_for_alloc_slot` mints fresh on every
execution and *overwrites* the frame's mapping, so if the lowerer emits N
allocation instructions against one slot with only the final `DecrefRegion`,
the first N−1 physical regions are orphaned the moment their mapping is
overwritten — an unreleasable initial reference each, i.e. a structural leak
(Rule 4's dual: every allocation *execution* needs its own demise, so every
allocation *instruction* needs its own slot unless a drop provably intervenes,
as the loop back-edge drop does). The file-letrec capture-cell pre-pass is the
case in point: `lower_begin` emits one `MakeCaptureCell` per captured top-level
binding, and routing them all through the Begin's single region slot would
orphan every cell but the last (at stdlib scale, thousands of cells plus
everything they pin). Pre-allocated capture cells therefore get **one region
per cell** (`begin_cell_regions`), each released by its own `DecrefRegion` at
its binding's last use.

## Constants lower as ordinary allocations, not promoted values

A heap literal — a string, an array/quoted form, a closure template — is **not**
a pre-allocated `Value` baked into the code object. The constant pool holds the
literal's immutable *template* (the bytes, the structure, the closure template)
as plain compile-time data. A `MaterializeConst` instruction builds a *fresh*
value from that template each time it executes, into the literal's own
solver-assigned region (`alloc_here(hir.id)`, the one-region-per-value baseline),
resolved per activation to a fresh physical region and allocated with
`arena::alloc_in_region(obj, region)` — exactly as `MakeArrayMut`/`List` do.

So a literal is born in the right region (Rule 3), dies at its `decref_point`
(Rule 4), and lives past that point only by ordinary RC if it escapes (Rule 5).
Re-materializing per execution is the correct-and-slow baseline: runtime
`(eval …)` and module load re-run the compiler, so the same source materializes
fresh copies each time, each reclaimed when it falls out of use. The rejected
alternative — a "constant-pool region" whose lifetime is the code object — would
promote a value into a longer-lived region (Rule 3 forbids promotion), need a
second demise mechanism outside `DecrefRegion`, and share one region across every
activation of the code object; do not adopt it.

Closure **templates** are no exception: the template is itself a
region-allocated heap object (its bytecode an inline `RegionSlice<u8>`),
materialized at its definition site; a closure **instance** holds a normal
cross-region reference to it, increfed when the instance is built and
cascade-released when its region frees. Region RC is the single reclamation
mechanism for code objects too.

## Physical representation

A per-thread page pool with size classes hands pages to regions on demand and
reclaims them at RC 0; it can munmap excess pages under pressure. Regions never
share pages (Rule 6). RC lives in `RegionStore`, one counter per physical region.
One region per value, unmerged, is the baseline: it claims a page per allocation
— correct but expensive. Two kinds of *merging* amortize that cost, both
collapsing several solver `Region`s onto one physical region (the
consumer-facing performance account is in
[regions/performance.md](../../regions/performance.md)):

- the **builder-idiom seed** below — merge a freshly-built child aggregate into
  the parent aggregate it is stored into (the `%pair` car/cdr store). This is
  implemented as the analysis and runtime mint-or-reuse described in [merging.md](merging.md);
- **sibling page-amortization** — collapse sibling regions with coincident
  lifetimes and no edge between them. A later rider, not yet implemented.

## Page recycling: a claim is a free-list pop

**A page moves between a region and the pool untouched, in both directions.**
`release` pushes it onto a size-class free list; `claim` pops it and hands it
straight back. Neither reads nor writes a byte of it, and neither makes a
system call. The hot path of a small short-lived region — mint, claim, write
one object, free — therefore costs the write and nothing else, which matters
because that is the *common* path: one region per value is the baseline above,
so a program allocates regions at the rate it allocates values.

Nothing needs the page prepared. `RegionPage::new` stamps the header, sets the
object cursor to `HEADER_SIZE` and the data cursor to the page top; every
object slot is written before it is read, and every inline-data slice is fully
copied before its `RegionSlice` is handed out. **A claimed page's body is
therefore unspecified, not blank** — it holds whatever the previous occupant
left, and no reader is entitled to look.

Two consequences worth stating, because both are easy to get wrong:

- **Do not discard the page's frames at claim.** `madvise(MADV_DONTNEED)` on a
  page about to be written hands memory back that the very next store faults
  straight in — a system call and a fault per claim, for no resident-memory
  reduction. Cached bytes are bounded by the pool's `max_cached`, and a page
  past that bound is `munmap`ed on release; that is where memory genuinely
  returns to the OS.
- **A page in the free list keeps its header.** A cached page still carries the
  `(region_id, generation, store)` stamp of the region that died on it, which
  is exactly what a pointer outliving that region finds: the ids match, the
  generations do not, and the debug-build check panics at the deref site
  ([generations.md](generations.md)). Blanking offset 0 would take that
  detector away and leave the stale pointer with no self-validating header at
  its own page size, so `region_of_ptr`'s page-base walk would mask past this
  page into memory the store does not own.

### `--trace=scrub`: make a stale read wrong on purpose

The one thing that writes a released page. Under `--trace=scrub`, `release`
zeroes the spans the dying region wrote — the object slots
`[HEADER_SIZE, obj_cursor)` and the inline-data suffix `[data_cursor, len)`,
together one `PageDirty` pair, sparing the header for the reason above. The gap
between the two cursors was never written by that region, so it is not scrubbed
either; a region holding one 48-byte cons costs 48 bytes of work.

The point is not hygiene. A read through a pointer that outlived its region
normally finds the dead region's bytes — plausible, well-typed, and silently
wrong. Scrubbed, it finds an all-zero `HeapObject` slot, whose tag matches no
live value, so `arena::deref` panics naming the deref site. It is the cheap
member of the family: `--trace=guardfree` never reuses a page and so catches a
stale read at any distance, at a mapping per freed page; the generation check
catches a stale *region resolution*, but only in debug builds and only while
the page is unclaimed; scrub catches a stale *content* read, in release builds
too, for one `memset` per freed page. A page on its way to `munmap` is never
scrubbed — an unmapped address faults on its own.

`tests/elle/region-page-recycle.lisp` measures what the claim path costs per
call from Elle, through the `arena/page-claims` gauge; `pagepool::tests` pins
the untouched-recycle contract and the scrub's spans.

## Physical id recycling: reserved, live, free

A physical region id has three states, and every id must reach `free` again.

- **Reserved.** `new_runtime_region` takes an id off `free_physical`, or bumps
  `next_physical` when that list is empty. The id names no region yet: it has no
  entry and no pages. Its generation slot, if it has one, still holds the value
  its previous incarnation's teardown left.
- **Live.** `ensure_raw` builds the id's `RegionEntry` on first touch. It also
  sizes `regions` and `generations` to the id, so **the table is as long as the
  largest id ever made live**, whatever the count of live regions is. (Static
  slot ids reach `ensure_raw` too and size the table the same way; they come
  from the compiler's own bounded counter — see § "Two id-spaces".)
- **Free.** A teardown returns the id's pages, bumps its generation, and pushes
  the id onto `free_physical`, where the next mint finds it.

The reserved state has a second exit: a caller can mint an id and never allocate
into it. The **per-call result region** is that case, on the hottest path in the
runtime. `dispatch_native_call` and `dispatch_collection_call` each mint one
region per call, before the call runs, because the callee may allocate its
result into it. A primitive that returns an
immediate (`(< a b)`), or one that returns a value borrowed from an argument
(`first`, `rest`, `get`), allocates nothing into it. That id never reaches
`ensure_raw`, so no teardown can ever return it.

An id stranded that way costs no heap object, no page, and no reference count,
which is why the object and region gauges cannot see it. It costs the region
**table**: it raises the largest id a later mint hands out, and `regions` is a
`Vec<Option<RegionEntry>>` indexed by id, so a stranded id is one
`size_of::<Option<RegionEntry>>()` slot of resident memory that nothing frees.
Resident memory then grows with total work while `arena/count`,
`arena/region-count`, and `arena/bytes` all stay flat.

So both dispatchers close the reserved state themselves: after the call,
`recycle_unmaterialized` pushes the result region back onto `free_physical` when
the call left it unmaterialized. Unmaterialized means two things together —
`regions[id]` is empty **and** the id's generation still equals the generation
read at the mint.

The generation half is what makes the test exact, and it is not optional. A
region that materialized and was freed inside the call (a native that re-enters
the VM) also leaves `regions[id]` empty — but its teardown already pushed that
id, so pushing it again would put a **duplicate** in `free_physical`. Two mints
could then take the same id before either materialized, and `new_runtime_region`
could not tell them apart: its skip loop only rejects an id that is already
*live*. Two logical regions on one physical id is the aliasing UAF the mint loop
exists to prevent. A teardown bumps the generation, so the generation check
rejects exactly that id and admits only a mint that nothing has touched since.

`arena/region-ids` reads `next_physical` from Elle — the gauge that moves the
moment an id fails to come back — and `arena/region-table` reads what the table
costs. The bound is pinned by the `id-*` probes of `tests/elle/oracle.lisp`,
which measure id issuance per call against a live-growth discriminator of their
own: a loop of calls that allocate nothing issues no new id, and a materializing
call's id comes back by its teardown. `regionstore::tests::recycle` pins the
store-level contract, the duplicate the generation check refuses included.

## RegionSlice contents share their object's region

Non-obvious and load-bearing: immutable aggregates (string, array, struct, and a
**closure's captured env**) store their variable-length payload as an
`RegionSlice` laid out *in the same region pages* as the HeapObject header. Such
contents therefore have **no** region of their own and **no** cross-region RC
edge — their lifetime *is* the containing object's region's lifetime. Freeing the
object's region frees its inline payload with it.

The consequence to keep in mind: a closure's captured environment dies with the
closure's region. A prematurely-freed closure region surfaces as a *torn
captured-env read* in `populate_env`, not as an RC underflow — there was never a
separate region to underflow. When `(squelch f …)` shares `f`'s env, the new
closure's env is copied inline into the new closure's region, so that region now
owns the captures.

The corollary for **metadata-only clones**: an operation that rebuilds a heap
object to change only its metadata (`with-traits` is the canonical case) must
**copy the payload slice into the clone's own region** — `RegionSlice` is
`Copy`, and copying the `(ptr, len)` pair instead aliases backing pages in the
*source's* region with no counted edge: the source's ordinary demise then frees
the payload under the live clone (the with-traits UAF — a clone of `[1 2 3]`
captured by a spawned closure read freed pages in the send serializer;
tests/elle/region-withtraits-slice-uaf.lisp). It also falsifies the operation's
`Fresh` declaration, which claims the whole result lives in the call's own
region. The one sanctioned alias is the closure-env share (`squelch`/`attune`),
which pays for itself with an explicit backing edge in the free-cascade scan's
Closure arm.

