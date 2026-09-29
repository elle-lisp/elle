# Region representation — id-spaces, per-execution model, layout

<!-- audited: 2026-09-29 -->

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
construction, not by a runtime `!= 0` assert). Being distinct newtypes means a
static slot cannot be passed where a runtime region is expected. So "never index
a static id into `RegionStore`" is a compile error. "No region active" / "this
value has no region" is `Option<RuntimeRegion>` (`None`), never a sentinel `0`.

A region is **not** a uniform optional field hung on every
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
  `activation_region_map` (`runtime_region_for_alloc_slot`,
  `new_runtime_region_for_call_slot`, `take_runtime_region_for_drop_slot`),
  never indexed into the store as if it were physical;
- `new_runtime_region` never reissues an id that currently names a live region.

Drop either guard and two logical regions land on one physical id — a torn read
when one is freed under the other.

## The per-execution region model

A static region id is a per-function slot; every activation mints its own
physical region for it. `runtime_region_for_alloc_slot` records the
slot→physical mapping in the activation's frame, so the matching `DecrefRegion`
frees the same physical region. `take_runtime_region_for_drop_slot` clears the
slot, so the next loop iteration mints fresh. This is what makes deep recursion
and loops run in bounded memory: one static id names a per-function slot, never
a single live physical region shared across activations.

The model carries an emission-side obligation — **one allocation execution per
slot between drops**. `runtime_region_for_alloc_slot` mints fresh on every
execution and *overwrites* the frame's mapping. So if the lowerer emits N
allocation instructions against one slot with only the final `DecrefRegion`,
the first N−1 physical regions are orphaned the moment their mapping is
overwritten. Each carries an unreleasable initial reference, that is, a
structural leak. (Rule 4's dual: every allocation *execution* needs its own
demise. So every allocation *instruction* needs its own slot unless a drop
provably intervenes, as the loop back-edge drop does.)

The file-letrec capture-cell pre-pass is the case in point. `lower_begin` emits
one `MakeCaptureCell` per captured top-level binding. Routing them all through
the Begin's single region slot would orphan every cell but the last (at stdlib
scale, thousands of cells plus everything they pin). Pre-allocated capture
cells therefore get **one region per cell** (`begin_cell_regions`), each
released by its own `DecrefRegion` at its binding's last use.

A spliced call's args array is the same obligation with a different
resolution. It takes a managed slot of its own so the call's result mint cannot
orphan it. Its drop is the call's own: the runtime takes that slot
([mechanism.md](mechanism.md)).

A slot's execution can also **join** an existing region instead of minting one.
A `JoinRegion` instruction leaves a pending join on the fiber, and the very next
alloc-slot or call-slot mint consumes it. The slot resolves to the partner's
region, which the mint takes one reference on. The slot's usual release gives
that reference back, so the model's obligation is unchanged: one reference per
allocation execution, and one demise per reference
([colocation.md](colocation.md)).

## Constants lower as ordinary allocations, not promoted values

A heap literal — a string, an array/quoted form, a closure template — is **not**
a pre-allocated `Value` baked into the code object. The constant pool holds the
literal's immutable *template* (the bytes, the structure, the closure template)
as plain compile-time data. A `MaterializeConst` instruction builds a *fresh*
value from that template each time it executes, into the literal's own
solver-assigned region (`alloc_here(hir.id)`, the one-region-per-value baseline).
That region is resolved per activation to a fresh physical region, and the value
is allocated with `arena::alloc_in_region(obj, region)` — exactly as
`MakeArrayMut`/`List` do.

So a literal is born in the right region (Rule 3) and dies at its
`decref_point` (Rule 4). It lives past that point only by ordinary RC if it
escapes (Rule 5). Re-materializing per execution is the correct-and-slow
baseline. Runtime `(eval …)` and module load re-run the compiler, so the same
source materializes fresh copies each time, each reclaimed when it falls out of
use. The rejected alternative is a "constant-pool region" whose lifetime is the
code object. It would promote a value into a longer-lived region (Rule 3 forbids
promotion) and need a second demise mechanism outside `DecrefRegion`. It would
also share one region across every activation of the code object. Do not adopt
it.

