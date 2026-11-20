# Booting from an image

<!-- audited: 2026-10-21 -->

What the boot configuration carries, how `elle image dump-boot` writes it, and
how a fresh instance starts from it.

[image.md](../image.md) owns the design, [sealing.md](sealing.md) what a body
may hold, [format.md](format.md) the file's sections, and [blob.md](blob.md)
the embedded blob. This document owns the boot configuration: its root set,
and the source digest that decides whether an image still describes these
sources. It also owns the policy that picks the image a start reads, the warm
cache, and how a worker boots from its parent's.

## The boot state is one value graph

A source boot leaves three things behind that no later process can rebuild
without running the front end again:

- core.lisp's exports, which reach user code through `PrimitiveMeta` and macro
  bodies through the expander's `core_env`;
- prelude.lisp's macros, each a parameter list, a template tree and a lazily
  filled transformer;
- stdlib.lisp's exports, which reach user code through `PrimitiveMeta` and
  macro bodies through `eval_meta`.

The primitives are the fourth part of a booted instance, and they are the one
part every process already rebuilds for itself. A native-fn is an immediate
whose id hydration remaps by name ([the image file](format.md)).

The [foundations](foundations.md) sealed all three: exports are
closures and parameters over region-native code payloads, and a macro is its
template tree. So the boot image's root is one immutable struct, and its fields
are ordinary body data:

| Key | Value |
|-----|-------|
| `:sources` | the digest of the three sources this image was built from |
| `:core` | core.lisp's export struct |
| `:stdlib` | stdlib.lisp's export struct |
| `:macros` | a struct of macro name to macro entry |

A macro entry is a struct of its own: `:params`, `:optional` and `:rest` carry
the parameter lists, `:template` carries the template tree, and `:transformer`
carries the compiled transformer or nil. A transformer the boot never compiled
hydrates absent and fills lazily, exactly as it does under a source boot
([sealing](sealing.md)).

## The manifest is body data

[The image file](format.md) reserved a manifest section: a name, a kind, a value
location, a signal and an arity per binding, with macro entries carrying more.
The boot configuration needs none of it. An export struct already names each
binding with a keyword key, and each exported value already answers its own
signal, arity and doc — which is where a source boot reads them from too.

So the installer reads names and signals through the same code a source boot
reads them through, and the two boots share one tail. A section would restate
what the struct holds, in a second encoder, which is a second place for the two
boots to drift apart. The header still carries what is not a value: the
fingerprint, the watermarks, and — for an image that depends on another — the
dependency fingerprints. The boot configuration's dependency list is empty, so
it records none.

## The digest decides whether the image still describes these sources

The fingerprint gates *layout*: an image whose dumping binary laid `HeapObject`
out differently cannot be mapped at all ([the image file](format.md)). It says
nothing about the three sources, and an image built before somebody edited
stdlib.lisp is stale rather than incompatible — it would hydrate, and it would
answer with the previous library.

The root struct therefore carries a digest of core.lisp, prelude.lisp and
stdlib.lisp, and the installer refuses an image whose digest is not this
binary's. The warm cache's key holds the same digest, so an edit to any of the
three misses rather than loads. Keeping the digest in the image is what covers
the sources an embedded blob is built from, because a blob has no filename to
key on.

## Producing one

```sh
elle image dump-boot boot.image
```

The subcommand boots from source and dumps the result, so the artifact it
writes is the one those three sources produce. It never boots from an image
itself: an artifact two hops from the sources it claims is one nobody can
check by rebuilding it.

The stdlib disk cache is off for that boot, and a warm-cache store compiles for
the same reason. A cache hit rebuilds the library's closures through the send
codec, which carries no cell's binding. So every restored capture cell reads as
minted at run time, and the dump refuses one of those by variant
([sealing.md](sealing.md)). A boot image therefore costs one stdlib compile per
build and digest, paid by the start that stores it.

The dump is the ordinary dumper over the root struct, so determinism, the
refusals, and the cross-build name and primitive tables are the ones
[dumping](dumping.md) and [sealing](sealing.md) already describe. The
boot policy adds one refusal of its own: a process that registered a user signal
bit fails the dump, naming the signal. Bits above 31 are minted in declaration
order into a process-global registry, and no image carries a signal table yet.
So a bit baked into a dumped template would name a different signal in the
instance that hydrates it. None of the three sources declares one, which is
what makes the refusal free.

