# Mutable stores in region pages

<!-- audited: 2026-10-22 -->

A design to keep each mutable container's contents in its own region's pages,
so a mutable value is sealed data an image or a send can copy.

This is the sixth representation fix the image rests on
([foundations](image/foundations.md)), and [sending a region](send.md) rests on
it too. The seam is in place ([the stores](../../src/value/store.rs)). The
stores of `@string`, `@bytes` and `@array` lie in region pages, and a box or a
capture cell holds its value inside its shell. The stores of `@set` and
`@struct` keep their contents on the Rust heap.

## Where a mutable store lives

| Variant | Store | Holds |
|---------|-------|-------|
| `@array` (`LArrayMut`) | `ArrayStore` | an extent of `Value` in the container's region |
| `@string` (`LStringMut`), `@bytes` (`LBytesMut`) | `ByteStore` | an extent of bytes in the container's region |
| `@set` (`LSetMut`) | `SetStore` | `Rc<RefCell<BTreeSet<Value>>>` |
| `@struct` (`LStructMut`) | `StructStore` | `Rc<RefCell<BTreeMap<TableKey, Value>>>` |
| box (`LBox`), capture cell (`CaptureCell`) | `CellStore` | the `Value` itself, inline in the shell |

The `Rc` buys no sharing. Nothing clones it: `with-traits` builds a fresh store,
so a traited copy is independent of its source
([traits.rs](../../src/primitives/traits.rs)). Every holder of a mutable value
holds the one shell, because the fibers of an instance share one heap. A write
that overlaps a Rust-side read of the same store panics: the `RefCell` refuses
it, and so does an extent's borrow count. The store type is the seam: a
read takes a guard, and only the store funnels write
([region rules](region/rules.md)).

A Rust-heap store costs three things:

- **Allocation the region gauges cannot see.** Each container is a `malloc`,
  and each growth a `realloc`. `--region-page-size`, `arena/page-claims`,
  `--trace=scrub` and `--trace=guardfree` see none of it.
- **Destructors.** Two variants own Rust-heap memory, so freeing a region
  that holds one must drop it.
- **No seal.** A mutable value is not page bytes, so an image refuses it and a
  send needs a Rust-heap mirror of it ([sending a region](send.md)).

## The store is an extent in the container's region

A mutable container keeps its contents in an **extent** of its own region: the
shell holds the extent's pointer, its length and its capacity. That is a
`RegionVec` without its arena pointer ([lir.md](lir.md)). One extent type,
generic over what it holds, serves every store that keeps one. A store is built
with an extent exactly its length, and an empty store claims nothing.

A full extent grows by claiming one at least twice as large in the same region
and copying into it. The old extent is dead until the region frees, as a
`RegionVec`'s is. Doubling bounds the dead extents of one store below its
capacity.

A growth resolves the container's region through the store funnel: the region
of the funnel's heap whose pages hold the shell. A shell that no region of that
heap holds belongs to another thread's heap, and a channel delivers one today
([sending a region](send.md)). The funnel panics rather than grow it. A write
that fits the capacity claims nothing, so it resolves no region.

| Variant | Store in region pages |
|---------|-----------------------|
| `@array` | an extent of `Value` |
| `@string`, `@bytes` | an extent of bytes |
| `@set` | a sorted extent of `Value`, the layout `LSet` uses |
| `@struct` | a sorted extent of `(TableKey, Value)`, the layout `LStruct` uses |
| box, capture cell | the `Value` itself, inline in the shell |

A one-slot container needs no extent: a read copies the `Value` out, and a
store goes through the funnel. No read holds the slot, so it needs no borrow
count either. A sorted store keeps the immutable twin's
layout, so freezing one copies a single extent and a lookup is the same binary
search. An insert or a delete shifts entries within the capacity.

A shell with an extent keeps a **borrow count** with the rules a `RefCell`
keeps, so a write that overlaps a Rust-side read still panics. The count is
plain bits, and a dump writes it as zero. A store in region pages owns nothing
outside them, so its variant needs no destructor. The free still scans the
shell: its `traits` edge, and each `Value` in its live extent or its one slot.

## What does not change

- **Identity.** A mutable value is its shell's address, and every fiber of the
  instance reads the same shell.
- **Edges.** Every store still goes through the funnels, which record and
  release the container's cross-region edges ([region rules](region/rules.md)).
  A growth copies entries, not edges: an edge counts region to region, so the
  copy moves none. A value in a dead extent is never read again.
- **The copies that exist for a reason.** `with-traits` copies the store into
  the clone's region. A write copies a mutable payload before the kernel reads
  it, because a growth can move the extent while the operation waits
  ([the bytes an operation holds](io-bytes.md)).
- **Hydrated regions.** A mutable store in a hydrated region grows into pages
  the region claims after hydration, as a hydrated region already may
  ([hydration](image/hydration.md)).

## What it deletes

- The Rust-heap stores of the seven variants. Freeing a region of mutable
  values frees nothing outside its pages.
- One Rust-heap allocation per container and one per growth.
- The environment image's side-stream. A mutable store is body data, so
  `&allow-mutable` becomes a switch in the dump policy rather than a second
  encoding ([dumping](image/dumping.md)).
- The mirror of each mutable variant a send needs ([sending a region](send.md)).

## Costs and risks

- **Sorted inserts shift.** A `@struct` insert or delete is O(n) in a sorted
  extent where a `BTreeMap` is O(log n). A put-heavy benchmark at 10³ to 10⁵
  entries gates the `@struct` step. A B-tree whose nodes are extents in the
  container's region is the fallback.
- **Dead extents last as long as the region.** A long-lived region that holds a
  growing store keeps up to its capacity again in dead extents. A process-root
  registry that only grows pays that once per doubling.
- **Shell size.** Pointer, length, capacity, borrow count and `traits` must fit
  within the `HeapObject` size the `Syntax` variant already sets, and the layout
  probes pin every variant ([the image file](image/format.md)).
- **Foreign shells.** Until a crossed channel copies its messages, a store that
  arrived from another thread can be read and edited within its capacity, and
  its growth panics.

## Landing

The seam is the first step: every read and every write of a mutable store goes
through methods on the store, so no call site names the representation. A read
takes a guard that shows the contents as a slice, or answers a lookup, and
never shows the collection behind it. The store funnels take every write of a
`Value`. A byte store has one funnel, a splice, and it takes the heap as the
others do, because a growth claims an extent. Then one variant moves per step,
each green on its own: `@bytes` and `@string`, then `@array`, then box and
capture cell, then `@set`, then `@struct` behind its benchmark. A step changes
its own store file, its funnels and its constructors. The `@array` step also
puts the byte store on the shared extent type. The seam, the byte step, the
`@array` step and the cell step have landed.
[The image plan](image/plan.md) orders the foundation among the image
milestones.
