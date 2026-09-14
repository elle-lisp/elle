# Landing order and test plan

<!-- audited: 2026-09-14 -->

What lands in which order, and the pins each milestone must land with.

[image.md](../image.md) owns the design, [foundations.md](foundations.md) the
four representation fixes below, and [measurements.md](measurements.md) the
experiments that dispatched the design's open risks.

## Landing order

Foundations first — each lands green on the existing corpus with no image
code, and each deletes image machinery
([foundations.md](foundations.md) argues all four):

1. **symbol** — landed. Stable content-addressed symbol identity
   ([symbol.md](../symbol.md)); deleted the symbol remap pass, the
   sorted-container re-sort hazard, the `symbol_names` maps, and `send`'s
   re-interning.
2. **struct** — landed. Region-native immutable struct payloads and keys
   ([values.md](../values.md) § "Struct keys"); deleted the per-probe key
   allocation and the owned-key arms the dumper would have had to encode.
3. **template** — landed. The blueprint / payload / header split
   ([region/template.md](../region/template.md)); deleted the per-creation
   blueprint clone, the `HashMap` location map, and the `Rc`-shared template
   variant that no image could dump.
4. **syntax** — landed. The region-native syntax tree
   ([syntax.md](../syntax.md)); deleted the side-stream syntax codec this
   design would otherwise have needed, the `Box<Syntax>` inside
   `HeapObject::Syntax`, and the retained lambda tree on every closure
   template.

Then the image milestones:

5. **store** — landed. The file-backed page flag in the pool, the dumper and
   hydrator, the object-index rebuild, the fingerprint fallback, the
   `(fd, offset)` input form with an anonymous memory file for an image that
   arrives as bytes, the verifier's two passes, the name table that teaches a
   hydrating instance the spellings its symbols and keywords carry, the
   sorted containers, whose entries the dumper assembles from probed extents
   exactly as it does an object's, syntax, with the file table its spans need
   and the scope watermark a fresh expander must mint above, and the scrub
   and guardfree pins over a hydrated region. The format, mapping,
   relocation, and teardown are proven end to end over every value the
   foundations sealed as data.
6. **boot** — in progress. Landed so far: the primitive table that remaps a
   native-fn by name and the user traitsets it makes dumpable, the
   reconstruction stream — which the default trait tables need before
   `Parameter`'s stdio default does — and closures and closure templates in
   the body: the payload crosses whole and shared, the header hydrates
   without its blueprint, and the code objects a `MakeClosure` indexes cross
   as the payload's child table ([sealing.md](sealing.md) § "A child code
   object crosses as a header"). Still to land: cell snapping,
   dump-boot, warm cache, embedded blob, per-worker hydration for
   `sys/spawn`, the encoded-LIR side-stream with lazy decode, compiler-state
   persistence, the hydrated-region interval table that keeps
   `region_of_ptr` off the probe ladder ([image.md](../image.md) § "Pointer
   resolution must not regress" — the regression it prevents needs a region
   the size of stdlib to show), and the parity gate (bytecode *and* tier).
7. **environment** — `image/save` and `image/load`, manifest deltas over
   boot, mutable side-stream.

## Test plan

- Foundations: existing corpus plus targeted unit tests pinning the new
  layouts, the no-clone `MakeClosure`, stable symbol ordering across two
  tables, and syntax round-trips through `send`.
- Round-trip: dump a data graph, hydrate in a fresh runtime, assert
  structural equality — and a counter-factual load with a corrupted
  fingerprint falls back cleanly.
- Sorted containers: a hydrated set and a hydrated struct answer the
  membership and lookup questions their sources did, over keys that rank by
  hash (a symbol, a keyword) and keys that rank by content (a string, an
  array). The counter-factual for the entry canonicalization is the
  determinism pin: a struct whose key padding differs between two dumps must
  still write one file.
- Syntax: a tree round-trips with its structure, its spans, its scope sets,
  and its scope-exempt flags intact. The file table is what decides a
  hydrated span's file: rename the spelling in the table, and every span
  follows it. A synthetic span still names no file. The scope watermark
  exceeds every counter value the body carries, intro scopes included.
- Source: an image parked at a non-zero, base-page-aligned offset inside a
  larger file hydrates from that descriptor and offset. A misaligned offset
  is refused by name, before anything is mapped.
- Bytes: an image that arrives as bytes hydrates through an anonymous memory
  file, with no filesystem path anywhere in the path. On Linux the seal holds:
  a write to that descriptor after hydration fails.
- Verifier: each of a relocation slot outside the image, a relocation slot
  that is not 8-byte aligned, a `RegionSlice` whose extent leaves the image,
  and a page cursor that disagrees with the object index fails the load with
  a named error and leaves no region and no mapping behind. A closure header
  is refused the same way five ways: a nonzero blueprint word (the one bit
  pattern teardown could hurt on — a fabricated `Rc`), a header naming zero
  payloads, a payload landing misaligned, a payload field whose extent
  leaves the image, and a child slot naming an object the index does not call
  a header — the one slot whose target is read back as a header rather than
  as data.