Closure **templates** are no exception: the template is itself a
region-allocated heap object, materialized at its definition site. A closure
**instance** holds a normal cross-region reference to it, increfed when the
instance is built and cascade-released when its region frees. Region RC is the
single reclamation mechanism for code objects too. What the materialized
template holds, and why its bytecode is shared rather than copied per
execution, is [template.md](template.md).

## Physical representation

A per-thread page pool with size classes hands pages to regions on demand. It
takes them back when their region frees, by a count reaching zero or by its
owner's subtree drop. A page released past the pool's `max_cached` bound is
`munmap`ed at once. Regions never share pages (Rule 6). `RegionStore` holds one
`Reclaim` per physical region: a count for a `Counted` region, an owner for an
`Owned` one ([ownership.md](ownership.md)). One region per value, unmerged, is
the baseline: it claims a page per allocation, correct but expensive. Two
kinds of *merging* amortize that cost, both collapsing several solver `Region`s
onto one physical region. The consumer-facing performance account is in
[regions/performance.md](../../regions/performance.md).

- the **builder-idiom seed** merges a freshly-built child aggregate into the
  parent aggregate it is stored into (the `%pair` car/cdr store). The analysis
  and the runtime mint-or-reuse are in [merging.md](merging.md);
- **sibling page-amortization** — collapse sibling regions with coincident
  lifetimes and no edge between them. A later rider, not yet implemented.

Merging is one form of colocation. [colocation.md](colocation.md) owns the
whole set: the join rule every form obeys, the bound each pattern argues, and
which patterns are realized.

### The base page is the OS page

`base_page()` ([pagepool.rs](../../../src/value/fiberheap/pagepool.rs)) asks
the OS for its page size once and caches the answer. Class 0 of the size-class
ladder is that page, and every larger class is a power-of-two multiple of it. So
a region page is always a whole number of OS pages. `--region-page-size`
rejects anything below `base_page()`.

The rejected alternative is a fixed 4096. It is right on Linux x86-64 and wrong
on every host with a larger page — macOS aarch64 uses 16384, and Linux aarch64
can be built for 4096, 16384, or 65536. On such a host a fixed 4096 costs four
things at once, and the OS query removes all four together:

- **The ladder splits one physical page across three free lists.** Classes 0, 1
  and 2 name 4096, 8192 and 16384 bytes, but the kernel charges a full
  16384-byte page for each. `release` files a page by its recorded length, so a
  released class-0 page — physically 16384 bytes — cannot serve a class-2 claim,
  and the pool maps a fresh page instead.
- **`cached_bytes` undercounts, so `--page-pool-max` does not bound what it
  names.** `release` adds the page's recorded length. A class-0 page records
  4096 and holds 16384, so a 4 MB pool retains up to 16 MB per thread.
- **The trim `munmap` gets an address the kernel refuses.** A class-1 page
  over-allocates 16384 bytes and unmaps the suffix at `base + 8192`, which lands
  half an OS page past a page boundary, so `munmap` answers `EINVAL`. The
  mapping survives to `Drop`, which unmaps the whole rounded-up range, but until
  then the page holds 16384 bytes and records 8192.
- **`mapped_bytes` reports the same understatement.** It records the length each
  `mmap` asked for, not the mapping the kernel made. So the gauge that says
  whether a worker gave its heap back is short by 4× for every class-0 page.

Every accounting claim the pool makes rests on one identity: the size a page
records is the size the kernel charges. That holds only when class 0 is the OS
page.

## Page recycling: a claim is a free-list pop

**A page moves between a region and the pool untouched, in both directions.**
`release` pushes it onto a size-class free list; `claim` pops it and hands it
straight back. Neither reads nor writes a byte of it, and neither makes a
system call. The hot path of a small short-lived region — mint, claim, write
one object, free — therefore costs the write and nothing else. That matters
because it is the *common* path: one region per value is the baseline above,
so a program allocates regions at the rate it allocates values.

