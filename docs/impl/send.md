# Sending a region

<!-- audited: 2026-10-22 -->

A design where a value crosses a thread as a sealed region whose pages move,
and crosses a process as an image: one copy, and no decode.

This document owns the design argument. [The plan](send/plan.md) owns the
landing order, the pins and the open risks, and
[mutable stores in region pages](mutable.md) owns the foundation it rests on.
The copy is [the dumper's](image/dumping.md), and the bytes are
[an image](image.md). Nothing here is implemented yet.

## The problem

`sys/spawn` moves a closure and its result as a `SendBundle`
([threads](../threads.md)). `SendValue` is a second encoding of every heap
variant, on the Rust heap: `SendValue`, `SendableClosure`, `SendKey`,
`SendSyntax`, and a serde mirror. The sender walks the graph into boxes,
vectors, strings and maps; the receiver rebuilds every object through an
allocation context. That is two copies and an allocation per object, and every
change to a heap layout is paid in both encodings. A code object crosses as its
fields, and its origin span does not cross at all.

The image dumper already walks the same graph into a sealed region, so every
new heap variant is written into two walks.

A channel uses neither. `chan/send` enqueues the value itself, so across
threads the receiver reads the sender's heap without holding a count on it.
When the sending worker exits, its heap tears down, and the receiver reads freed
pages: a worker that sends a heap string and exits crashes the process that
receives it, where an integer crosses fine.

`SendValue` has a byte form only for the stdlib disk cache, and only for a
subset of values. A process boundary needs bytes ([fleet](fleet.md)).

## A message is a sealed region

A send copies its root's graph once, with the dumper's walk, into a fresh
scratch region of the sender's heap. That region is **sealed**: every internal
reference is a self-edge, and every value the receiving instance must supply
for itself is a reconstruction entry ([sealing](image/sealing.md)). Then the two
transports part:

| Transport | Crosses to | After the copy | The receiver |
|-----------|------------|----------------|--------------|
| **move** | another store in this process: a worker, a joiner, a crossed channel | detach the region into a parcel | adopt the parcel's pages |
| **bytes** | another process, a file, Redis, a socket | write the image sections | hydrate from an anonymous file |

The bytes transport is an image exactly: a data image whose root is the message
([the image file](image/format.md)). This design adds no format.

## A parcel moves pages

A **parcel** is a sealed region between two stores: its pages with their
cursors, its object lists, its reconstruction entries, its name table, its
scope watermark, and the resources it carries. A parcel is `Send`, and it owns
its pages.

1. **Detach.** The sender's store takes the scratch region out of its table
   without releasing its pages, and recycles the id. The parcel takes the pages.
2. **Adopt.** The receiving store mints a region and adopts the parcel's pages
   as its own. It stamps each page header with the new region id and its own
   store, installs the object lists, and replays the name table into its display
   memo. It builds each reconstruction entry's value into a companion region and
   records the edge, as hydration does ([hydration](image/hydration.md)). The
   region is `Counted` at rc 1, held by the caller.

No pointer is rewritten. Within one process a page keeps its address, so every
internal pointer of the sealed region stays valid. The sender pays one copy, and
the receiver pays O(pages + objects + reconstructions), with no allocation per
object and no decode.

A region still belongs to one store at a time. Between the two stores the
parcel owns the pages, and Rust's move makes a second owner unrepresentable. A
parcel nobody adopts unmaps its pages and drops what it carries.

A page pool is a per-thread cache of process memory. A parcel's pages came from
the sender's pool, and when the adopted region frees they go to the receiver's.
The `mapped_bytes` gauge counts every thread's pages, so a moved page is
counted once throughout.

The policy below admits no variant that owns Rust-heap memory, and a carried
resource is rebuilt in the companion region. So freeing an adopted region frees
nothing outside its pages.

## The message policy

The dumper's refusals become one table with four configurations. Boot and
environment are [the image's](image.md); move and bytes are the transports.

| Value | Boot | Environment | Move | Bytes |
|-------|------|-------------|------|-------|
| immutable data, syntax, parameters, closures and their code | body | body | body | body |
| a mutable store ([mutable](mutable.md)) | refused | opt-in | body | body |
| a capture cell minted at run time | refused | opt-in | body | body |
| a default traitset, a standard-stream port | rebuilt | rebuilt | rebuilt | rebuilt |
| a channel half, an FFI type descriptor | refused | refused | carried | refused |
| a declared signal bit, a native-fn outside the canonical tables | refused | refused | body | refused |
| a fiber, a thread handle, a file or socket port, an FFI handle, a managed pointer, any other external, a WASM closure | refused | refused | refused | refused |

**Rebuilt** is a reconstruction entry. **Carried** means the parcel holds the
Rust-side object, a channel half with its wake list or a type descriptor, and
the receiver builds a fresh value around it in the companion region. The move
column admits what the bytes column refuses, because the signal registry, the
primitive registry, the file interner and the parameter counter are
process-wide. A bit, an id or a file means the same thing in every store of one
process, so the move transport writes no primitive or file stream at all.

A message is a copy. A mutable store sent is a new store on the far side, and a
mutation on either side is invisible to the other, as it is today.

Syntax carries hygiene scopes, and an expander mints them per instance. So the
parcel carries the scope watermark, and adoption raises the receiving
instance's expander past it, as a boot image does ([booting](image/boot.md)).

## Spawn and join

`sys/spawn` seals its closure into a parcel, and the worker adopts it into its
own store. The adopted region is the worker's receive region, which it releases
once the result is sealed. The worker seals its result into a parcel before its
heap tears down, and the thread handle's result slot holds the parcel.

The first `sys/thread-state` that finds the parcel adopts it into the joiner's
store. It records an edge from the handle's region to the adopted region and
releases the mint's reference, so the handle keeps the result. Every later join
answers that same value. Today each join rebuilds a fresh copy, so two joins
answer equal values. Under this design they answer one value, and a mutation
through the first answer shows through the second. A handle that dies unjoined
drops its parcel.

## Channels

A channel is **local** until either half crosses to another thread: in a spawn
closure's captures, in a join result, or inside a message on a crossed channel.
The walk that carries a half marks the channel **crossed**, in a flag both
halves share.

- On a local channel, `chan/send` enqueues the value itself, with the
  retain-and-release seam it has today. Fibers on one thread keep sharing a
  message.
- On a crossed channel, `chan/send` seals a heap message into a parcel, and an
  immediate rides as itself. A receive adopts the parcel and answers its root.
- **Crossing seals the buffer.** Before a half leaves, the walk drains every
  message already enqueued and enqueues each one again, sealed, in order. The
  channel was local, so every one of them was sent on this thread, and nothing
  races the drain.

The walk sets the flag on the sending thread, before the parcel that carries
the half reaches the other thread, and the handoff synchronizes the two. So no
send on the other thread precedes the flag. A receiver never meets another
store's page through a channel, and the guards that tolerate one today go.

## Bytes

Two primitives carry a value across a process boundary under the bytes column:
`image/encode` answers a value's image as bytes, and `image/decode` hydrates
bytes into a value, through an anonymous file. Decoding is hydration, so it
trusts its input as `dlopen` does ([hydration](image/hydration.md)).

The bytes are locked to the binary layout that wrote them, like every image
([the image file](image/format.md)). Data that must cross builds belongs in
files or RDF; this design provides no portable value format.

## What this deletes

- `src/value/send`: `SendValue`, `SendBundle`, `SendableClosure`, `SendKey`,
  `SendSyntax`, the serde mirror, and the template codec.
- The stdlib disk cache, the codec's one other user. The boot image's warm
  cache serves the starts it served. The three readers left — `--boot-image=off`,
  `Runtime::with_stdlib_cache`, and the workers of a parent with no image —
  compile instead ([the stdlib cache](stdlib-cache.md)).
- The cross-store read a channel makes, and the guards in `chan/send` and
  `chan/recv` that tolerate it.
- With the mutable foundation, the environment image's side-stream.

## Rejected alternatives

- **Keep `SendValue`, and deep-copy channel messages through it.** It fixes the
  crash and keeps two walks, two copies and a second encoding of every variant.
- **Share regions between threads.** Region RC would pay an atomic on every
  increment, and a region would belong to several stores at once.
- **Encode every send as image bytes, in-process too.** Relocation and an
  anonymous file buy nothing where every address stays valid.
- **Copy the pages at the receiver.** A second copy of memory both threads can
  already address.
- **Detach the value's own regions, with no copy at all.** It needs every region
  the graph reaches to be referenced from nowhere else, and it must rewrite each
  pointer into the sender's boot image. [The plan](send/plan.md) orders it after
  the copying send, which stays its fallback.
