# Images — regions hydrated at load

Design for image-style persistence. One mechanism with two shipped
configurations: the **boot** image (core, prelude, and stdlib pre-compiled
into the binary) and **environment** images (user `save`/`load`) — the same
format, dumper, and hydrator throughout, differing only in dependency list
and dump policy (see *One mechanism, two configurations*). This doc owns the
design argument. Nothing here is implemented yet; the test plan at the end
names the pins each milestone must land with.

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
(405 measured; risk item 2 records the census). The image is **one**
region: every internal reference is a self-edge, so its edge tables are
empty, and all RC traffic against stdlib values lands on a single counter.

## One mechanism, two configurations

"Boot image" and "environment image" are not two kinds of image. There is
one format, one dumper, one hydrator, one verifier. Every use of either
name in this doc means a *configuration* of that one mechanism; an image
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

## Foundations

The image effort does not start with images. Four representation fixes come
first. Each is independently valuable at runtime today, lands green against
the existing corpus with no image machinery, and **deletes** image machinery
that would otherwise have to be built and then thrown away. Building the
image first would mean shipping remap passes, re-sort passes, and a syntax
codec whose only purpose is to compensate for representations we intend to
fix anyway.

### Stable symbol identity

`SymbolId` is a dense per-table index minted in first-intern order — a
process-local accident. Everything downstream compensates: every code object
carries a `symbol_names` map for cross-table remap, `send` re-interns by
name, `CompileCtx` leans on a fragile "registration order is deterministic"
invariant, and — the sharpest edge — immutable structs and sets are *sorted
arrays* whose order (`TableKey`/`Value` compare symbols by raw id) is
process-local, so a persisted sorted container is correct only where it was
built.

The fix is the identity model keywords already use: the id is a 64-bit
FNV-1a hash of the name, stable across tables, processes, and builds. Then
symbol values are as portable as keyword values; sort orders are stable by
construction; the image needs no symbol remap pass, no re-sort pass, no
symbol watermark; templates lose their `symbol_names` maps; `send` stops
re-interning for identity; the `CompileCtx` ordering invariant dissolves.

No global registry backs this. Minting an id consults nothing — the hash
is the id — so the only remaining jobs are display (hash→name) and
collision detection, and both stay per instance. `SymbolTable` survives as
an instance-local hash→name **display memo** on the plumbing it rides
today: no lock, because no structure is shared; teardown drops the memo
with its instance, so the leak guarantee holds without a carve-out. The
memo learns names at the boundaries where names already travel — the
reader, `send`'s name field (now display-only data the receiver records),
hydration's name-table replay, `ConstTemplate` decode — and every learning
site doubles as the collision check: recording a hash the memo maps to a
different name is the panic. That catches cross-thread, cross-image, and
cross-build collisions at the moment they would first confuse a reader,
which one process-wide table cannot. A CI test hashes every static name —
the primitive tables and aliases, every symbol read from core, prelude,
and stdlib — and asserts distinctness, so the built-in vocabulary is
proven collision-free per build; the runtime panic covers dynamic names,
where the 64-bit birthday bound is ~3×10⁻¹⁰ at 10⁵ symbols. The keyword
registry takes the same demotion in this migration.

Migration notes: `SymbolId(u32)` widens to `u64` (the `Value` payload
already is one); the `SYNTHETIC` sentinel survives as a reserved hash;
struct and set iteration order changes from intern-order to hash-order —
deterministic, and measured to churn nothing. Risk item 4 records the
full site audit and the measured cost: no bytecode operand carries a
symbol id, no live dense-index site exists, and the widening is ~75
mechanical seam edits.

### Region-native immutable structs

`LStruct` holds `Vec<(TableKey, Value)>` and `TableKey::String` owns a
`String`. Move the payload to `RegionSlice<(TableKey, Value)>` with string
keys inline in the region, as arrays and strings already do. Payoff now:
immutable structs stop allocating on the Rust heap. Payoff for images:
structs become body data.

### Region-native closure templates

