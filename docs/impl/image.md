# Images — regions hydrated at load

<!-- audited: 2026-10-20 -->

Design for image-style persistence: one mechanism, and the two configurations
it serves.

The two are the **boot** image (core, prelude, and stdlib pre-compiled into the
binary) and **environment** images (user `save`/`load`). Both use the same
format, dumper, and hydrator, and differ only in dependency list and dump
policy (see *One mechanism, two configurations*). This document owns the design
argument. Six companions carry the rest:

- [Foundations](image/foundations.md) — the five representation fixes the
  image rests on.
- [Sealing](image/sealing.md) — what the body may hold, what the hydrating
  instance rebuilds, and what fails the dump.
- [boot.md](image/boot.md) — the boot configuration: its root set, its source
  digest, the embedded blob and the warm cache.
- [format.md](image/format.md) — the file's sections, and the fingerprint that
  gates a load.
- [plan.md](image/plan.md) — the landing order, and the pins each milestone
  must land with.
- [measurements.md](image/measurements.md) — the eight experiments that
  answered the design's open questions, with their numbers.

This document states the design, not its progress. [plan.md](image/plan.md)
records what has landed: the warm-cache boot, the body fragment, the bytecode
parity gate, the interval table, per-worker hydration and the embedded blob
have, and the environment configuration has not.

## The problem

Without an image, every process start compiles core.lisp, prelude.lisp, and
stdlib.lisp — about 4300 lines — through the full pipeline: read, expand,
letrec analysis, regularize, type inference, region inference, lower, emit,
execute. Every `sys/spawn` worker repeats it on its own heap, and so does the
WASM entry.

A REPL or embedding session that builds up an environment loses it on exit.
The only durable stores are files and RDF; there is no way to save compiled
state.

## The idea: an image is a region, dumped and hydrated

An image is the page-level bytes of one real region — a compacted, sealed
copy of a value graph — plus a relocation table, name tables, and a manifest.
*Hydration* (loading) is:

1. map the image's pages privately into an aligned reservation — they become
   the pages of a freshly minted region,
2. one linear pass over the relocation table: rewrite each pointer slot to
   its target's new page address; remap primitive ids by name when the
   registry differs,
3. stamp each page header with the minted region id and the store's stamp,
   and rebuild the region's Rust-side lists (`dtors`, `ref_objs`, cursors)
   from the object index,
4. register the region as a process root and install the manifest's bindings
   into `PrimitiveMeta` and the expander.

No value is deserialized, nothing allocates per object, and untouched pages
are not even read. The cost is O(relocations) + O(objects) pointer pushes +
O(bindings), and the bulk of the bytes fault in lazily on first use: page
speed, not serde speed.

The hydrated region is not a new kind of memory. It is a `Counted` region
like any other: pinned for the process's life by the same process-root
registration that pins stdlib's exports today, released by
`teardown_process_root_regions` at exit, and freed by the ordinary cascade.
The [lifetime guarantee](../regions/lifetime.md) — after the program ends,
everything frees — holds without amendment. Leak
accounting, the generation checks, `--trace=scrub`, `--trace=guardfree`, and
`arena/dump` all see ordinary pages in an ordinary region.

Compaction is where the runtime win comes from. Stdlib boot today produces
hundreds of regions — one per letrec capture cell plus everything they pin
(405 measured; item 2 of [the measurements](image/measurements.md) records the
census). The image is **one** region: every internal reference is a self-edge,
so its edge tables are empty, and all RC traffic against stdlib values lands
on a single counter.

## One mechanism, two configurations

"Boot image" and "environment image" are not two kinds of image. There is
one format, one dumper, one hydrator, one verifier. Every use of either
name in this document means a *configuration* of that one mechanism; an image
differs from another only in its **dependency list** and its **dump
policy**:

- Every image declares the fingerprints of the images beneath it. A dump
  walks its roots and stops at any value a dependency layer already
  provides, emitting a cross-image reference instead of a copy — the
  visited map is seeded with the dependency regions' address intervals. The
  root set of a dump is therefore always the same thing: *the bindings the
  layers below do not already provide*.
- The **boot** configuration is the image with an empty dependency list. Its
  root is one struct over the core exports, the stdlib exports and the macro
  definitions ([boot.md](image/boot.md) owns the shape). Its policy is strict:
  no mutable bindings, in body or side-stream. The warm cache or the embedded
  blob distributes it, and it is regenerated from sources whenever the
  fingerprint or the source digest misses.