- Hygiene: hydrate, run, exit — the live region count returns to baseline
  and the leak suite stays green with no image-specific carve-out. Free the
  hydrated region explicitly under `--trace=guardfree` and assert the
  cascade releases its cross-image edges exactly once.
- Diagnostics: under `--trace=scrub`, a program that hydrates an image
  answers exactly as it does without the flag, and the freed pages are
  unmapped rather than blanked into the cache. Under `--trace=guardfree`, a
  freed hydrated page stays mapped and inaccessible, and the free log
  records its range so a fault can be attributed.
- Relocation: hydrate the same image twice in one process (two regions, two
  address sets) and assert both hydrations are correct and independent.
- Mapping: replace the image file by rename while a hydration is live, then
  read the hydrated values — the old inode's mapping is intact. Release a
  file-backed page and assert the pool unmapped it rather than caching it.
- Closures: a closure compiled in one runtime hydrates in a fresh one and
  answers a call with the same result — through a REPL binding, so the call
  goes through the ordinary dispatch path. The payload survives field by
  field: bytecode, constants (a heap constant included), arity, signal,
  name, doc, and the capture masks. Two headers materialized from one
  blueprint hydrate naming one payload copy — the counter-factual is a
  per-header deep copy, which round-trips equal and silently doubles every
  payload. A hydrated header has no blueprint: `meta/origin` answers nil
  and the JIT is never entered. A WASM-dispatch closure and an env holding a
  capture cell each refuse the dump with a named error. A dumped closure
  writes one file across two
  dumps, whatever its construction temporaries held. The relocation stream
  records a shared payload's slots once, however many headers name it — the
  counter-factual is a per-header walk, which appends every inner entry
  again for each header and grows the tables with the header count.
- Children: a hydrated closure builds its nested lambda, and the lambda
  answers a call — through a REPL binding, so `MakeClosure` runs on the
  ordinary dispatch path. The child's payload crosses field by field, and a
  lambda nested two deep builds out of the child's own child table. The
  instruction materializes a fresh header per creation: two lambdas built
  from one hydrated parent are two headers over one payload, and the
  counter-factual is handing out the image's own header, which answers every
  call correctly and quietly moves the instance-to-template edge across
  regions. A child a WASM module built refuses the dump like any other WASM
  closure, and a parent with a child writes one file across two dumps. A
  hydrated closure sent to a worker carries its children, which the worker
  rebuilds as the blueprints its own `MakeClosure` indexes.
- Traits: a value carrying its instance's default traitset hydrates carrying
  the *hydrating* instance's table for that tag, and a user traitset hydrates
  out of the body with its methods intact. The counter-factual is the identity
  check: a default traitset copied into the body would hydrate as a table equal
  to the instance's own and distinct from it, which only a pointer comparison
  against `default_traits_for` can see. Freeing the hydrated region releases the
  trait table's region exactly once, and the free-time edge oracle agrees with
  the recorded table.
- Primitives: a native-fn in the body answers to the live registry's id for its
  name, in an instance that minted its ids differently. A def the canonical
  tables do not name fails the dump by name, and two dumps write one file
  whatever ids the dumping process handed out.
- Snapping: boot from image, run the full smoke corpus — behavior identical
  to source boot. A stdlib top-level that is `assign`ed must fail the dump
  with a named error.
- Compile parity: compile the same user file under image boot and source
  boot and assert byte-identical bytecode — the acceptance gate for the
  persisted compiler state (inline fragments, dispatch wrappers).
- Tier parity: a hot stdlib function reaches the JIT under image boot
  exactly as under source boot — the lazy LIR decode feeds `submit_jit_task`
  and the compiled result executes.
- Names: a fresh instance prints an image's symbol and its keyword by name,
  having met neither spelling before, and two dumps of one graph write one
  name table whatever order the dumping memo learned the spellings in. A
  spelling the dumping instance never learned hydrates as an equal value that
  still prints as `#<keyword:hash>`. Under boot, an image-defined signal
  raises with the replayed bit matching the baked profile.
- Macros: a macro whose transformer cache was empty at dump expands
  correctly after hydration (the lazy fill still works).
- Parameters: a parameter's id crosses unchanged, and a fresh instance mints
  above the image's watermark — the counter-factual is a hydrated body whose
  next new parameter would otherwise repeat `*stdin*`'s id. A standard-stream
  default hydrates as a port this instance opened, in a region of its own
  rather than out of the image's pages, and freeing the hydrated region
  releases that region exactly once. A file port refuses the dump by name, and
  the artifact records no port address whatever the dumping process held. After
  image boot, `println` writes to the process's real stdout (the reconstructed
  default, not a stale dump-time resource), and `parameterize` of `*stdout*`
  redirects it — the captured `Parameter` identity and the fiber's frame lookup
  both survived hydration.
- Determinism: dump the same graph twice and assert byte-identical whole
  files. The counter-factual: scribble a pattern into a live object's
  padding bytes before the dump and assert the file does not change — a
  wholesale slot copy would carry the pattern into the artifact.
- Resolution: a unit test pins that an image pointer resolves through the
  interval table without touching the header ladder, and that `owns` on a
  hydrated region is a range check.