`ClosureTemplate` is ~20 `Rc`/`Vec` fields. [region/model.md](region/model.md)
already specifies the target: a region-resident template whose bytecode is an
inline `RegionSlice<u8>`. Flatten every field: constants as
`RegionSlice<Value>`, masks and release tables as inline slices, the location
map as a sorted `RegionSlice<(u32, PackedLoc)>` over an interned file table,
`child_protos` as `RegionSlice<Value>` of sibling templates, name and doc as
region strings (`symbol_names` is gone per the above). Payoff now:
`MakeClosure` clones the blueprint — ~20 `Rc` bumps per closure creation —
into the instance region; a flattened template is referenced, not cloned.
Payoff for images: code objects become body data.

### Region-native syntax

Syntax is load-bearing for images, not a reconstruction nicety: a macro *is*
its template (`MacroDef.template: Syntax` plus parameter lists; the compiled
transformer is a lazily filled cache), so the expander cannot be rebuilt
without syntax. Today `Syntax` is a Rust-heap `Box` tree, which would force a
side-stream codec (an extended `SendSyntax`) and a lazy-decode seam — a
serializer whose entire job is to work around the representation.

Make syntax a region-native immutable tree instead: nodes and child slices
inline in region pages, spans and hygiene scopes as plain fields. This is
the largest foundation — it touches the expander's core type — but it pays
three times: syntax objects become ordinary sealed values (macro templates,
closure syntax, and inline-fn syntax are just body data, demand-paged like
everything else); `send` gets real syntax support, fixing its
syntax-dropping defect at the root rather than at a boundary; and the
`SyntaxLiteral` case (post-expansion syntax embedding a value) becomes an
ordinary child `Value` instead of an unencodable special case.

The migration is whole, not boundary-only: the expander's working tree
moves too. Keeping a mutable Rust tree in the compiler and a region-native
form at the value boundary would mean two representations of one datum —
conversion seams, double maintenance, and a standing invitation for the two
to drift. The performance bar for the migration is **parity**: building and
mutating syntax trees in regions must be comparably fast to the Rust-heap
tree they replace, since expansion is compile-path-hot and the fallback
compile pays it too. Risk item 5 measured the bar and retired it: the
prototype region tree beats the Rust-heap tree on every expansion-hot
operation by 2–5×, so the boundary-only split is off the table.

Hygiene scope ids minted by the expander remain process-local counters; the
image records a scope watermark so a fresh expander mints above every scope
baked into persisted syntax.

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
the census (risk item 2): every collection's `traits` field points at one of
the instance's two default traitsets — `@struct`s built by
`init_default_traits` at VM init, before any stdlib load or hydration. They
are instance infrastructure, not program state, so the dumper never copies
them: a `traits` slot aimed at a default traitset becomes a reconstruction
entry whose constructor resolves the hydrating instance's own table for that
tag. The tables exist before hydration by construction (VM-init order), so
the constructor is a lookup, not an allocation.

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

- `FnInlineRegistry` — per-name HIR templates of cross-unit-inlineable
  stdlib functions; user code inlines through it.
- `DispatchWrapperRegistry` and the signal-projection memo.

An image boot that leaves these empty compiles user code *differently* from
a source boot — silently worse code, and divergent artifacts for anything
keyed on compiler output. That is not acceptable: the two boot modes must
produce identical user-code compiles.

With region-native syntax the fix is small: the manifest records each
inlineable function's *syntax* as a body value, and the registry re-derives
the HIR template lazily (expand + analyze of one small form) the first time
a user compile asks for that name. The parity test below is the acceptance
gate, and it belongs to the **boot** milestone, not a follow-up.

## File format

One image is one file (or one embedded blob):

