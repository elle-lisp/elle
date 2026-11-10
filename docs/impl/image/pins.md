# The boot milestone's pins

<!-- audited: 2026-10-21 -->

The pins the boot milestone landed with, one entry for each subject it brought
into an image.

[plan.md](plan.md) owns the landing order and the pins of the foundations and
the store. [boot.md](boot.md) and [blob.md](blob.md) own the behavior these
pins hold.

- Primitives: a native-fn in the body answers to the live registry's id for its
  name, in an instance that minted its ids differently. A def the canonical
  tables do not name fails the dump by name, and two dumps write one file
  whatever ids the dumping process handed out.
- Traits: a value carrying its instance's default traitset hydrates carrying
  the *hydrating* instance's table for that tag, and a user traitset hydrates
  out of the body with its methods intact. The counter-factual is the identity
  check: a default traitset copied into the body would hydrate as a table equal
  to the instance's own and distinct from it. Only a pointer comparison
  against `default_traits_for` can see that. Freeing the hydrated region
  releases the trait table's region exactly once, and the free-time edge oracle
  agrees with the recorded table.
- Parameters: a parameter's id crosses unchanged, and a fresh instance mints
  above the image's watermark — the counter-factual is a hydrated body whose
  next new parameter would otherwise repeat `*stdin*`'s id. A standard-stream
  default hydrates as a port this instance opened, in a region of its own
  rather than out of the image's pages, and freeing the hydrated region
  releases that region exactly once. A file port refuses the dump by name, and
  the artifact records no port address whatever the dumping process held. After
  image boot, `println` writes to the process's real stdout (the reconstructed
  default, not a stale dump-time resource), and `parameterize` of `*stdout*`
  redirects it. Both prove the captured `Parameter` identity and the fiber's
  frame lookup survived hydration.
- Closures: a closure compiled in one runtime hydrates in a fresh one and
  answers a call with the same result — through a REPL binding, so the call
  goes through the ordinary dispatch path. The payload survives field by
  field: bytecode, constants (a heap constant included), arity, signal,
  name, doc, and the capture masks. Two headers over one payload hydrate
  naming one payload copy. The counter-factual is a per-header deep copy,
  which round-trips equal and silently doubles every payload.

  A hydrated header answers its LIR, because the LIR is the payload's.
  `meta/origin` still answers, because the defining span is the payload's: a
  hydrated closure reports the line, the column and the file it was written
  at. The file table decides the file — rename the spelling there and the
  origin follows it. A lambda with no origin still answers nil, which is what
  stops the file stream from writing the table's first entry over an absent
  id.

  A WASM-dispatch closure and an env holding a run-time-minted capture cell
  each refuse the dump with a named error. A dumped closure writes one file
  across two dumps, whatever its construction temporaries held. The relocation
  stream records a shared payload's slots once, however many headers name it.
  The counter-factual is a per-header walk, which appends every inner entry
  again for each header and grows the tables with the header count.
- Children: a hydrated closure builds its nested lambda, and the lambda
  answers a call — through a REPL binding, so `MakeClosure` runs on the
  ordinary dispatch path. The child's payload crosses field by field, and a
  lambda nested two deep builds out of the child's own child table. The
  instruction materializes a fresh header per creation: two lambdas built
  from one hydrated parent are two headers over one payload. The
  counter-factual is handing out the image's own header, which answers every
  call correctly and quietly moves the instance-to-template edge across
  regions. A child a WASM module built refuses the dump like any other WASM
  closure, and a parent with a child writes one file across two dumps. A
  hydrated closure sent to a worker carries its children, which the worker
  rebuilds into the child table its own `MakeClosure` indexes.
- Snapping: a captured top-level crosses as its final value — the hydrated
  closure's env holds no cell, and a call through a REPL binding answers as
  the source closure did. Two closures over one cell hydrate naming one
  content copy; the counter-factual is a per-capture copy, which answers
  every read correctly and silently doubles the value. A top-level that is
  `assign`ed anywhere in the file fails the dump naming the binding, and a
  cell minted at run time (a captured lambda-local) still refuses. A snapped
  dump writes one file across two dumps.
- Boot: an instance that hydrated a boot image answers a stdlib call, a core
  call and a prelude macro expansion as a source-booted one does. It also
  reports that it booted from the image, because behaviour alone cannot tell
  the two apart. A macro the boot never expanded still expands after
  hydration, which is the lazy transformer fill working over a hydrated
  template. The installed expander mints the scopes a source boot would.

  A fresh instance prints a hydrated stdlib closure's name and answers
  `meta/origin` with the file it was written in, both out of the image's
  tables. An image whose source digest is not this binary's is refused by its
  own name, not the fingerprint's, and the instance boots from source instead.
  The counter-factual is an instance that answers with the previous library. A
  process holding a user signal bit fails the boot dump, naming the signal.

  Two dumps of one boot state write one file, which is determinism at the
  scale where a code payload's padding can leak. Hygiene: an image-booted
  instance tears down to the residue a source-booted one leaves, the whole boot
  graph released through one root registration. Boot from image running the
  full smoke corpus identically to source boot is dump-boot's gate.