- The **environment** configuration depends on a boot image. Its roots are
  the session's bindings beyond that layer, and its policy is user-facing
  (refuse mutables by default, `&allow-mutable` opt-in). It is a file the user
  saves and loads explicitly: user data, not a regenerable artifact, and
  locked to the binary that dumped it (§ The binary lock is the current
  configuration).

Nothing limits the stack to two layers; an image may depend on any hydrated
stack whose fingerprints it names. Boot-over-nothing and
environment-over-boot are simply the two depths this design ships.

## The binary lock is the current configuration

An image is a region snapshot, and regions are in the language specification
([spec](../spec.md)). The fingerprint locks an image to the binary layout that
dumped it ([the image file](image/format.md)). That lock
solves this implementation's portability problem, and it is the configuration
the format ships today. It is not a property of an image.

A snapshot becomes portable once the code layer is in the specification: a
specified instruction set, a validator, and a rule for which compiled code
keeps loading. Until then an image hydrates only into a binary whose layout
agrees with the dumper's, and every other binary falls back as § Hydration
describes.

The HIR fragments an image carries (§ Compiler state is part of the
environment) are optional data to any other implementation. One that ignores
them grafts and fuses nothing across units, and every call stays a call. This
implementation still requires them, because its two boot modes must compile
user code identically.

## Rejected alternatives

- **CAS cache over a serialized code object** (a byte codec on the `send`
  types). Load re-allocates every object, re-interns every symbol, and
  rebuilds every `Rc` — O(objects) allocator and decode work on every start,
  and the measured shape confirms it: deserialization dominates the cache-hit
  path. This design keeps the *keying* — the warm-cache path below — and
  decodes no value.
- **Position-independent (offset-based) pointers in the runtime
  representation.** Zero-fixup load, but every deref pays the add forever.
  The region system's point is raw-pointer deref; do not tax it for a load
  path that runs once.
- **In-place capture of live regions.** Pages of a live heap are entangled
  with Rust-side state (`Rc` payloads, cursors, dtor lists) and fragmented
  across hundreds of regions. Dumping is the rare, slow operation; a
  compacting copy buys a dense, sealed, single-region body.
- **An immortal image store outside the region system.** Image values as
  permanently region-less foreign pointers would skip even the single
  counter and would let many stores share one mapping. But it is a second
  memory class: the leak suite, the freelog guard, the debug asserts, and
  the lifetime guarantee would each need a carve-out, and reclamation would
  gain a second mechanism beside region RC. Hydration keeps one mechanism;
  each store maps the image privately, and the clean pages are shared
  through the page cache anyway.
- **A copy-based hydrator** (claim pool pages, `memcpy` the image in). It
  would work with no pool changes, but it is a second implementation beside
  the mapped one, and its existence would let unmappable assumptions creep
  into the format and the runtime unnoticed. Mapping is the only hydrator,
  and its input is a mappable descriptor. Every source provides one: the
  warm-cache file, an environment-image file, the executable itself for the
  embedded blob, and an anonymous memory file for an image that arrives as
  bytes (below). This holds even if relocation dirties enough frames that
  mapping's cost approaches a copy's. One implementation, kernel-enforced
  immutability, and a clean set that grows as the layout improves beat a
  second code path at equal cost.
- **A constant-pool region owned by the code object** is rejected for
  ordinary constants ([the region model](region/model.md)); images do not
  change constant materialization. `MaterializeConst` keeps building fresh
  values per execution — image templates carry the same encoded
  `ConstTemplate` bytes, which are already name-stable.

## Compiler state is part of the environment

The heap is not the whole boot product. A later user compile reads two more
things the stdlib compile leaves behind:

- The body of each stdlib function, as a `LambdaFragment` encoded in the
  bytes its closure's code payload carries. A user compile grafts a call to a
  stdlib function from it ([grafting a call](graft.md)), and fusion inlines a
  named stdlib function from it
  ([dissolving a named function](dissolution/inline.md)).
- The signal-projection memo.

An image boot that leaves these out compiles user code *differently* from a
source boot — silently worse code, and divergent artifacts for anything keyed
on compiler output. That is not acceptable: the two boot modes must produce
identical user-code compiles.

A fragment is an HIR body closed over its own binding table
([the HIR design](hir.md)), so it is plain data that crosses a process boundary
as it stands. The payload carries it as bytes, like the docstring, so the
stdlib disk cache stores it with the payload ([the stdlib cache](stdlib-cache.md))
and an image maps it with the payload, as it maps the LIR (§ JIT), with no
re-derivation from syntax. A hydrated closure therefore grafts as a compiled one
does. The parity test in [the landing plan](image/plan.md) is the acceptance
gate.