| Section | Content |
|---------|---------|
| header | magic, format version, fingerprint, section offsets, page count |
| pages | the dumped region's pages, largest first: body bytes per page at 4 KiB-aligned file offsets, so the section is mappable. Descending size order makes the packed layout self-aligning: every earlier page's size is a multiple of every later page's, so each page's offset — in the file and in the mapped interval — is a multiple of its own size, satisfying the masked-header walk with no padding |
| page table | (size, object cursor, data cursor) per page, in placement order — rebuilds each page's cursors |
| relocations | pointer stream: (slot offset, target segment, target offset); primitive stream: (slot offset); reconstruction stream: (slot offset, constructor tag). Offsets are region-relative bytes: the hydrated region is one contiguous interval (Hydration step 3), so `base + offset` names any slot or target in O(1) and the (page, offset) pair collapses |
| object index | (offset, tag) per heap object, sorted — rebuilds `dtors`/`ref_objs` and drives the verifier |
| primitive table | primitive names in dump-time `prim_id` order |
| name table | symbol and keyword names in the body — hashes are stable, but the hydrating instance's display memos must learn them |
| signal table | user-defined signal names in dump-time bit order |
| watermarks | dump-time counters: parameter id, static-region mint, hygiene scope id, next signal bit |
| manifest | bindings: name, kind (function / macro / core), value location, signal, arity, doc location; macro entries add parameter lists, template-syntax and transformer-cache locations; inline-fn syntax locations; plus root locations and dependency fingerprints |
| side-stream | typed streams, present in any image: encoded `LirFunction`s keyed by template location (the boot configuration requires this stream — see *JIT*); `SendValue`-encoded mutable bindings (present only where the dump policy permits mutables) |

**Relocation slots.** A pointer slot is any 8-byte field holding an absolute
address: a heap-tagged `Value`'s payload, a `RegionSlice`'s ptr. A primitive
slot is a `Value` with `TAG_NATIVE_FN`, remapped by name — and a name
missing from the live registry is minted on the spot from its static def
(trait-method handlers are appended to the registry on first use, so a dump
can hold ids the fresh process has not minted yet). Symbol and keyword
payloads are stable hashes and need no relocation. The dumper emits each
entry as it copies the object — it knows every variant's layout, so there is
no post-hoc discovery, and targets are region-relative offsets so hydration
rewrites each slot in O(1) with no address search.

**Static region slots** baked into bytecode operands are opaque per-function
keys; they collide harmlessly across functions. The loader bumps the global
mint counter past the image watermark anyway, so uniqueness diagnostics stay
truthful. Parameter ids, hygiene scopes, and signal bits get the same
watermark treatment.

## Fingerprint: regenerate, never migrate

`HeapObject` is `repr(Rust)`; opcode discriminants and `prim_id`s are
source-order-dependent. An image is therefore valid only for a binary whose
layout agrees with the dumper's. The fingerprint records: format version,
rustc version and target triple, `size_of`/`align_of` for `Value`,
`HeapObject`, `RegionSlice`, `Closure`, `ClosureTemplate`, and `TableKey`, the
instruction-set high-water mark, `CURRENT_EPOCH`, the feature set, a hash of
the primitive name list in registration order, and (for the boot
configuration) the hashes of the three sources.

Size and align alone do not pin field offsets. The fingerprint therefore
also records the probed layout of every variant the dumper can emit: the
discriminant byte and each leaf field's offset and length (risk item 6
records the probe mechanism and the measured layout). A build whose layout
reorders a field or moves the discriminant fails the fingerprint instead of
hydrating garbage, so the two-stage embed build cannot pass with a shifted
layout. The same extents drive slot canonicalization (§ Dumping).

On mismatch the loader falls back — the `include_str!` sources never go away,
so the image is an optimization, not a correctness dependency. Images are
regenerated, not migrated; epochs stay a source-level concept. An environment
image is binary-locked: durable cross-version data belongs in files or RDF,
and the manifest is designed so a tool running the *old* binary can export
bindings as source. State that limit to users rather than promising
migration.

## Hydration

1. Validate the fingerprint; on failure, fall back (boot: compile sources;
   environment: report the mismatch).
2. Register the name table in the hydrating instance's symbol and keyword
   display memos.
   Replay the signal table so user signal bits land where the dump minted
   them. Check the primitive table against the live registry.