Nothing needs the page prepared. `RegionPage::new` stamps the header, sets the
object cursor to `HEADER_SIZE`, and sets the data cursor to the page top. Every
object slot is written before it is read, and every inline-data slice is fully
copied before its `RegionSlice` is handed out. **A claimed page's body is
therefore unspecified, not blank** — it holds whatever the previous occupant
left, and no reader is entitled to look.

Two consequences worth stating, because both are easy to get wrong:

- **Do not discard the page's frames at claim.** `madvise(MADV_DONTNEED)` on a
  page about to be written hands memory back that the very next store faults
  straight in. That costs a system call and a fault per claim, for no
  resident-memory reduction. Cached bytes are bounded by the pool's
  `max_cached`, and a page past that bound is `munmap`ed on release. That is
  where memory returns to the OS.
- **A page in the free list keeps its header.** A cached page still carries the
  `(region_id, generation, store)` stamp of the region that died on it. That
  stamp is exactly what a pointer outliving that region finds: the ids match,
  the generations do not, and the debug-build check panics at the deref site
  ([generations.md](generations.md)). Blanking offset 0 would take that
  detector away and leave the stale pointer with no self-validating header at
  its own page size. Then `region_of_ptr`'s page-base walk would mask past this
  page into memory the store does not own.

### `--trace=scrub`: make a stale read wrong on purpose

The one thing that writes a released page. Under `--trace=scrub`, `release`
zeroes the spans the dying region wrote — the object slots
`[HEADER_SIZE, obj_cursor)` and the inline-data suffix `[data_cursor, len)`,
together one `PageDirty` pair, sparing the header for the reason above. The gap
between the two cursors was never written by that region, so it is not scrubbed
either; a region holding one pair costs one 128-byte `HeapObject` slot of work.

Scrub turns a silent wrong read into a panic. A read through a pointer that
outlived its region normally finds the dead region's bytes: plausible,
well-typed, and wrong. Scrubbed, it finds an all-zero `HeapObject` slot, whose
tag matches no live value, so `arena::deref` panics naming the deref site.

Scrub is the cheap member of the family. `--trace=guardfree` never reuses a page
and so catches a stale read at any distance, at a mapping per freed page. The
generation check catches a stale *region resolution*, but only in debug builds
and only while the page is unclaimed. Scrub catches a stale *content* read, in
release builds too, for one `memset` per freed page. A page on its way to
`munmap` is never scrubbed, because an unmapped address faults on its own.

[tests/elle/region-page-recycle.lisp](../../../tests/elle/region-page-recycle.lisp)
measures what the claim path costs per call from Elle, through the
`arena/page-claims` gauge. `pagepool::tests` pins the untouched-recycle contract
and the scrub's spans.

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
  from the compiler's own bounded counter, as the two id-spaces above say.)
- **Free.** A teardown returns the id's pages, bumps its generation, and pushes
  the id onto `free_physical`, where the next mint finds it.

The reserved state has a second exit: a caller can mint an id and never allocate
into it. Two sites reserve an id ahead of work that may allocate nothing, and
that id never reaches `ensure_raw`, so no teardown can ever return it.

The **per-call result region** is the hot one. `dispatch_native_call` and
`dispatch_collection_call` each mint one region per call, before the call runs,
because the callee may allocate its result into it. A primitive that returns an
immediate (`(< a b)`), or one that returns a value borrowed from an argument
(`first`, `rest`, `get`), allocates nothing into it.

The **macro-expansion arena** is the other ([macroscope.md](macroscope.md)).
An expansion wraps each argument as a `Value` born in the arena, and an atom
argument becomes an immediate rather than a heap value. So an expansion whose
arguments are all atoms, and whose transformer allocates nothing, never touches
the arena. The scope's own open and close own that region: `begin_macro_scope`
mints it and answers with a `MacroScope` carrying the receipt, and
`reclaim_macro_scope` consumes the scope and returns the id. The expander
cannot name the region without holding the receipt, so it cannot reach the
close having lost it.

