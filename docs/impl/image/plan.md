# Landing order and test plan

<!-- audited: 2026-10-22 -->

What lands in which order, and the pins each milestone must land with.

[The image design](../image.md) owns the argument,
[the foundations](foundations.md) the six representation fixes below,
[booting from an image](boot.md) the boot configuration, and
[the measurements](measurements.md) the experiments that answered the design's
open questions. [pins.md](pins.md) holds the boot milestone's pins.

## Landing order

Foundations first. Each lands green on the existing corpus with no image code,
and each deletes image machinery ([the foundations](foundations.md) argue all
six). Five landed before any image code:

1. **symbol** — landed. Stable content-addressed symbol identity
   ([symbols](../symbol.md)).
2. **struct** — landed. Region-native immutable struct payloads and keys
   ([values](../values.md)).
3. **template** — landed. The blueprint / payload / header split
   ([code objects](../region/template.md)).
4. **syntax** — landed. The region-native syntax tree
   ([the syntax tree](../syntax.md)).
5. **lir** — landed. Region-native LIR ([lir.md](../lir.md)), in four steps,
   each green on its own. **lir-view** froze every function into one read
   form, and **lir-payload** put the frozen function in the code payload.
   **lir-retire** made a code object one payload slice, and **lir-lower** built
   the records in a working region.

Then the image milestones:

6. **store** — landed. The format, mapping, relocation and teardown, proven end
   to end over every value the foundations sealed as data
   ([format.md](format.md)).
7. **boot** — landed. The boot configuration ([boot.md](boot.md)), with
   closures, primitives, traitsets and parameters in the body, and the warm
   cache. It also brought parity with a source boot in tiers, grafts and
   bytecode, the interval table ([hydration](hydration.md)), per-worker
   hydration, and the embedded blob ([blob.md](blob.md)).
8. **mutable** — the sixth foundation: region-native mutable stores
   ([mutable.md](../mutable.md)). It lands before environment, so a mutable
   binding is body data before any dump writes one. [The send plan](../send/plan.md)
   orders its steps and holds its pins. The seam, the byte store, the
   `@array` store, the cell store and the `@set` store have landed.
9. **environment** — `image/save` and `image/load`, manifest deltas over
   boot, and `&allow-mutable` as a switch in the dump policy.

## Test plan

- Foundations: existing corpus plus targeted unit tests pinning the new
  layouts, the no-clone `MakeClosure`, stable symbol ordering across two
  tables, and syntax round-trips through `send`.
- LIR: every corpus file and the standard library emit byte-identical
  bytecode through each step, which is the foundation's acceptance gate. A
  representation change that moves one instruction is a defect, and the golden
  is what the binary emitted before the step. `make bytecode-golden` records
  it under `target/` and `make bytecode-golden-check` compares, and
  `--dump=bytecode` writing one text across two runs is what lets a golden
  exist at all.
- Tier parity: a hot stdlib function reaches the JIT under image boot
  exactly as under source boot — the hydrated payload's own LIR feeds
  `submit_jit_task` and the compiled result executes.
- Round-trip: dump a data graph, hydrate in a fresh runtime, assert
  structural equality — and a counter-factual load with a corrupted
  fingerprint falls back cleanly.
- Sorted containers: a hydrated set and a hydrated struct answer the
  membership and lookup questions their sources did. The keys span both
  orders — a symbol and a keyword rank by hash, a string and an array by
  content. The counter-factual for the entry canonicalization is the
  determinism pin: a struct whose key padding differs between two dumps must
  still write one file.
- Syntax: a tree round-trips with its structure, its spans, its scope sets,
  and its scope-exempt flags intact. The file table decides a hydrated
  span's file: rename the spelling in the table, and every span follows it. A
  synthetic span still names no file. A scope id hydrates unchanged, its unit
  tag and its intro bit included.
- Source: an image parked at a non-zero, base-page-aligned offset inside a
  larger file hydrates from that descriptor and offset. A misaligned offset
  is refused by name, before anything is mapped.
- Bytes: an image that arrives as bytes hydrates through an anonymous memory
  file, with no filesystem path anywhere in the path. On Linux the seal holds:
  a write to that descriptor after hydration fails.
- Verifier: four defects each fail the load with a named error and leave no
  region and no mapping behind. The four are a relocation slot outside the
  image, a relocation slot that is not 8-byte aligned, a `RegionSlice` whose
  extent leaves the image, and a page cursor that disagrees with the object
  index. A closure header is refused the same way four ways. They are a header
  naming zero payloads, a payload landing misaligned, a payload field whose
  extent leaves the image, and a child slot naming an object the index does not
  call a header. The child slot is the one slot whose target is read back as a
  header rather than as data.
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
- Names: a fresh instance prints an image's symbol and its keyword by name,
  having met neither spelling before. Two dumps of one graph write one name
  table, whatever order the dumping memo learned the spellings in. A
  spelling the dumping instance never learned hydrates as an equal value that
  still prints as `#<keyword:hash>`. Under boot, an image-defined signal
  raises with the replayed bit matching the baked profile.
- Determinism: dump the same graph twice and assert byte-identical whole
  files. The counter-factual: scribble a pattern into a live object's
  padding bytes before the dump and assert the file does not change — a
  wholesale slot copy would carry the pattern into the artifact.