## Booting from one

A boot image hydrates into the instance being built, before its compile context
exists, and installs into the same structures a source boot writes:

1. Hydrate the image into this instance's heap, replaying the name table into
   its display memo and raising its parameter and static-region counters
   ([hydration](hydration.md)).
2. Check the digest against this binary's sources, and fall back to a source
   boot when it differs.
3. Register the hydrated region as a process root, so teardown releases the
   whole boot graph by the ordinary cascade — one registration, because one
   image is one region.
4. Install `:core` into the expander's `core_env` and into `PrimitiveMeta`.
5. Install `:macros` into the expander.
6. Install `:stdlib` into `PrimitiveMeta` and `eval_meta`.

Steps 4 and 6 are the tails of `compile_core` and `init_stdlib`. Step 5 leaves
the hydrated template trees where they are. The hydrated region is a process
root, so it outlives every expansion that reads a template, and a copy into the
expander's own template arena would buy nothing.

Step 5 raises no counter. A scope id names the unit that minted it
([syntax.md](../syntax.md)), and a hydrating instance never expands the
prelude, so it mints no id a prelude template carries. A macro a session
defined carries its own unit's tag, so the environment configuration needs no
raise either.

The expander reads a template and never writes one, because it copies as it
stamps scopes ([the syntax tree](../syntax.md)). So a template's pages stay clean,
and every process mapping the image shares them.

## The embedded blob

A release executable carries a boot image of its own, the embedded blob, which
[blob.md](blob.md) owns: the two-stage build, the alignment, and how a start
finds the bytes.

`BootImage::Embedded` names a blob and a warm-cache directory to fall back to,
and `BootImage::Process` resolves to it when the binary carries a blob. A
hydrated blob stores nothing, because the image a store would write is the one
the binary carries. The boot reports a refused blob and goes on to the warm
cache.

## The warm cache

The warm cache distributes the boot image to a build that carries no blob, and
to a start that refuses the blob it carries. When a valid image is available,
boot hydrates it; when none is, boot compiles from source and dumps one for the
next start.

- The file is `<dir>/<key>.image`. `<key>` hashes the source digest the root
  struct carries with the binary's identity, which is the length and
  modification time of the executable, as the stdlib cache reads them
  ([stdlib-cache.md](../stdlib-cache.md)).
- An image another build stored has another name, so this binary never opens
  it. A binary that cannot read its own identity reads and writes no image.
- A fingerprint mismatch, a corrupt file and a missing file are one case:
  compile, then store, so a rejected file is replaced rather than rejected
  again by every later start.
- The directory travels with the instance rather than coming from process
  state, for the reason [the stdlib cache](../stdlib-cache.md) argues. The suite
  builds many instances across threads, and a directory on the instance is what
  keeps one test's cache invisible to the test beside it.
- A store writes a temporary file in the same directory and renames it over the
  target. The temporary file's name is the store's alone, because two threads
  of one process can store one image at once. Never rewrite an image in place:
  a `MAP_PRIVATE` mapping of a file mutated underneath it is unspecified, and a
  live hydration holds one ([hydration](hydration.md)).
- A store prunes every other `.image` in the directory after the rename, never
  before. Each rebuild and each edit to a boot source orphans the last file, at
  megabytes apiece. A directory therefore holds one image whatever runs in it,
  and that bound is the reason for the rule. A rustdoc test, for example, runs
  from a fresh temporary executable every time, and each one stores an image.
- Executables that share a directory evict each other, as they do in the stdlib
  cache. `elle` and `elle-rig`, or a debug and a release build, then pay one
  compile and one dump at each switch. Give them separate directories with
  `--boot-image=DIR` or `--cache=` when that matters.

A store failure is reported and ignored. The cache is a speedup, and a source
boot is always the fallback.

## A worker boots as its parent did

A `sys/spawn` worker builds an instance of its own on a new thread
([threads.md](../../threads.md)). A region belongs to one store, so the worker
cannot read the region its parent hydrated. It hydrates the same image into its
own heap instead. Each worker holds a private mapping, and the clean pages are
shared between them through the page cache.

- The worker hydrates from the descriptor its parent hydrated, never from a
  path. The parent records the open image on its VM, and the worker takes a
  share of it, as it takes the Unicode generation. A prune or a rename over the
  file after the parent opened it therefore changes nothing a worker maps.
