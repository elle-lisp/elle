# Booting from an image

<!-- audited: 2026-09-22 -->

What the boot configuration carries, how `elle image dump-boot` writes it, and
how a fresh instance starts from it.

[image.md](../image.md) owns the design, [sealing.md](sealing.md) what a body
may hold, and [format.md](format.md) the file's sections. This document owns
the boot configuration: its root set, the source digest that decides whether an
image still describes these sources, and the warm cache that distributes it.

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
part every process already rebuilds for itself: a native-fn is an immediate
whose id hydration remaps by name ([format.md](format.md)).

The foundations sealed all three ([foundations.md](foundations.md)): exports are
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
([sealing.md](sealing.md)).

## The manifest is body data

[format.md](format.md) reserved a manifest section: a name, a kind, a value
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
out differently cannot be mapped at all ([format.md](format.md)). It says
nothing about the three sources, and an image built before somebody edited
stdlib.lisp is stale rather than incompatible — it would hydrate, and it would
answer with the previous library.

The root struct therefore carries a digest of core.lisp, prelude.lisp and
stdlib.lisp, and the installer refuses an image whose digest is not this
binary's. The warm cache names its file after the same digest, so an edit to
any of the three misses rather than loads. Keeping the digest in the image is
what covers the sources an embedded blob is built from, because a blob has no
filename to key on.

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
codec, which carries no cell's binding, so every restored capture cell reads as
minted at run time — and the dump refuses one of those by variant
([sealing.md](sealing.md)). A boot image therefore costs one stdlib compile per
digest, paid by the start that stores it.

The dump is the ordinary dumper over the root struct, so determinism, the
refusals, and the cross-build name and primitive tables are the ones
[image.md](../image.md) and [sealing.md](sealing.md) already describe. The boot
policy adds one refusal of its own: a process that registered a user signal bit
fails the dump, naming the signal. Bits above 31 are minted in declaration
order into a process-global registry, and no image carries a signal table yet.
So a bit baked into a dumped template would name a different signal in the
instance that hydrates it. None of the three sources declares one, which is
what makes the refusal free.

## Booting from one

A boot image hydrates into the instance being built, before its compile context
exists, and installs into the same structures a source boot writes:

1. Hydrate the image into this instance's heap, replaying the name table into
   its display memo and raising its parameter counter
   ([image.md](../image.md) § Hydration).
2. Check the digest against this binary's sources, and fall back to a source
   boot when it differs.
3. Register the hydrated region as a process root, so teardown releases the
   whole boot graph by the ordinary cascade — one registration, because one
   image is one region.
4. Install `:core` into the expander's `core_env` and into `PrimitiveMeta`.
5. Install `:macros` into the expander, and raise its scope counter past the
   image's watermark ([format.md](format.md)).
6. Install `:stdlib` into `PrimitiveMeta` and `eval_meta`.

Steps 4 and 6 are the tails of `compile_core` and `init_stdlib`. Step 5 leaves
the hydrated template trees where they are. The hydrated region is a process
root, so it outlives every expansion that reads a template, and a copy into the
expander's own template arena would buy nothing.

The raise in step 5 has nothing to clear at boot. A prelude template carries
the prelude scope, which is zero, so a boot image's watermark is one — where an
expander starts anyway. Every scope a boot mints is minted on a per-compile
clone of the master expander, and a clone's counter never reaches the master,
so an image boot and a source boot hand out the same ids with or without the
raise. The environment configuration is what will need it, because a macro a
session defined carries its file's scope.

The expander reads a template and never writes one, because it copies as it
stamps scopes ([syntax.md](../syntax.md)). So a template's pages stay clean,
and every process mapping the image shares them.

## The warm cache

The warm cache is the boot image's development distribution: when a valid image
is available, boot hydrates it; when none is, boot compiles from source and
dumps one for the next start.

- The file is `<dir>/<digest>.image`, where the digest is the one the root
  struct carries. A fingerprint mismatch, a corrupt file and a missing file are
  one case: compile, then store, so a rejected file is replaced rather than
  rejected again by every later start.
- The directory travels with the instance rather than coming from process
  state, for the reason [stdlib-cache.md](../stdlib-cache.md) argues. The suite
  builds many instances across threads, and a directory on the instance is what
  keeps one test's cache invisible to the test beside it.
- A store writes a temporary file in the same directory and renames it over the
  target. Never rewrite an image in place: a `MAP_PRIVATE` mapping of a file
  mutated underneath it is unspecified, and a live hydration holds one
  ([image.md](../image.md) § Hydration).
- A store prunes every other `.image` in the directory after the rename, never
  before. Each edit to a boot source mints a new digest and orphans the last
  file, at megabytes apiece.

A store failure is reported and ignored. The cache is a speedup, and a source
boot is always the fallback.

## The gate

`make smoke-boot-image` runs the corpus from an image instead of from the three
sources. The target fills a cache directory, proves that the next start
hydrates what it stored, and then runs the corpus with every `elle test` batch
pointed at that directory. A pull request gets the same run from the `Boot
Image Tests` job ([ci.md](../../analysis/ci.md)).

The proof is a `--trace=boot` start that has to print an `image-hydrate` mark.
Without it the target reports on a source boot: a binary that ignored
`--boot-image=` would pass it, and so would an image every start refuses and
replaces.

The runner instance is the one that hydrates, so every corpus file is read,
expanded and compiled against the image's macros and exports. A file's forms
then run on a worker with a VM of its own, which registers its own primitives
and loads its own stdlib where the file needs the library at run time.
Per-worker hydration is a milestone of its own ([plan.md](plan.md)).

## Why the cache is opt-in

`--boot-image=DIR` turns the warm cache on, `--boot-image=on` uses the
`--cache=` directory, and the default is off. Two pieces are still to land
([plan.md](plan.md)) — one of them a foundation rather than a boot milestone —
and each costs a hydrating instance something a source boot does not pay:

- Until LIR is region-native, a hydrated closure carries no LIR, so no stdlib
  function is ever promoted to the JIT tier
  ([foundations.md](foundations.md) argues that fix).
- Without the two cross-unit compile registries, user code compiles without
  cross-unit inlining and without stdlib dispatch monomorphization.

A default-on cache would trade steady-state throughput for startup, which
[image.md](../image.md) rejects as a violation of parity: the two boot modes
must be indistinguishable to running code, tiers included. The flag is what
makes the mechanism usable and testable meanwhile, and the default flips when
both land.