## Hydration

[The image file](image/format.md) owns the sections this reads.

1. Validate the fingerprint; on failure, fall back (boot: compile sources;
   environment: report the mismatch).
2. Register the name table in the hydrating instance's display memo (one
   map serves both vocabularies).
   Replay the signal table so user signal bits land where the dump minted
   them. Check the primitive table against the live registry.
3. Mint a region and map the page section: reserve an aligned `PROT_NONE`
   range, then `MAP_FIXED` + `MAP_PRIVATE` each dumped page from the file
   into its slot. The section's base-page file alignment makes the offsets
   legal; the reservation gives each page the self-alignment the
   masked-header walk requires. The pages enter the region flagged
   **file-backed**: the pool neither caches nor recycles such a page, and
   its release is `munmap`. Generations, scrub's `memset`, and guardfree's
   `mprotect` operate on mappings unchanged.
4. Run the relocation passes. Rewrite pointer slots to
   `new_page_base + offset`, and resolve each primitive slot's name against
   this process's registry. Run the reconstruction stream's constructors and
   write their values into their slots. Record the cross-region edge each one
   creates, so the free cascade releases the target exactly once. It is one
   linear pass, and each write copy-on-write faults its 4 KiB frame private.
5. Stamp each page header; rebuild `dtors`, `ref_objs`, cursors, and
   `obj_count` from the object index.
6. Register the region as a process root; bump the watermark counters.
7. Rebuild `PrimitiveMeta` maps, expander macros, and `core_env` from the
   manifest — hundreds of entries, microseconds.
8. Decode side-stream mutable bindings (present only when the dump policy
   permitted them) through the existing `SendValue` deserializer into
   ordinary regions, rooted like REPL bindings. A function's LIR needs no step
   here: it is body data, mapped with everything else (§ JIT).

An environment image hydrates on top of a boot image and relocates its
cross-image slots against the boot region's hydrated pages. References
between the two hydrated regions are ordinary counted cross-region edges,
recorded during relocation; the environment region's `outgoing` table names
the boot region, nothing else.

Workers hydrate their own mapping. A region belongs to one `RegionStore`, so
each `sys/spawn` worker runs the whole hydration against the descriptor its
parent hydrated, instead of compiling the stdlib. Each worker holds a private
mapping, and the clean pages are shared between them in the page cache
([boot.md](image/boot.md) owns what a worker inherits). The WASM entry's
*host-side* runtime hydrates the same way. The WASM module itself still splices
stdlib source into the compiled unit; fixing that tier is the separate
stdlib-module work described in [wasm.md](wasm.md), not this design.

**The hydrator's input is `(fd, offset)`,** never a path. A path source is
opened first, and the embedded blob maps from the executable's descriptor. An
image that arrives as bytes — over the network, from Redis, from a channel —
is written into an anonymous file. It hydrates from that descriptor, with no
path any other process can open. A kernel with `memfd_create` mints one
directly. Every other host creates a file under `TMPDIR` and unlinks it before
writing a byte, so the descriptor is the only way back to the bytes and no
failure downstream can leave a file behind.

The split is by the call the platform has, not by the name it goes under:
Android is Linux to `memfd_create`, so a `target_os = "linux"` test decides the
wrong way there. Where a memfd is what was minted, it is write-sealed
(`F_SEAL_WRITE | F_SEAL_SHRINK`) before mapping, so the immutability the
mapping relies on is kernel-enforced. An unlinked file gets no seal, and
immutability is this process's discipline instead.

**The fallback is an unlinked file rather than POSIX shared memory,** which is
the obvious candidate and does not work. A Darwin shared-memory object answers
`mmap` only with `MAP_SHARED`: `pread` and `pwrite` to one fail with `ESPIPE`,
and a `MAP_PRIVATE` mapping of one fails with `EINVAL`. Private mapping is the
whole hydrator — every page arrives through `MAP_FIXED | MAP_PRIVATE` — so a
descriptor that refuses it cannot carry an image at all. It costs a write to
the filesystem where a memfd costs none, and that is the price of the one
descriptor every platform treats alike.

The offset must sit on a base-page boundary of the descriptor, and a misaligned
one is refused before anything is mapped ([the image file](image/format.md) owns
that rule).

**Never rewrite an image file in place.** The atomic temp-file-and-rename
discipline is what keeps a mapped old inode stable while a new image
replaces the path; `MAP_PRIVATE` over a file mutated in place is
unspecified. Each dump writes a temporary file no other dump names, so two
dumps racing for one path, from two processes or two threads, each rename a
whole file. A sealed memfd is immune by construction.

