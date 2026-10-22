# Images — regions hydrated at load

<!-- audited: 2026-10-21 -->

Design for image-style persistence: one mechanism, and the two configurations
it serves.

The two are the **boot** image (core, prelude, and stdlib pre-compiled into the
binary) and **environment** images (user `save`/`load`). Both use the same
format, dumper, and hydrator, and differ only in dependency list and dump
policy (§ One mechanism, two configurations). This document owns the design
argument and the alternatives it beat. The companions carry the rest:

| Document | Owns |
|----------|------|
| [foundations](image/foundations.md) | the five representation fixes the image rests on |
| [dumping](image/dumping.md) | the compacting copy, its determinism, and the mutable-binding policy |
| [sealing](image/sealing.md) | what the body may hold, what the hydrating instance rebuilds, what fails the dump |
| [the image file](image/format.md) | the file's sections, and the fingerprint that gates a load |
| [hydration](image/hydration.md) | mapping, relocation, the descriptor, pointer resolution, the clean set |
| [the verifier](image/verifier.md) | the two passes that refuse a damaged image by name |
| [parity](image/parity.md) | the compiler state and the LIR a hydrated closure keeps |
| [booting](image/boot.md) | the boot configuration, its digest, the warm cache, and how a worker boots |
| [the blob](image/blob.md) | the two-stage release build that embeds a boot image |
| [the plan](image/plan.md) | the landing order, and the pins each milestone lands with |
| [the boot pins](image/pins.md) | the pins the boot milestone landed with |
| [the measurements](image/measurements.md) | the experiments that answered the design's open questions |

This document states the design, not its progress; [the plan](image/plan.md)
records what has landed. The boot configuration has, and the environment
configuration has not.

## The problem

Without an image, every process start compiles core.lisp, prelude.lisp, and
stdlib.lisp through the full pipeline: read, expand, letrec analysis,
regularize, type inference, region inference, lower, emit, execute. Every
`sys/spawn` worker repeats it on its own heap, and so does the WASM entry.

A REPL or embedding session that builds up an environment loses it on exit.
The only durable stores are files and RDF; there is no way to save compiled
state.

## The idea: an image is a region, dumped and hydrated

An image is the page-level bytes of one real region — a compacted, sealed
copy of a value graph — plus a relocation table, name tables, and a manifest.
*Hydration* (loading) maps the image's pages privately into an aligned
reservation, rewrites each pointer slot in one linear pass, and installs the
result as a freshly minted region ([hydration](image/hydration.md)).

No value is deserialized, nothing allocates per object, and untouched pages
are not even read. The cost is O(relocations) + O(objects) pointer pushes +
O(bindings), and the bulk of the bytes fault in lazily on first use: page
speed, not serde speed.

The hydrated region is not a new kind of memory. It is a `Counted` region
like any other: pinned for the process's life by the same process-root
registration that pins stdlib's exports, released by
`teardown_process_root_regions` at exit, and freed by the ordinary cascade.
The [lifetime guarantee](../regions/lifetime.md) — after the program ends,
everything frees — holds without amendment. Leak accounting, the generation
checks, `--trace=scrub`, `--trace=guardfree`, and `arena/dump` all see
ordinary pages in an ordinary region.

Compaction is where the runtime win comes from. A source boot produces
hundreds of regions, one per letrec capture cell plus everything they pin. The
image is **one** region: every internal reference is a self-edge, so its edge
tables are empty, and all RC traffic against stdlib values lands on a single
counter.

## One mechanism, two configurations

"Boot image" and "environment image" are not two kinds of image. There is
one format, one dumper, one hydrator, one verifier. Every use of either
name means a *configuration* of that one mechanism; an image differs from
another only in its **dependency list** and its **dump policy**:

- Every image declares the fingerprints of the images beneath it. A dump
  walks its roots and stops at any value a dependency layer already
  provides, emitting a cross-image reference instead of a copy — the
  visited map is seeded with the dependency regions' address intervals. The
  root set of a dump is therefore always the same thing: *the bindings the
  layers below do not already provide*.
- The **boot** configuration is the image with an empty dependency list. Its
  root is one struct over the core exports, the stdlib exports and the macro
  definitions ([booting](image/boot.md) owns the shape). Its policy is strict:
  no mutable bindings, in body or side-stream. The warm cache or the embedded
  blob distributes it, and it is regenerated from sources whenever the
  fingerprint or the source digest misses.
- The **environment** configuration depends on a boot image. Its roots are
  the session's bindings beyond that layer, and its policy is user-facing
  (refuse mutables by default, `&allow-mutable` opt-in;
  [dumping](image/dumping.md) owns the fork). It is a file the user saves and
  loads explicitly: user data, not a regenerable artifact, and locked to the
  binary that dumped it (§ The binary lock is the current configuration).

Nothing limits the stack to two layers; an image may depend on any hydrated
stack whose fingerprints it names. Boot-over-nothing and
environment-over-boot are simply the two depths this design ships.

## The binary lock is the current configuration

An image is a region snapshot, and regions are in the language specification
([spec](../spec.md)). The fingerprint locks an image to the binary layout that
dumped it ([the image file](image/format.md)). That lock solves this
implementation's portability problem, and it is the configuration the format
ships today. It is not a property of an image.

A snapshot becomes portable once the code layer is in the specification: a
specified instruction set, a validator, and a rule for which compiled code
keeps loading. Until then an image hydrates only into a binary whose layout
agrees with the dumper's, and every other binary falls back
([hydration](image/hydration.md)).

The HIR fragments an image carries ([parity](image/parity.md)) are optional
data to any other implementation. One that ignores them grafts and fuses
nothing across units, and every call stays a call. This implementation still
requires them, because its two boot modes must compile user code identically.

## Rejected alternatives

- **CAS cache over a serialized code object** (a byte codec on the `send`
  types). Load re-allocates every object, re-interns every symbol, and
  rebuilds every `Rc`: O(objects) allocator and decode work on every start.
  This design keeps the *keying* — the warm cache ([booting](image/boot.md)) —
  and decodes no value.
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
  and every source provides a mappable descriptor
  ([hydration](image/hydration.md)). One implementation, kernel-enforced
  immutability, and a clean set that grows as the layout improves beat a
  second code path, even where relocation dirties enough frames that mapping
  costs what a copy would.
- **A constant-pool region owned by the code object** is rejected for
  ordinary constants ([the region model](region/model.md)); images do not
  change constant materialization. `MaterializeConst` keeps building fresh
  values per execution — image templates carry the same encoded
  `ConstTemplate` bytes, which are already name-stable.
