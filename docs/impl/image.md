# Images — regions hydrated at load

<!-- audited: 2026-09-09 -->

Design for image-style persistence: one mechanism, two shipped configurations.

The two are the **boot** image (core, prelude, and stdlib pre-compiled into the
binary) and **environment** images (user `save`/`load`) — the same format,
dumper, and hydrator throughout, differing only in dependency list and dump
policy (see *One mechanism, two configurations*). This document owns the design
argument. Three companions carry the rest:

- [foundations.md](image/foundations.md) — the four representation fixes the
  image needed first, all landed.
- [format.md](image/format.md) — the file's sections, and the fingerprint that
  gates a load.
- [plan.md](image/plan.md) — the landing order, and the pins each milestone
  must land with.
- [measurements.md](image/measurements.md) — the six experiments that
  dispatched the design's open risks, with their numbers.

## The problem

Every process start compiles core.lisp, prelude.lisp, and stdlib.lisp — about
3700 lines — through the full pipeline: read, expand, letrec analysis,
regularize, type inference, region inference, lower, emit, execute. Every
`sys/spawn` worker repeats it on its own heap. The WASM entry repeats it too.
Nothing persists between runs.

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
are not even read: cost is O(relocations) + O(objects) pointer pushes +
O(bindings), with the bulk of the bytes faulting in lazily on first use —
page speed, not serde speed.

The hydrated region is not a new kind of memory. It is a `Counted` region
like any other: pinned for the process's life by the same process-root
registration that pins stdlib's exports today, released by
`teardown_process_root_regions` at exit, and freed by the ordinary cascade.
The lifetime guarantee in [regions/lifetime.md](../regions/lifetime.md) —
after the program ends, everything frees — holds without amendment. Leak
accounting, the generation checks, `--trace=scrub`, `--trace=guardfree`, and
`arena/dump` all see ordinary pages in an ordinary region.