**An image is code — trust it like a shared library.** Hydration installs
raw memory and executes whatever the templates say; the verifier
bounds-checks pointers and tags, but it is a drift detector, not a sandbox.
Hydrate only images from sources you would `dlopen`. A network-retrieved
image deserves the same policy as a network-retrieved `.so`.

**Pointer resolution must not regress.** Resolving a pointer to its region
(`region_of_ptr`) is a probe ladder. It masks the address to each size-class
base and tests the 16-byte header, then confirms with `RegionPool::owns`, a
linear scan of the region's page list. Today's regions are one or two pages,
so both steps are cheap.

A single region holding all of stdlib breaks both assumptions. A pointer into a
large page fails every smaller class's header probe first, each a likely cache
miss into arbitrary image bytes, and the owns scan walks the whole page list. Resolution runs on the RC hot path, at every
escape incref and edge record of a stdlib value. So that is a per-operation
regression, not a curiosity.

The mapped layout dissolves it: a hydrated region is **one contiguous
address interval by construction** (the aligned reservation of step 3). The
region's pool records that interval, so `owns` answers a pointer inside it with
one range check. The store's **interval table** lists its hydrated regions,
one or two of them. Every resolution reads the table before the probe ladder:
`region_of_ptr`, and the ownership resolver behind the allocation funnel's edge
record and the free cascade's scan. Image pointers never enter the ladder at
all.

The interval belongs to the pool that owns the pages, so it goes when they go,
and a recycled id starts without one. A region leaves the table when its pages
are released. Page headers stay, stamped for uniformity and diagnostics, and
the hydrating store's resolution does not depend on them. A store that did not
hydrate the image, such as a worker reading its parent's value, resolves the
pointer through the stamped header, as it resolves any foreign page. The
stamps also argue for large image pages: each stamped header copy-on-write
dirties its 4 KiB frame, so fewer, larger pages keep the clean set large.

**The clean set is the currency.** Relocation copy-on-write dirties every
4 KiB frame that holds a pointer slot or a page-header stamp; those frames
pay a fault and a private copy. Frames with no relocations — bytecode,
strings, syntax, docs, name tables — stay clean. They fault in lazily, so a
function never executed is never read from disk. Every process mapping the
image shares them, and under memory pressure they are evictable rather than
swappable. The dumper therefore routes byte payloads
(`RegionSlice<u8>`) to dedicated data pages and keeps object shells and
`Value` slices on separate pages, maximizing the clean set.

## Dumping

The dumper is a compacting copy into a real region. It allocates a fresh
scratch region and walks the root set with a visited map keyed on payload
address, so cycles and sharing are preserved; `send` copies shared plain data
per edge. The walk builds sealed forms through the ordinary arena API: cells
snapped, strings and file names interned. Every internal reference is
a self-edge by construction. It records a relocation entry per pointer slot
as it writes, then dumps the scratch region's pages verbatim and drops the
region. Unsupported values fail the dump with an error naming the binding.

The walk down a list's `rest` spine is a loop, not a recursion. A list's
length is bounded by memory rather than by the text that built it, so a
recursive spine walk aborts the process on a stack overflow where a loop
writes the image. The spine reserves each copy before it fills it, exactly as
a closure does, so a cycle that re-enters a pair closes onto the one copy.
What hangs off the spine still recurses, and so does every other nesting.

The root set is defined once, in *One mechanism, two configurations*: the
bindings the dependency stack does not already provide. For the boot
configuration that is everything boot pins as process roots today; for an
environment dump it is the session's delta.

Dump determinism is engineered, not assumed. The walk visits roots and
children in sorted order and never iterates a hash map, so the same graph
always yields the same layout. Offsets, page table, relocation table, and
object index are byte-identical across dumps. The page bytes are assembled
canonically from a zeroed buffer, so headers, cursor gaps, alignment slack,
and relocation slots stay zero.

Every record the dumper writes receives only its discriminant byte and the
leaf-field extents the layout probes record ([format.md](image/format.md)).
The records are an object slot, a struct entry whose key is an
enum with padding of its own, and a syntax node, which is a struct with a
`bool` in it around an enum. A `repr(Rust)` enum copy carries uninitialized
padding from its construction temporary, so the dumper never copies a record
wholesale. The extent copy leaves padding out, and two dumps of the same graph
are byte-identical whole files. The warm cache keys on the source digest and
the binary's identity, not on a hash of the file. Whole-file determinism buys
reproducible embedded blobs and the check that a release binary dumps its own
blob again. Concurrent dumpers racing through the atomic rename produce
identical files.