- Warm cache: a second instance over one directory boots from the image the
  first stored, asserted on the reported boot source, and the stored file is
  the one the policy names. A rejected file is replaced rather than rejected
  again — the start that meets it compiles and stores, and the start after
  that hits. Two binary identities give two keys for one source digest, and a
  binary with no identity has no path. A valid image filed under the plain
  digest, as an older build named it, is never opened.

  A store prunes every other image in its directory. Two threads that store
  one image at once each rename a whole file. The default policy stores under
  the `--cache=` directory and the next start hydrates; `--boot-image=off` and
  `--cache=` with no directory read and write nothing. `elle image dump-boot`
  writes nothing into the cache, so it never booted from it.
- Workers: under `--trace=boot`, a start whose instance hydrated prints one
  `image-hydrate` mark for itself and one for each worker, heavy or light. A
  worker that a worker spawns prints one too. The counter-factual is a worker
  that compiles, which answers every call correctly and prints no mark. A cold
  start's worker hydrates the image the start stored.

  A `--no-stdlib` start's worker prints no mark, though the policy names a
  directory that holds an image. The counter-factual is a worker that reads
  the policy itself. A worker whose inherited image refuses, because its
  header was overwritten after the parent hydrated it, compiles and answers. A
  light worker of an image-booted instance still refuses a stdlib name.

  Image-booted workers run in sequence give back their mappings: the mapped
  bytes grow by less than one worker's instance across six more of them. The
  language server stores an image under a cold cache directory and writes no
  stdlib cache. The WASM entry's host runtime hydrates a stored image.
- Embedded blob: a static in this binary is found in the file that holds it,
  at a 64 KiB-aligned offset, and the bytes there are the static's. Bytes on
  the heap are found in no file. A blob no file holds hydrates from an
  anonymous copy, and so does a blob whose file holds other bytes at the
  offset. The counter-factual is a start that maps whatever the file holds
  there.

  An instance under `BootImage::Embedded` hydrates a valid blob and stores
  nothing in its warm-cache directory. The counter-factual is a boot that
  ignores the blob, which answers every call and stores an image. A refused
  blob falls back to the warm cache: the instance hydrates a stored image, or
  compiles and stores one. A refused blob with no directory compiles.

  A release `make elle` checks its own output. Stage 2's dump writes the blob
  again, byte for byte, and a `--cache=` start maps the blob and its worker
  hydrates it. A dry run of the Makefile shows the release `elle` and
  `elle-rig` builds passing `ELLE_BOOT_BLOB`, and stage 1 building in a target
  directory of its own. It shows the debug and variant builds passing none.
  The parity target's source boots pass `--boot-image=off`.
- Compile parity: a call to a stdlib function grafts under image boot as
  under source boot, and the same user file compiles to the same instruction
  stream under both. This is the acceptance gate for the body fragment a
  hydrated payload carries. The comparison reads each code object's opcodes
  and length rather than its bytes. A static region id in an operand comes
  from a process-wide mint ([the region model](../region/model.md)), which a
  stdlib compile advances, so two boots in one process differ in their
  operands while their instructions agree.
  `a_call_to_a_stdlib_function_grafts_under_either_boot` and
  `the_two_boots_emit_one_bytecode_for_a_user_file` pin the two.
- Bytecode parity: every corpus file and the standard library write one
  `--dump=bytecode` under an image boot and under a source boot, each in a
  process of its own (`make bytecode-parity`, which `make smoke-boot-image`
  runs first). The target refuses to compare until the image start prints its
  `image-hydrate` mark. A fixture that fuses a `map` and reads a qualified key
  holds the same in the Rust suite. The counter-factual is a hydration that
  leaves the static-region counter alone: every program compiles, fuses and
  runs as before, and every slot in its bytecode moves. A boot image's
  watermark is the mark its boot left, so two boot dumps with a compile between
  them still write one file; the counter-factual is the live counter, which
  that compile moves. An image of any other graph records zero.
- Resolution: overwrite the region id in every page header of a hydrated
  region, and each page still resolves to that region. `region_of_ptr` answers
  it, and an allocation that stores an image value records its edge into the
  image region. The counter-factual is the probe ladder, which reads the
  overwritten id and answers no region. The pool answers its interval exactly,
  and `owns` holds at both ends of the interval and fails one byte outside
  either. A page the region claims after hydration lies outside the interval
  and still resolves. A hydrated region is in its store's interval table until
  its pages are released.