3. Mint a region and map the page section: reserve an aligned `PROT_NONE`
   range, then `MAP_FIXED` + `MAP_PRIVATE` each dumped page from the file
   into its slot. The section's 4 KiB file alignment makes the offsets
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
is written into an anonymous memory file (`memfd_create`; `shm_open` on
macOS) and hydrated from that descriptor without touching a filesystem. On
Linux the memfd is write-sealed (`F_SEAL_WRITE | F_SEAL_SHRINK`) before
mapping, so the immutability the mapping relies on is kernel-enforced.

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
and relocation slots stay zero, and each object slot receives only its
discriminant byte and the leaf-field extents the layout probes record
(§ Fingerprint). A `repr(Rust)` enum copy carries uninitialized padding
from its construction temporary — the store spike measured this residue —
so the dumper never copies a slot wholesale; the extent copy leaves padding
out, and two dumps of the same graph are byte-identical whole files. The
warm cache still keys on the fingerprint, not a content hash; whole-file
determinism buys reproducible embedded blobs, and concurrent dumpers racing
through the atomic rename produce identical files.

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
  4 KiB-aligned — an `include_bytes!` behind a `#[repr(align(4096))]`
  wrapper keeps its virtual address aligned, and load-segment congruence
  makes its file offset aligned too — so hydration maps it straight from
  the executable's own file, with the offset recovered from the static's
  address and the segment table (`dl_iterate_phdr`; the Mach-O load
  commands on macOS). Embedding changes the binary but not the fingerprint
  — every fingerprint input is computed from layout probes and sources, not
  from the binary hash — so the two-stage build converges in one iteration.

## Verifier

A debug verifier walks the object index after hydration and asserts sealing:
every tag is in the sealed set, every pointer slot targets this image or a
declared dependency, every `RegionSlice` is in bounds, and the rebuilt
cursors agree with the index. Format drift then fails loudly at load, not as
a torn read later.

## Open risks and dispatch experiments

The design rests on assumptions that are cheap to test and expensive to be
wrong about. Run these before the foundations land, in this order:

1. **Boot-time attribution — dispatched, value proposition confirmed.**
   `--trace=boot,compile` (landed with this design; pinned by
   `tests/integration/trace_boot.rs`) attributes a warm release-build boot
   of a trivial script (~545 ms total): stdlib frontend compile ~494 ms
   (91%), of which region inference ~216 ms, expand ~95 ms, analyze
   ~75 ms, emit ~74 ms, lower ~18 ms; core ~8 ms; prelude, registration,
   and meta build ~2 ms combined; stdlib *execute* ~1.4 ms; ~38 ms of
   process/VM setup and teardown remain. The image removes everything but
   that ~38 ms floor and the 1.4 ms execute — roughly a 10× boot. Region
   inference alone is 44% of the stdlib compile, an independent
   optimization target for the fallback path.
2. **Post-boot heap census — dispatched, sealing confirmed.**
   `--trace=census` (landed with this experiment; pinned by
   `tests/integration/census.rs`, whose sealing net fails the moment an
   unsealed variant enters the boot graph) walks every live object in the
   instance's region store after boot. A warm release boot leaves **405
   regions**, **630 objects**, 1.62 MiB of committed region pages, and
   ~455 KiB of estimated body payload:

   | Tag | Count | Bytes | Heap-ptr slots | Slices |
   |-----|-------|-------|----------------|--------|
   | ClosureTemplate | 202 | 317,383 | 24 | 0 |
   | Closure | 202 | 68,320 | 816 | 142 |
   | CaptureCell | 210 | 63,840 | 205 | 0 |
   | LStruct | 4 | 12,490 | 197 | 0 |
   | Parameter | 7 | 2,016 | 3 | 0 |
   | External | 3 | 864 | 0 | 0 |
   | LStructMut | 2 | 748 | 3 | 0 |

   The boot residue is code, not data: no pair, string, or array survives
   to the post-boot heap. The 210 capture cells are the snapping set — the
   static scan found zero top-level `assign`s in stdlib.lisp, so every
   cell snaps. The unsealed leaves are exactly the reconstruction stream's
   two classes: the three stdio-port `External`s and the two default
   traitsets (§ "Process-owned resources reconstruct in place"); nothing
   else in the graph is refused, so the boot image is dumpable. Relocation
   load: 1,248 heap-pointer slots + 142 `RegionSlice` ptrs + 682
   primitive slots ≈ 2,100 relocation entries, ~2.7 heap-pointer slots
   per KiB of payload. `shared-templates 0`: every boot closure already
   references a region-resident template object, so the template
   foundation rewrites representation, not sharing structure.