An id stranded that way costs no heap object, no page, and no reference count,
which is why the object and region gauges cannot see it. It costs the region
**table**: it raises the largest id a later mint hands out. `regions` is a
`Vec<Option<RegionEntry>>` indexed by id, so a stranded id is one
`size_of::<Option<RegionEntry>>()` slot of resident memory that nothing frees.
Resident memory then grows with total work while `arena/count`,
`arena/region-count`, and `arena/bytes` all stay flat.

So each site closes the reserved state itself, through one call:
`recycle_unmaterialized` pushes the id back onto `free_physical` when the mint
left it unmaterialized. Unmaterialized means two things together — `regions[id]`
is empty **and** the id's generation still equals the generation read at the
mint.

The generation half is what makes the test exact, and it is not optional. A
region that materialized and was freed inside the call (a native that re-enters
the VM) also leaves `regions[id]` empty. But its teardown already pushed that
id, so pushing it again would put a **duplicate** in `free_physical`. Two mints
could then take the same id before either materialized, and `new_runtime_region`
could not tell them apart: its skip loop only rejects an id that is already
*live*. Two logical regions on one physical id is the aliasing UAF the mint loop
exists to prevent. A teardown bumps the generation, so the generation check
rejects exactly that id and admits only a mint that nothing has touched since.

`arena/region-ids` reads `next_physical` from Elle — the gauge that moves the
moment an id fails to come back — and `arena/region-table` reads what the table
costs. The bound is pinned by the `id-*` probes of
[tests/elle/oracle.lisp](../../../tests/elle/oracle.lisp). They measure id
issuance per call against a live-growth discriminator of their own. A loop of
calls that allocate nothing issues no new id, and a materializing call's id
comes back by its teardown.
[tests/elle/region-macro-id-recycle.lisp](../../../tests/elle/region-macro-id-recycle.lisp)
gauges the expansion site the same way, against the same discriminator.
`regionstore::tests::recycle` pins the store-level contract, the duplicate the
generation check refuses included, and `arena::tests::macroscope` pins what the
scope's open and close owe each other.

## RegionSlice contents share their object's region

Non-obvious, and every claim below rests on it. Immutable aggregates (string,
array, struct, and a **closure's captured env**) store their variable-length
payload as a `RegionSlice` laid out *in the same region pages* as the HeapObject
header. Such contents therefore have **no** region of their own and **no**
cross-region RC edge — their lifetime *is* the containing object's region's
lifetime. Freeing the object's region frees its inline payload with it.

The consequence to keep in mind: a closure's captured environment dies with the
closure's region. A prematurely-freed closure region surfaces as a *torn
captured-env read* in `populate_env`, not as an RC underflow — there was never a
separate region to underflow. When `(squelch f …)` shares `f`'s env, the new
closure's env is copied inline into the new closure's region, so that region now
owns the captures.

The corollary for **metadata-only clones**: an operation that rebuilds a heap
object to change only its metadata (`with-traits` is the canonical case) must
**copy the payload slice into the clone's own region**. `RegionSlice` is
`Copy`, and copying the `(ptr, len)` pair instead aliases backing pages in the
*source's* region with no counted edge. The source's ordinary demise then frees
the payload under the live clone. That is the with-traits UAF: a clone of
`[1 2 3]` captured by a spawned closure read freed pages in the send serializer
([tests/elle/region-withtraits-slice-uaf.lisp](../../../tests/elle/region-withtraits-slice-uaf.lisp)).
It also falsifies the operation's
`Fresh` declaration, which claims the whole result lives in the call's own
region. The one sanctioned alias is the closure-env share (`squelch`/`attune`),
which pays for itself with an explicit backing edge in the free-cascade scan's
Closure arm.