- A parent that compiled and then stored an image hands its workers the file
  it stored, so the workers of a cold start hydrate too.
- A parent with no image to hand on has workers that compile, through the
  parent's stdlib cache. That parent booted with `--no-stdlib` or through
  `Runtime::with_stdlib_cache`, or its store failed. A worker never reads the
  process policy itself, so it never boots from an image its parent did not
  hold.
- A worker never stores. Its parent already holds the image a store would
  write.
- A worker whose hydration refuses the image reports the refusal and compiles.
  It hands on only an image it hydrated, so its own workers compile too.

The descriptor is the hydrator's input form ([hydration](hydration.md)). So the
embedded blob reaches a worker the same way, whether the parent mapped it from
the executable or copied it. A process copies a blob
once.

`sys/spawn-vm` starts a light worker, with no standard library. A source boot
of one compiles core.lisp and prelude.lisp and never compiles stdlib.lisp. An
image boot of one installs the same two parts, the core exports and the macros,
and leaves out the stdlib exports. So `eval` in a light worker resolves no
stdlib name under either boot, and it grafts no stdlib call. A transformer
that the image carries filled was compiled while the boot compiled
stdlib.lisp, before any stdlib export existed. So it calls no stdlib function
that a light worker lacks.

The language server and the host side of the WASM entry boot as `elle` does:
under the process policy, hydrating on a hit, and storing on a miss. A WASM
entry without the stdlib boots from source, as `--no-stdlib` does.

`init_stdlib` stays the path that compiles stdlib.lisp. An image installs
before the compile context exists, and `init_stdlib` runs after it, so it
cannot hydrate. An embedder that wants the image boots through `Runtime`.

## The gate

`make smoke-boot-image` runs the corpus from the warm cache instead of from the
three sources. The target fills a cache directory, proves that the next start
hydrates what it stored, and then runs the corpus with every `elle test` batch
pointed at that directory. A pull request gets the same run from the `Boot
Image Tests` job ([CI](../../analysis/ci.md)). The other corpus jobs run
release binaries that carry a blob, so they run the corpus from the blob.

The target runs `make bytecode-parity` first, the compile half of
[parity](parity.md). It writes every corpus file's
`--dump=bytecode` once under a source boot and once under an image boot, and
fails on any difference. Each dump runs in a process of its own, because the
static-region counter is process-global: a second boot in one process starts
where the first left it. The source boots pass `--boot-image=off`, because a
binary that carries a blob hydrates it under `--cache=` alone.

Two things make the dumps agree. Each stdlib closure carries its body
fragment in its payload ([graft.md](../graft.md)), and the image's header
carries the static-region watermark ([format.md](format.md)). The compile
context records the counter where the boot ends, as it installs the stdlib
exports, and the boot dump writes that mark. So the watermark is the counter a
source boot hands its first user compile, however late the dump runs.

The proof is a `--trace=boot` start that has to print an `image-hydrate` mark.
Without it the target reports on a source boot: a binary that ignored
`--boot-image=` would pass it, and so would an image every start refuses and
replaces. The parity target asks for the same mark, or it would compare two
source boots.

The runner instance hydrates, and so does every worker it spawns to run a
file's forms (§ A worker boots as its parent did). So every corpus file is
read, expanded, compiled and run against the image's macros and exports.

## What the default reads

The default is `--boot-image=on`. It hydrates the embedded blob, then the warm
cache in `boot` beneath the `--cache=` directory ([config.md](../../config.md)).
`--cache=` with no directory turns the warm cache off and leaves the blob.
`--boot-image=DIR` reads and writes the warm cache in DIR and never reads the
blob, so a test of the warm cache tests it on every build. `--boot-image=off`
compiles core, prelude and stdlib at every start.

The default reaches every `Runtime::new()`: `elle`, its REPL and its
subcommands, and an embedder's runtime. It reaches the language server and the
WASM entry's host runtime, and every worker of an instance that holds an image
(§ A worker boots as its parent did). Four paths always boot from source:
`--no-stdlib`, `elle image dump-boot` (§ Producing one),
`Runtime::with_stdlib_cache`, which names the stdlib cache it reads, and
`init_stdlib`.

The default has two costs where no blob hydrates. A start that misses the warm
cache compiles the three sources and skips the stdlib cache, because the store
needs a compiled boot (§ Producing one). A process that declares a user signal
before it boots cannot store at all, so it pays that compile at every start and
reports the refused store.