3. **The store spike — dispatched, mechanism proven.** `src/image` dumps
   and hydrates data-only graphs (pairs, strings, bytes, arrays, floats,
   portable immediates) end to end, pinned by `tests/integration/image.rs`
   against the § Test plan: round-trip equality in a fresh heap, sharing
   preservation, corrupted-fingerprint fallback with no leaked region or
   mapping, double hydration with independent address sets, rename-over-a-
   live-mapping, teardown to baseline, and file-backed release as `munmap`
   (never cached — also pinned at the pool in `pagepool/tests.rs`). The
   pool interplay reduced to one field: `MmapPage` carries a `file_backed`
   flag the release path checks. Two findings: relocation slots and targets
   collapse to region-relative offsets (recorded in § File format), and
   raw object-slot bytes are not byte-deterministic under `repr(Rust)`
   padding (recorded in § Dumping; resolved by risk item 6's extent
   copy). Still
   open for the full **store** milestone: scrub/guardfree exercised over a
   hydrated region, the `(fd, offset)` input form (memfd for byte
   sources), and the always-on verifier's pointer-bounds walk beyond the
   tag check.
4. **Symbol-identity scout — dispatched, migration confirmed cheap.** The
   audit classified all 221 `SymbolId` sites, and a throwaway prototype —
   `SymbolId(u64)` minted as the FNV-1a name hash, `SymbolTable` reduced
   to a hash→name registry with the keyword collision panic — ran the full
   suites. Site classes and the cost of each:

   | Site class | Sites | Migration cost |
   |------------|-------|----------------|
   | Opaque keys and pass-throughs (`PrimitiveMeta`, classification maps, inline/dispatch registries, JIT `scc_peers`, binding arenas) | ~170 | none — already hash-keyed |
   | Width seams: `SymbolId(u32)`; `Value::symbol(u32)`; the truncating `as_symbol() → u32`; `Bytecode::add_symbol(u32)`; `SendValue::Symbol.id` (dead on receive); `errors.rs` parses `SymbolId(N)` as `u32`; 66 `HashMap<u32, String>` name maps | ~75 | mechanical widening — the whole prototype is 39 files, ±110 lines, and compiles clean beyond these seams |
   | Dense indexing beyond `SymbolTable` | 1 | `jit/group.rs` `globals[sym.0 as usize]` — dead code with test-only callers; there is no VM globals table (the letrec model has no `LoadGlobal`), so no live density assumption exists |
   | Bytecode operands carrying a symbol id | 0 | none to audit: symbols reach bytecode only as constant-pool `Value`s (u16 pool index) and `ConstTemplate`s, which already encode symbols by name |
   | Raw-id comparators (`Value::Ord` rank-3 arm, `TableKey::Ord` symbol arm) | 2 | sort order flips to hash order coherently; sorted structs, sets, and their binary searches stay correct because build and probe share the comparator |
   | Sentinel `SYNTHETIC = u32::MAX` | 1 production read | becomes a reserved `u64::MAX`; the binding's existing `is_synthetic` flag could replace it outright |

   Measured fallout: Rust suites green except two tests that pin the
   property being removed (the sequential-mint assertion, and a
   different-ids-across-two-tables setup assert). The full smoke corpus —
   2,264 files, VM and JIT — passes with **zero expectation churn**: no
   Elle test observes symbol sort or print order. `(environment)` is the
   one producer of symbol-keyed structs, and nothing pins its key order.
   Keyword-keyed containers sort by name string and are unaffected. Boot
   is unharmed: quiet-machine stdlib-compile is not slower under hash
   interning. Deleted by the migration: the five `symbol_names` maps and
   their threading, `all_names()`, `send`'s by-name symbol re-intern,
   `intern_primitive_names` and its five call sites, and the `CompileCtx`
   registration-order invariant (including the docs/pipeline.md bullet).
   The audit also found two live cross-table id holes that stable ids
   close: `send` ships `LirConst::Symbol` inside the live `LirFunction`
   verbatim, so worker-side JIT re-emission pools sender-space ids; and
   `TableKey::Symbol` keys inside sent structs cross untranslated. The
   symbol milestone must land regression tests for both.