Compaction is where the runtime win comes from. Stdlib boot today produces
hundreds of regions — one per letrec capture cell plus everything they pin
(405 measured; [measurements.md](image/measurements.md) item 2 records the
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
- The **boot** configuration is the image with an empty dependency list:
  its roots are the boot bindings (stdlib exports, module closure, core
  exports, macro definitions, meta tables, inline-fn syntax), its policy is
  strict — no mutable bindings, in body or side-stream — and it is
  distributed by the warm cache or the embedded blob, regenerated from
  sources whenever the fingerprint misses.
- The **environment** configuration depends on a boot image: its roots are
  the session's bindings beyond that layer, its policy is user-facing
  (refuse mutables by default, `&allow-mutable` opt-in), and it is a file
  the user saves and loads explicitly — binary-locked user data, not a
  regenerable artifact.

Nothing limits the stack to two layers; an image may depend on any hydrated
stack whose fingerprints it names. Boot-over-nothing and
environment-over-boot are simply the two depths this design ships.

## Rejected alternatives

- **CAS cache over a serialized code object** (a byte codec on the `send`
  types). Load re-allocates every object, re-interns every symbol, and
  rebuilds every `Rc` — O(objects) allocator and decode work on every start,
  and the measured shape confirms it: deserialization dominates the cache-hit
  path. The content-addressed *keying* survives in this design (the
  warm-cache path below); the per-value decode does not.
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
  into the format and the runtime unnoticed. Mapping is the only hydrator;
  its input is a mappable descriptor, and every source provides one — the
  warm-cache file, an environment-image file, the executable itself for the
  embedded blob, and an anonymous memory file for an image that arrives as
  bytes (below). This holds even if relocation dirties enough frames that
  mapping's cost approaches a copy's: one implementation, kernel-enforced
  immutability, and a clean set that grows as the layout improves beat a
  second code path at equal cost.
- **A constant-pool region owned by the code object** is still rejected for
  ordinary constants ([region/model.md](region/model.md)); images do not
  change constant materialization. `MaterializeConst` keeps building fresh
  values per execution — image templates carry the same encoded
  `ConstTemplate` bytes, which are already name-stable.

## Sealing

After the foundations, the body may contain only *sealed* heap objects:
byte-self-contained, pointing only into this image (or an image it depends
on), and free of Rust heap ownership (no `Rc`, `Vec`, `Box`, or `RefCell`
inside) — page bytes must *be* the object. Sealed objects have no real
destructors, so the hydrated region's teardown drops are no-ops by
construction.

Sealed and portable after the foundations: `Pair`, `LString`, `LArray`,
`LBytes`, `LSet`, `LStruct`, `Syntax`, closure templates, closure instances
(env is an inline `RegionSlice<Value>`), keywords and symbols (payloads are
stable name hashes), native-fns (dense `prim_id`, remapped by name), ints
and floats, `Parameter`.

A sorted container copies in order and is never re-sorted. Every key an image
may carry ranks by its own content — a name hash for a symbol or a keyword, the
bytes for a string, its elements for an array, its structure for anything else
— so the order the dump wrote is the order the hydrating instance's comparator
agrees with, and a binary search over the mapped entries finds what it found
before. The keys that rank by address instead belong to values the dumper
refuses anyway.

**Capture cells are snapped, not persisted.** The stdlib file-letrec
allocates one `CaptureCell` (`Rc<RefCell<Value>>`) per captured top-level
binding. After the letrec fixpoint completes, a cell whose binding is never
`assign`ed again holds its final value; the dumper rewrites each closure env
to reference that value directly. The compiler knows which top-level
bindings are assigned anywhere in the file; the dumper refuses to snap
those. The boot image requires stdlib to have no post-boot-mutable
top-levels — a property the dump step enforces, and a reasonable one to
demand of a standard library.

Refused from the body outright: every mutable variant, `LBox`, `Fiber`,
thread and library handles, ports, externals, FFI signatures, managed
pointers. Mutable *bindings* may still be persisted through the side-stream
(below) where the image's dump policy permits it — the environment policy
does, opt-in; the strict boot policy does not. The `spirv` kernel cache is
the one true drop: the GPU path recompiles.

**Process-owned resources reconstruct in place.** The boot graph is not
fully pure: stdlib defines `*stdin*`/`*stdout*`/`*stderr*` as dynamic
parameters, and a `Parameter` heap object — itself sealed POD
(`{id, default, traits}`) — holds as its *default* an `External` wrapping
the stdio port. `send` already made the semantic call for this case: a
stdio port is reconstructed fresh on the receiving side, never carried.
The image does the same via the **reconstruction stream**: (slot location,
constructor tag) entries emitted by the dumper wherever it meets a
reconstructible resource. Hydration runs each constructor, allocates the
fresh value into a companion region (an ordinary region whose edge from the
hydrated region is recorded, so the teardown cascade releases it), and
writes the pointer into the listed slot — a handful of dirtied frames.
Reconstruction must be in place, not re-evaluation of the defining forms:
closures like `println` capture the `Parameter` object itself, so a
re-evaluated `def` would mint a second parameter the captured references
never see. Anything the dumper meets that is neither sealed nor
reconstructible nor side-streamable fails the dump with a named binding.

The **default trait tables** are the second reconstructible class, found by
the census ([measurements.md](image/measurements.md) item 2): every
collection's `traits` field points at one of the instance's two default
traitsets — `@struct`s built by `init_default_traits` at VM init, before any
stdlib load or hydration. They are instance infrastructure, not program
state, so the dumper never copies them: a `traits` slot aimed at a default
traitset becomes a reconstruction entry whose constructor resolves the
hydrating instance's own table for that tag. The tables exist before
hydration by construction (VM-init order), so the constructor is a lookup,
not an allocation.

**Macros persist whole.** A manifest macro entry carries its parameter
lists, its template syntax (a body value), and its transformer cache's body
location. The filled caches — ordinary closures — hydrate without
recompiling, preserving the hygiene property the lazy fill exists for: the
persisted transformer is the one compiled in a real expansion context,
which is exactly what later compiles reuse in a source boot. A cache the
boot never filled stays empty and fills lazily as today.

## Compiler state is part of the environment

The heap is not the whole boot product. Compiling stdlib also fills
compile-side registries on `CompileCtx` that later user compiles read:

- `FnInlineRegistry` — per-name HIR fragments of cross-unit-inlineable
  stdlib functions; user code inlines through it.
- `DispatchWrapperRegistry` and the signal-projection memo.

An image boot that leaves these empty compiles user code *differently* from
a source boot — silently worse code, and divergent artifacts for anything
keyed on compiler output. That is not acceptable: the two boot modes must
produce identical user-code compiles.

`FnInlineRegistry` holds `HirFragment`s — HIR bodies closed over their own
binding tables ([impl/hir.md](hir.md) § "A fragment is closed over its
bindings") — so the registry is plain data that crosses a process boundary as
it stands. The image records it the way the stdlib disk cache already does,
with no re-derivation from syntax and no dependency on the syntax foundation.
The parity test in [plan.md](image/plan.md) is the acceptance gate, and it
belongs to the **boot** milestone, not a follow-up.

## Hydration

[format.md](image/format.md) owns the sections this reads.

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
4. Run the relocation passes: rewrite pointer slots to
   `new_page_base + offset`; remap primitive payloads when the registry
   differs; run the reconstruction stream's constructors and write the
   fresh values' addresses into their slots. One linear pass; each write
   copy-on-write faults its 4 KiB frame private.
5. Stamp each page header; rebuild `dtors`, `ref_objs`, cursors, and
   `obj_count` from the object index.
6. Register the region as a process root; bump the watermark counters.
7. Rebuild `PrimitiveMeta` maps, expander macros, and `core_env` from the
   manifest — hundreds of entries, microseconds.
8. Decode side-stream mutable bindings (present only when the dump policy
   permitted them) through the existing `SendValue` deserializer into
   ordinary regions, rooted like REPL bindings. The LIR stream is not
   decoded here at all — it decodes lazily, per function, on JIT promotion.

An environment image hydrates on top of a boot image and relocates its
cross-image slots against the boot region's hydrated pages. References
between the two hydrated regions are ordinary counted cross-region edges,
recorded during relocation; the environment region's `outgoing` table names
the boot region, nothing else.

Workers hydrate their own mapping: a region belongs to one `RegionStore`,
so each `sys/spawn` worker runs steps 3–8 against the same image file
instead of `init_stdlib` — a private mapping per worker, not a compile,
with the clean pages shared between them in the page cache. The WASM
entry's *host-side* runtime hydrates the same way, but the WASM module
itself still splices stdlib source into the compiled unit; fixing that tier
is the separate stdlib-module work described in [wasm.md](wasm.md), not
this design.

**The hydrator's input is `(fd, offset)`,** never a path. A path source is
opened first; the embedded blob maps from the executable's descriptor; an
image that arrives as bytes — over the network, from Redis, from a channel —
is written into an anonymous memory file and hydrated from that descriptor
without touching a filesystem. A kernel with `memfd_create` gives one
directly, and every other host opens a POSIX shared-memory object and unlinks
it. The split is by the call the platform has, not by the name it goes under:
Android is Linux to `memfd_create` and has no POSIX shared memory at all, so a
`target_os = "linux"` test decides the wrong way there. Where a memfd is what
was minted, it is write-sealed (`F_SEAL_WRITE | F_SEAL_SHRINK`) before
mapping, so the immutability the mapping relies on is kernel-enforced.

**Only one of the two anonymous files is a file.** A memfd is one, and it
carries bytes both ways through `pread` and `pwrite`. A Darwin shared-memory
object answers `mmap`, `ftruncate` and `fstat` and refuses the rest, so both of
those calls fail on it with `ESPIPE`.

So every transfer to or from the descriptor goes through `ImageSource`, which
is the one type that knows which kind it holds. It fills a new object through a
writable shared mapping, and it serves the hydrator's header and section reads
from a read-only one, mapped from the base page below the offset asked for.
The hydrator's own page mappings need none of this — `mmap` is the call both
kinds of descriptor answer, and it is the only one the mapping path makes.

The offset must sit on a base-page boundary of the descriptor, and a misaligned
one is refused before anything is mapped ([format.md](image/format.md) owns
that rule).

**Never rewrite an image file in place.** The atomic temp-file-and-rename
discipline is what keeps a mapped old inode stable while a new image
replaces the path; `MAP_PRIVATE` over a file mutated in place is
unspecified. A sealed memfd is immune by construction.

**An image is code — trust it like a shared library.** Hydration installs
raw memory and executes whatever the templates say; the verifier
bounds-checks pointers and tags, but it is a drift detector, not a sandbox.
Hydrate only images from sources you would `dlopen`. A network-retrieved
image deserves the same policy as a network-retrieved `.so`.

**Pointer resolution must not regress.** Resolving a pointer to its region
(`region_of_ptr`) is a probe ladder — mask the address to each size-class
base, test the 16-byte header, then confirm with `RegionPool::owns`, a
linear scan of the region's page list. Today's regions are one or two
pages, so both steps are cheap. A single region holding all of stdlib
breaks both assumptions: a pointer into a large page fails every smaller
class's header probe first (each a likely cache miss into arbitrary image
bytes), and the owns scan walks the whole page list. Since resolution runs
on the RC hot path — every escape incref and edge record of a stdlib value
— that is a per-operation regression, not a curiosity.

The mapped layout dissolves it: a hydrated region is **one contiguous
address interval by construction** (the aligned reservation of step 3).
`region_of_ptr` consults a per-store table of hydrated-region intervals —
one or two entries, two compares each — before the probe ladder, and
`owns` for a hydrated region is the same range check. Image pointers never
enter the ladder at all. Page headers stay, stamped for uniformity and
diagnostics, but resolution does not depend on them — which also argues for
large image pages: each stamped header copy-on-write dirties its 4 KiB
frame, so fewer, larger pages keep the clean set large.

**The clean set is the currency.** Relocation copy-on-write dirties every
4 KiB frame that holds a pointer slot or a page-header stamp; those frames
pay a fault and a private copy. Frames with no relocations — bytecode,
strings, syntax, docs, name tables — stay clean: they fault in lazily
(functions never executed are never read from disk), every process mapping
the image shares them, and under memory pressure they are evictable rather
than swappable. The dumper therefore routes byte payloads
(`RegionSlice<u8>`) to dedicated data pages and keeps object shells and
`Value` slices on separate pages, maximizing the clean set.

## Dumping

The dumper is a compacting copy into a real region. It allocates a fresh
scratch region and walks the root set with a visited map keyed on payload
address (cycles and sharing preserved — unlike `send`, which copies shared
plain data per edge), building sealed forms through the ordinary arena API:
cells snapped, strings and file names interned. Every internal reference is
a self-edge by construction. It records a relocation entry per pointer slot
as it writes, then dumps the scratch region's pages verbatim and drops the
region. Unsupported values fail the dump with an error naming the binding.

The root set is defined once, in *One mechanism, two configurations*: the
bindings the dependency stack does not already provide. For the boot
configuration that is everything boot pins as process roots today; for an
environment dump it is the session's delta.

Dump determinism is engineered, not assumed: the walk visits roots and
children in sorted order and never iterates a hash map, so the same graph
always yields the same layout — offsets, page table, relocation table, and
object index are byte-identical across dumps. The page bytes are assembled
canonically from a zeroed buffer: headers, cursor gaps, alignment slack,
and relocation slots stay zero. Every record the dumper writes receives only
its discriminant byte and the leaf-field extents the layout probes record
([format.md](image/format.md) § Fingerprint) — an object slot, a struct entry
whose key is an enum with padding of its own, and a syntax node, which is a
struct with a `bool` in it around an enum. A `repr(Rust)` enum copy carries
uninitialized padding from its construction temporary — the store spike
measured this residue — so the dumper never copies a record wholesale; the
extent copy leaves padding out, and two dumps of the same graph are
byte-identical whole files. The warm cache still keys on the fingerprint,
not a content hash; whole-file determinism buys reproducible embedded blobs,
and concurrent dumpers racing through the atomic rename produce identical
files.

Mutable bindings are the dump-policy fork. The strict policy (boot) fails
the dump, naming the binding. The environment policy defaults to the same
refusal with a `deep-freeze` suggestion, and `&allow-mutable` opts into
encoding via the side-stream, rebuilt as fresh mutables at load. The
manifest's per-binding kind field makes the opt-in mode additive.

## JIT

The JIT compiles from `lir_function`, which is Rust-heap LIR and cannot live
in the body. Every image stores an encoded `LirFunction` per template in the
side-stream, decoded lazily the first time the hotness counter promotes that
function — so decode fees are paid once, only for hot code. `jit_cache` keys
on the bytecode address, which is stable for the hydrated region's life.
Machine code itself is never persisted: Cranelift output bakes absolute
addresses and is not relocatable.

The LIR stream is not optional for the boot configuration. An image boot
whose stdlib cannot reach the JIT tier trades startup for steady-state
throughput — a deal-breaker, and a violation of the parity principle: the
two boot modes must be indistinguishable to running code, tiers included.
The stream ships inside the **boot** milestone, and tier parity is part of
its acceptance gate.

## Build integration

- **Warm cache (default, no build changes):** when no valid boot image is
  available, boot compiles from source as today, then dumps
  `$ELLE_CACHE/boot/<fingerprint>.image` (written atomically: temp file,
  rename). Subsequent starts hydrate it. Development builds get fast starts
  from the second run onward.
- **Embedded (release):** a Makefile stage builds `elle`, runs
  `elle image dump-boot`, and rebuilds with the blob embedded (path passed
  by env var; `build.rs` declares the rerun-if). The blob is embedded
  64 KiB-aligned — an `include_bytes!` behind a `#[repr(align(65536))]`
  wrapper keeps its virtual address aligned, and load-segment congruence
  makes its file offset aligned too — so hydration maps it straight from
  the executable's own file, with the offset recovered from the static's
  address and the segment table (`dl_iterate_phdr`; the Mach-O load
  commands on macOS). The constant is the largest base page any supported
  host reports, because the alignment a blob needs is the *host's* page
  size and no smaller constant covers a 64 KiB arm64 kernel. Embedding
  changes the binary but not the fingerprint — every fingerprint input is
  computed from layout probes and sources, not from the binary hash — so
  the two-stage build converges in one iteration.

## Verifier

The whole verifier is always on, in two passes: the tables before anything is
mapped, then the objects before the region is installed.

Every entry the hydrator decodes is bounds-checked as it is read: each page
size is a power of two at or above the base page, and the sizes sum to the
section; each relocation slot lies inside the image and is 8-byte aligned;
each target lies inside it; each object offset admits a whole `HeapObject`
and carries a tag in the sealed set; the root names an object inside the
pages or carries an immediate tag. This pass reads the file's tables rather
than its page bytes, so it costs no faults, and it is what stops a corrupt
table from writing outside the image during relocation.

The sealing walk then reads each indexed object: its tag matches the index's,
every `RegionSlice` extent stays inside the image, and each page's cursors
bound the objects the index places on it. It reads object shells only, never
a slice's backing bytes — an extent is checked from the `ptr` and `len` in
the shell — so the clean set is untouched. A syntax object's root node rides
in its shell, so that node's two extents are shell reads like any other; the
nodes behind them are covered by the relocation bounds check, exactly as an
array's elements are. The shells themselves are already
resident: relocation writes a pointer slot in every object that has one, so
the frames this walk reads are the frames it just dirtied
(§ "The clean set is the currency"). An image whose objects hold no pointers
at all is the one case that pays a fault per page, and it is not a case the
boot or environment configurations produce.

The verifier is a drift detector, not a sandbox (§ Hydration). Format drift
fails loudly at load rather than as a torn read later, and a truncated or
scrambled file is refused by name instead of mapped.
