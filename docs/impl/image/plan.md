# Landing order and test plan

<!-- audited: 2026-09-08 -->

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

5. **store** — landed. The file-backed page flag in the pool, dumper and
   hydrator for data-only graphs (no closures), the object-index rebuild,
   the fingerprint fallback, the `(fd, offset)` input form with an anonymous
   memory file for an image that arrives as bytes, the verifier's two
   passes, the name table that teaches a hydrating instance the spellings its
   symbols and keywords carry, and the scrub and guardfree pins over a
   hydrated region. The format, mapping, relocation, and teardown are proven
   end to end. What it does not carry is the rest of the sealed set the
   foundations opened up: structs, sets, and syntax are body data now, and
   the dumper still refuses them.
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
- Source: an image parked at a non-zero, base-page-aligned offset inside a
  larger file hydrates from that descriptor and offset. A misaligned offset
  is refused by name, before anything is mapped.
- Bytes: an image that arrives as bytes hydrates through an anonymous memory
  file, with no filesystem path anywhere in the path. On Linux the seal holds:
  a write to that descriptor after hydration fails.
- Verifier: each of a relocation slot outside the image, a relocation slot
  that is not 8-byte aligned, a `RegionSlice` whose extent leaves the image,
  and a page cursor that disagrees with the object index fails the load with
  a named error and leaves no region and no mapping behind.
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