5. **Expander mutation parity — dispatched, parity exceeded.** A throwaway
   prototype node ran the expander's hot operations head to head against
   the Rust-heap tree, allocating through the real region store
   (`FiberHeap::alloc_region_slice_in_region`). The node is 56 bytes to
   `Syntax`'s 112: kind tag, packed span over an interned file id, scope
   set inline (capacity 4 plus an overflow slice), string payloads and
   child slices as `RegionSlice`, symbols as stable hashes (the symbol
   foundation lands first). Corpus: the parsed prelude + stdlib trees
   (230 forms, 13,332 nodes), 20 rounds per op, in-place walks mutating
   uniquely owned trees through the child slices:

   | Per node | Rust-heap tree | Region tree |
   |----------|----------------|-------------|
   | stamp-copy (macro-arg clone + add-scope walk) | 176 ns | 37 ns |
   | hygiene flip walk, in place | 40 ns | 17 ns |
   | file-scope add walk, in place | 31 ns | 13 ns |
   | build + drop (the `from_value` shape) | 73 ns | 17 ns |
   | teardown | 31 ns | 7 ns |

   Region mint + free measures ~6 ns, so per-expansion transient regions
   are noise. The no-inline-capacity fallback — regrow the scope slice on
   every add — costs 8 ns per visit, so scope storage is not a parity risk
   in either form. The op mix is measured, not assumed: counters on
   `Syntax::clone`, the constructors, `map_scope_recursive`, and the
   converters (dumped at the `--trace=compile` expand/analyze marks)
   showed the stdlib expand phase (84.5 ms warm release) deep-clones
   **464,089** nodes against 46,746 built and 24,718 from `from_value`,
   across 1,532 expansions; analysis clones another 143,298. A 300-defn
   macro-heavy user file amplified the same shape: 992,170 clones in a
   206 ms expand, ~254 clones per expansion — a large share being the
   per-call `MacroDef` template deep clone, which pointer-shared immutable
   region trees delete outright. perf agrees: `Syntax::clone` +
   `drop_in_place<Syntax>` is ~10% of the whole boot. Scope sets are tiny
   everywhere: every one of the ~430k measured scope ops ended with ≤3
   scopes. Counts × per-op deltas put tree ops near half of an
   expansion-heavy expand phase and ~4× cheaper in regions, so the
   migration is projected to make expansion-heavy compiles roughly a
   third *faster* — and even a fully immutable working tree meets the
   bar, since a full stamp-copy (37 ns) undercuts the Rust tree's
   in-place walk (40 ns). The boundary-only split is dead; no measured
   deal-breaker exists. One condition binds the migration: keep in-place
   mutation legal on uniquely owned working trees (stamped copies and
   conversion results — the ownership discipline the hygiene flip already
   relies on). To redo: counter patch at the sites above, plus a
   `#[cfg(test)]` bench under `src/syntax/expand/` building the prototype
   node from `read_syntax_all` output and running stamp/flip/add/build/
   teardown against `stamp_scope`/`flip_scope_recursive`.
6. **Fingerprint strength — dispatched, probes landed.** Size/align probes
   do not pin field offsets; the fingerprint now records, per dumpable
   variant, the discriminant byte and every leaf field's offset and length
   (`src/image/layout.rs`), so the two-stage embed build cannot pass the
   fingerprint with a shifted layout. Mechanism finding: `offset_of!`
   cannot name an enum variant's field on stable Rust (E0658,
   rust-lang/rust#120141), so the probes construct one exemplar per variant
   and measure each field's address against the object's base, with
   `offset_of!` covering the nested structs (`Pair`, `Value`,
   `RegionSlice`). Measured layout (rustc 1.95, x86-64): `HeapObject` is
   288 bytes, align 8 — the by-value `ClosureTemplate` variant sets the
   size, so a `Float` slot is ~95% padding until the template foundation
   shrinks it. The discriminant is one byte at offset 0 (declaration index
   plus 3) with bytes 1–7 zero; every probed variant places its payload
   field at 8 and `traits` at 24 (`Pair` nests its whole struct at 8).
   `RegionSlice`'s `u32` len leaves interior padding at bytes 12–16 of the
   field, which is why extents are recorded per leaf field, never per
   variant field. The probes verify themselves on first use — distinct
   discriminant bytes, zero upper discriminant bytes, disjoint in-bounds
   extents, and a canonicalize-then-read-back check per variant — and
   panic on violation, so a rustc that moves the tag or reorders fields
   fails loudly before any image is written or trusted. The unlock
   (§ Dumping): the dumper assembles object slots from the extents, dumps
   are byte-identical whole files, and the determinism pin asserts
   whole-file equality with a poisoned-padding counter-factual.

