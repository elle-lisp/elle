# Sending a region: the plan

<!-- audited: 2026-10-22 -->

The order the send design lands in, the pins each milestone lands with, and
the risks still open.

[The send design](../send.md) owns the argument, and
[mutable stores in region pages](../mutable.md) owns the foundation. The first
milestone's seam, byte step, `@array` step, cell step and `@set` step have
landed, and nothing else here has.

## Landing order

Each milestone lands green on its own, and each deletes the machinery it
replaces in the same change.

1. **mutable**: the foundation. The seam first, then one variant per step, in
   the order [the foundation](../mutable.md) gives. [The image plan](../image/plan.md)
   orders it among the image milestones too.
2. **policy**: the dumper's refusals become the four-column message policy
   ([the send design](../send.md)). Boot keeps the answers it gives today, and
   the other three columns are written and tested before any caller selects
   them.
3. **parcel**: detach a sealed region from one store and adopt it into another,
   with the companion region, the name replay and the scope raise. The
   parcel's first callers are tests that move one between two runtimes.
4. **spawn**: `sys/spawn` moves its closure as a parcel, and the worker moves
   its result back as one. `SendBundle` leaves the thread handle, and the test
   runner's syntax shipment moves with it.
5. **channels**: the crossed flag, a sealed send on a crossed channel, the
   buffer sealed at crossing, and channel halves carried in a parcel. The
   foreign-heap guards in `chan/send` and `chan/recv` go.
6. **bytes**: `image/encode` and `image/decode`, over the bytes column.
   [Fleet's store](../fleet/plan.md) waits on this milestone.
7. **delete**: the stdlib disk cache goes, and then `src/value/send`, with
   `SendKey` and the template codec. [The stdlib cache](../stdlib-cache.md) and
   every document that cites it go in the same change.

Two milestones come later, each with the copying send as its fallback:

8. **layers**: a message stops copying what the boot image provides, and
   writes a reference into the layer beneath, as an environment dump does
   ([image.md](../image.md)). A spawned closure that calls `map` then carries
   no copy of `map`.
9. **detach**: zero-copy detach of a value's own regions, where the graph is
   reachable from nowhere else.

## Test plan

A pin that fails today names the failure it shows. A pin that holds today
guards a behavior its milestone must keep.

### mutable

- **Growth.** An `@array`, an `@string`, an `@bytes`, a `@set` and a `@struct`
  each grow past three doublings, and every element reads back.
- **Gauges.** A loop that pushes onto one store raises `arena/page-claims`. A
  Rust-heap store keeps the count flat, because `realloc` serves its growth.
- **Teardown.** A store in region pages has no destructor, so its variant
  leaves a region's destructor list for its reference list.
- **Foreign shells.** A growing write to a store whose shell lies in another
  heap panics, and the panic names the cause.
- **Borrows.** A write while a Rust-side read of the same store is live panics,
  as a `RefCell` does today. Two dumps of one `@array`, one of them taken while
  a read is live, are byte-identical, because a dump writes the count as zero.
- **Copies stay copies.** A mutation through a `with-traits` copy leaves its
  source unchanged, and a write's payload survives a growth of the store it was
  read from while the write waits.
- **Benchmark.** `@struct` put and delete at 10³, 10⁴ and 10⁵ entries gate the
  `@struct` step ([the foundation](../mutable.md) names the fallback).

### policy

- One value per row of the policy table, dumped under each column. Every
  refusal names the variant and the configuration that refused it. Every
  admission round-trips.
- Counter-factual: boot refuses an `@array` that the move column admits, so one
  table with one column per configuration is what the test reads.

### parcel

- **Two stores.** Seal a list of heap strings in one runtime, detach it, adopt
  it into a second runtime on the same thread, and drop the first runtime
  whole. The second reads every string. Read the first runtime's pointer
  instead, and the read faults.
- **No decode.** Adopting a parcel of 10⁴ objects allocates only its
  reconstruction entries in the receiving store. A decode would allocate one
  object per object sent.
- **Pages.** The adopted region names the receiving store in every page header,
  and the `mapped_bytes` gauge reads the same before and after the move. A
  parcel dropped unadopted returns the gauge to its baseline.
- **Names and scopes.** The receiver prints a symbol and a keyword it never
  interned. Adopting a syntax value raises the receiver's scope counter past the
  parcel's watermark.
- **Teardown.** Under `--trace=guardfree`, the adopted region's free releases
  its companion region exactly once.

### spawn

- **Join identity.** Two joins of one handle answer one object: `%identical?`
  holds, and a push through the first answer shows through the second. Today
  each join builds a fresh copy, so `%identical?` is false.
- **Copy semantics.** A worker that mutates a captured `@array` leaves the
  spawner's copy unchanged, and the spawner's later mutation is invisible to
  the worker.
- **Process-wide state.** A closure whose signal names a declared bit crosses a
  spawn, and the worker raises that signal by name.
- **Bounded memory.** [The worker heap pin](../../../tests/worker_heap.rs)
  holds: a program that spawns workers in sequence peaks at its largest worker.
  An unjoined handle that dies releases its parcel's pages.

### channels

- **The crash.** A worker sends a heap string on a channel and exits, and the
  receiver reads the string after the join. Today the process faults with
  SIGSEGV, in release and in debug.
- **The buffer.** A worker enqueues two heap strings on a channel of its own,
  answers the receiver as its result, and exits. The joiner receives both, in
  order. Today it faults, because crossing does not seal the buffer.
- **Local channels share.** On a channel that never crossed, the received
  message is `%identical?` to the sent one. Sealing every send would break this
  pin.
- **Crossed channels copy.** A push into an `@array` after its send is
  invisible to a receiver on another thread, and an immediate arrives as
  itself.
- **Carried halves.** A sender that rides inside a message on a crossed channel
  delivers to its receiver on the first thread, and its own channel reads as
  crossed.
- **One store.** In a debug build, every heap message a receive answers lies in
  the receiving store.

### bytes

- **Round-trip.** Every row the bytes column admits encodes and decodes to an
  equal value, in a fresh process as well as in the encoding one.
- **Determinism.** Two encodings of one graph are byte-identical.
- **Refusals.** A channel half, a declared signal and a native-fn outside the
  canonical tables each fail the encode by name.
- **The lock.** Bytes with one fingerprint byte flipped refuse the decode by
  name, before anything is mapped.
- **No residue.** A decode leaves no file in `TMPDIR`.

### delete

- A parent with no boot image spawns a heavy worker, and the worker compiles
  the standard library and answers. `--boot-image=off` and
  `Runtime::with_stdlib_cache` compile in the same way.

## Open risks

1. **Small messages on a crossed channel.** A parcel holds whole pages, so a
   one-integer struct sent across threads costs a page claim, a detach and an
   adopt. Measure message throughput on a crossed channel before the channels
   milestone lands. A small-message path that copies into a page the channel
   owns is the candidate answer.
2. **Every route between threads must run the walk.** The flag is correct only
   if every value that reaches another thread passes through the walk: a spawn
   closure, a join result, and a message on a crossed channel. A new route that
   moves a value some other way reopens the crash, and the debug-build store
   check above is the pin that finds it.
3. **Join identity is a change users see.** A program that mutated one join's
   answer and expected the next join to be fresh reads the mutation. The
   corpus has no such program, and [threads](../../threads.md) states the new
   rule when the spawn milestone lands.
4. **Pages migrate between pools.** A worker that sends steadily moves its
   pages into the receiver's pool, and `--page-pool-max` bounds what the
   receiver caches. Measure a producer and a consumer at a steady rate for
   unbounded growth in either pool.
