# Mutable stores in region pages

<!-- audited: 2026-10-23 -->

A design to keep each mutable container's contents in its own region's pages,
so a mutable value is sealed data an image or a send can copy.

This is the sixth representation fix the image rests on
([foundations](image/foundations.md)), and [sending a region](send.md) rests on
it too. Every mutable store lies in region pages, behind the seam
[the stores](../../src/value/store.rs) define.

## Where a mutable store lives

| Variant | Store | Holds |
|---------|-------|-------|
| `@array` (`LArrayMut`) | `ArrayStore` | an extent of `Value` in the container's region |
| `@string` (`LStringMut`), `@bytes` (`LBytesMut`) | `ByteStore` | an extent of bytes in the container's region |
| `@set` (`LSetMut`) | `SetStore` | a B-tree of `Value` in one extent of the container's region |
| `@struct` (`LStructMut`) | `StructStore` | a B-tree of `(TableKey, Value)` in one extent of the container's region |
| box (`LBox`), capture cell (`CaptureCell`) | `CellStore` | the `Value` itself, inline in the shell |

Every holder of a mutable value holds the one shell, because the fibers of an
instance share one heap. `with-traits` builds a fresh store, so a traited copy
is independent of its source ([traits.rs](../../src/primitives/traits.rs)). The
store type is the seam: a read takes a guard, and only the store funnels write
([region rules](region/rules.md)).

A store on the Rust heap is a `malloc` per container and a `realloc` per
growth, which `--region-page-size`, `arena/page-claims`, `--trace=scrub` and
`--trace=guardfree` cannot see. Freeing a region would have to drop it, and an
image or a send could not copy it as page bytes ([sending a region](send.md)).

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

A one-slot container needs no extent: a read copies the `Value` out, and a
store goes through the funnel. No read holds the slot, so it needs no borrow
count either.

A shell with an extent keeps a **borrow count** with the rules a `RefCell`
keeps, so a write that overlaps a Rust-side read still panics. The count is
plain bits, and a dump writes it as zero. A store in region pages owns nothing
outside them, so its variant needs no destructor. The free still scans the
shell: its `traits` edge, and each `Value` in its live extent, its tree or its
one slot.

## A sorted store is a B-tree in one extent

An `@set` and a `@struct` keep their entries in key order, in a B-tree whose
nodes lie in one extent of the container's region. A sorted extent shifts its
tail on every insert and delete, which is O(n), where a B-tree is O(log n). An
insert and a delete at 10⁵ random keys stay within twice the time std's
`BTreeSet` or `BTreeMap` takes on the same keys. A benchmark holds both
variants to that.

The tree stores each entry once, in node order, as std's `BTreeMap` does. A
B+tree keeps copies of keys in its internal nodes, and a copy can outlive the
entry it copied. A probe compared against a copy of a heap value whose region
freed reads a freed page, so the tree keeps no copies.

| Part | Holds |
|------|-------|
| node | up to 11 entries in order, and the index of each of its 12 children |
| shell | the node extent, the root's index, the height, the head of the free list, the entry count |

A child is named by its node's index in the extent, never by a pointer. A
growth moves the whole extent, so no node needs relocating, and the extent's
borrow count, growth and foreign-shell rule hold for the tree unchanged. Every
node but the root holds at least 5 entries. A free node holds none, and names
the next free node. A sorted store is a type of its own over its extent, so no
`@array` write can reach its order.

- **Lookup** descends from the root by a binary search within each node.
- **Insert** under a key already present adds no entry. An `@set` keeps the
  member it holds, and a `@struct` put replaces the value in place and keeps
  the stored key, so neither claims. A leaf with room takes a new entry. A
  full leaf splits around its middle entry, which moves up into the parent,
  and a full parent splits in turn. Before it edits anything, the insert
  counts the nodes its splits need. It takes them from the free list, and
  appends blank nodes to the extent for the rest. That append is the only
  claim a write makes, and it claims only when the extent is full. A growth
  that panics on a foreign shell therefore leaves the tree whole.
- **Delete** replaces an entry in an internal node with its predecessor, and
  removes the predecessor from its leaf. A node left with fewer than 5 entries
  takes one through the parent from a sibling that can spare one. Otherwise it
  merges with a sibling and the parent's entry between them. A merged-away
  node, and a root left with no entries, go on the free list. A delete never
  claims.
- **Iteration** walks the entries in order from both ends. Each end keeps the
  path from the root to its cursor. A child index is a `u32`, which holds the
  height below 16.
- **Build** loads a store from its entries in order into exactly the nodes they
  need. An empty store claims nothing, and its first insert claims one node.

An extent's growth claims at least four items, and at least eight bytes of
small ones, so a store built empty does not claim again for each of its first
few items. That floor stops at 256 bytes, so a store of nodes grows from one
node. Freezing an `@set` or a `@struct` walks it in order into the immutable
twin's slice. The free reads each entry in the same walk, and the census counts
the bytes of the node extent.

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

- **Node overhead.** A leaf carries the child indices an internal node uses,
  and a node is at least half full. A tree's nodes can take three times the
  bytes of its entries.
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

Each variant moved behind the store seam in a step of its own, green on its
own, and every step has landed.
[The send plan](send/plan.md) holds the pins the steps landed with, and
[the image plan](image/plan.md) orders the foundation among the image
milestones.