## Landing order

Foundations first — each lands green on the existing corpus with no image
code, and each deletes image machinery:

1. **symbol** — stable content-addressed symbol identity; deletes the
   symbol remap pass, the sorted-container re-sort hazard, `symbol_names`
   maps, and `send`'s re-interning.
2. **struct** — region-native immutable struct payloads.
3. **template** — the flattened, region-resident `ClosureTemplate`;
   `MakeClosure` references instead of clones.
4. **syntax** — the region-native syntax tree; deletes the syntax codec and
   fixes `send`'s syntax defect at the root.

Then the image milestones:

5. **store** — the file-backed page flag in the pool, then dumper/hydrator
   for data-only graphs (no closures), the object-index rebuild, and the
   fingerprint fallback. Proves the format, mapping, relocation, and
   teardown end to end. The mechanism here is independent of the
   foundations — pairs, strings, and arrays are already sealed — so a
   deliberately small spike of this milestone may run in parallel with them
   to retire the mapping and pool-interplay risk early; it must not grow
   compensating machinery (remap passes, codecs) that the foundations
   delete.
6. **boot** — cell snapping, dump-boot, warm cache, embedded blob,
   per-worker hydration for `sys/spawn`, the encoded-LIR side-stream with
   lazy decode, compiler-state persistence, and the parity gate (bytecode
   *and* tier).
7. **environment** — `image/save` and `image/load`, manifest deltas over
   boot, mutable side-stream.

## Test plan

- Foundations: existing corpus plus targeted unit tests pinning the new
  layouts, the no-clone `MakeClosure`, stable symbol ordering across two
  tables, and syntax round-trips through `send`.
- Round-trip: dump a data graph, hydrate in a fresh runtime, assert
  structural equality — and a counter-factual load with a corrupted
  fingerprint falls back cleanly.
- Hygiene: hydrate, run, exit — the live region count returns to baseline
  and the leak suite stays green with no image-specific carve-out. Free the
  hydrated region explicitly under `--trace=guardfree` and assert the
  cascade releases its cross-image edges exactly once.
- Relocation: hydrate the same image twice in one process (two regions, two
  address sets) and assert both hydrations are correct and independent.
- Mapping: replace the image file by rename while a hydration is live, then
  read the hydrated values — the old inode's mapping is intact. Release a
  file-backed page and assert the pool unmapped it rather than caching it.
  Run scrub and guardfree over a hydrated region.
- Snapping: boot from image, run the full smoke corpus — behavior identical
  to source boot. A stdlib top-level that is `assign`ed must fail the dump
  with a named error.
- Compile parity: compile the same user file under image boot and source
  boot and assert byte-identical bytecode — the acceptance gate for the
  persisted compiler state (inline templates, dispatch wrappers).
- Tier parity: a hot stdlib function reaches the JIT under image boot
  exactly as under source boot — the lazy LIR decode feeds `submit_jit_task`
  and the compiled result executes.
- Names: print an image keyword and an image symbol (the instance's display
  memos learned them) and raise an image-defined signal (the replayed bit
  matches the baked profile).
- Macros: a macro whose transformer cache was empty at dump expands
  correctly after hydration (the lazy fill still works).
- Parameters: after image boot, `println` writes to the process's real
  stdout (the reconstructed default, not a stale dump-time resource), and
  `parameterize` of `*stdout*` redirects it — the captured `Parameter`
  identity and the fiber's frame lookup both survived hydration.
- Determinism: dump the same graph twice and assert byte-identical whole
  files. The counter-factual: scribble a pattern into a live object's
  padding bytes before the dump and assert the file does not change — a
  wholesale slot copy would carry the pattern into the artifact.
- Resolution: a unit test pins that an image pointer resolves through the
  interval table without touching the header ladder, and that `owns` on a
  hydrated region is a range check.