Mutable bindings are the dump-policy fork. The strict policy (boot) fails
the dump, naming the binding. The environment policy defaults to the same
refusal with a `deep-freeze` suggestion, and `&allow-mutable` opts into
encoding via the side-stream, rebuilt as fresh mutables at load. The
manifest's per-binding kind field makes the opt-in mode additive.

## JIT

The JIT compiles from the LIR a code payload carries: region-native records in
the payload's own pages ([lir.md](lir.md)). The fifth
foundation put them there ([image/foundations.md](image/foundations.md) argues
it, and names the side-stream it deleted). A function's LIR therefore dumps in the
dumper's own walk and arrives with the mapping, so an image's save and load
need no mechanism of their own for it. `jit_cache` keys on the bytecode
address, which is stable for the hydrated region's life. Machine code itself is
never persisted: Cranelift output bakes absolute addresses and is not
relocatable.

A promotion copies. The JIT worker runs on another thread, and a region belongs
to one `RegionStore`, so `prepare_task` copies the function out of the payload
into a `LirOwned` the task owns.

Carrying the LIR is not optional for the boot configuration. An image boot
whose stdlib cannot reach the JIT tier trades startup for steady-state
throughput. That is a deal-breaker, and it violates the parity principle: the
two boot modes must be indistinguishable to running code, tiers included. A
hydrated stdlib closure promotes exactly as a compiled one does, so the tier
half of parity holds, as the compile half does ([image/boot.md](image/boot.md)
owns the gate that proves it).

The cost of the change is measured and small. The region form was faster on
every operation the Rust-heap form met, by about a third of one percent of the
compile it belongs to. It also held less memory
([image/measurements.md](image/measurements.md) items 7 and 8). The case for
it is never that number. It is that an image stops needing a mechanism of its
own for one type.

## Build integration

- **Warm cache (no build changes):** when no boot image hydrates, boot compiles
  from source, then dumps an image into `boot` under the `--cache=` directory,
  atomically. The file is named for the source digest and the binary's
  identity, and later starts hydrate it. So a build that carries no blob starts
  fast from its second run onward. [boot.md](image/boot.md) owns the key and
  the policy.
- **Embedded (release):** a release `make elle` builds twice and embeds the
  image the first build dumps, so a release binary's first start hydrates from
  its own executable. Embedding changes the binary and none of the
  fingerprint's inputs, and the dump is deterministic, so the second build
  dumps the blob it carries. [boot.md](image/boot.md) owns the two stages, the
  64 KiB alignment, and how a start finds the bytes.

## Verifier

The whole verifier is always on, in two passes: the tables before anything is
mapped, then the objects before the region is installed.

Every entry the hydrator decodes is bounds-checked as it is read:

- each page size is a power of two at or above the base page, and the sizes
  sum to the section;
- each relocation slot lies inside the image and is 8-byte aligned, and each
  target lies inside it;
- each object offset admits a whole `HeapObject` and carries a tag in the
  sealed set;
- the root names an object inside the pages, or carries an immediate tag and
  no pages at all.

An immediate root is what a graph that allocated nothing produces. So pages
beside one are drift, and the root's payload word means one thing rather than
either. This pass reads the file's tables rather than its page bytes, so it
costs no faults. It is what stops a corrupt table from writing outside the
image during relocation.

The sealing walk then reads each indexed object: its tag matches the index's,
every `RegionSlice` extent stays inside the image, and each page's cursors
bound the objects the index places on it. It reads object shells only, never
a slice's backing bytes: an extent is checked from the `ptr` and `len` in the
shell, so the clean set is untouched. A syntax object's root node rides in its
shell, so that node's two extents are shell reads like any other. The nodes
behind them are covered by the relocation bounds check, exactly as an array's
elements are.

A code payload's child table is checked one step further, because what it
names is read back as a header rather than as data. Each slot must name an
object the index itself calls a header. The shells themselves are already
resident: relocation writes a pointer slot in every object that has one, so
the frames this walk reads are the frames it just dirtied (§ "The clean set is
the currency"). An image whose objects hold no pointers at all is the one case
that pays a fault per page. Neither the boot nor the environment configuration
produces one.

The verifier is a drift detector, not a sandbox (§ Hydration). Format drift
fails loudly at load rather than as a torn read later, and a truncated or
scrambled file is refused by name instead of mapped.
