# Standard Library Disk Cache

<!-- audited: 2026-09-09 -->

`stdlib.lisp` (~2900 lines) is recompiled on every process start. The
`compile_file` front end (expand → analyze → regions → lower → emit) is what
boot spends its time on; executing the compiled artifact costs a few
milliseconds. The front end is **deterministic**: same source, same elle binary
→ same bytecode. This design turns that work into a one-time cost by serializing
the compiled `Bytecode` to disk, so later processes deserialize instead of
recompiling.

## High-level flow

```
First start                  Later starts
────────────                  ────────────
compile_file(STDLIB)         try_load (cache hit)
      │                            │
      ▼                            ▼
 try_store ──► cache.bin ──► deserialize & rebuild Bytecode
      │                            │
      └──────────► vm.execute ◄────┘
```

The cache is a speedup, not a correctness dependency: any failure (no directory
permission, corrupt file, format-version mismatch) falls back to a full
recompile, and store failures are silently ignored.

## Cache key and invalidation

```
cache_key = hash(stdlib source, build identity, FORMAT_VERSION,
                 primitive-table identity) + ".bin"
```

- **stdlib source**: the text embedded via `include_str!`; a source change
  naturally invalidates it.
- **build identity**: the running executable's length and modification time
  (`std::env::current_exe`). Not the version string: two builds of one version
  compile stdlib differently the moment the emitter or a pass changes, and a
  key blind to the rebuild hands every `Runtime::new()` — the one in each test
  included — bytecode the previous binary produced, so a test failure stops
  implicating the branch that caused it. It also covers the ids `prim_id_of`
  appends outside the canonical tables (trait methods, FFI callbacks), which
  the table identity below cannot see. A binary that cannot identify itself
  gets no cache at all rather than sharing one.
- **FORMAT_VERSION**: a hard version number bumped manually on incompatible
  layout changes; validated on load.
- **primitive-table identity**: the ordered names and aliases of every
  canonical primitive (`hash_prim_table_identity`). A serialized native-fn
  immediate carries a `prim_id` that is only valid against the exact table
  that minted it, so any addition, removal, rename, or reorder must
  invalidate the cache.

## Where the cache lives

The directory is a **construction parameter**, `StdlibCache`, passed to
`Runtime::with_stdlib_cache` and threaded to `try_load`/`try_store`. It is not
read from process-global state: the suite builds many runtimes across threads,
and a directory that travels with the instance is what keeps one runtime's
cache invisible to the runtime beside it — and what lets a test tell the
instance that wrote a cache from the instance that read it.

| Variant | Directory |
|---------|-----------|
| `Process` (the default) | `stdlib-cache` beneath the `--cache=<dir>` directory; `--cache=` turns caching off for the process |
| `Dir(path)` | `path`, whatever the process-wide choice is |
| `Off` | none — always compile |

`Runtime::stdlib_source()` reports which side of the fork an instance took. A
cache that silently never hits still yields a working runtime, so behaviour
alone cannot distinguish the two; the tests assert on this instead.

A `sys/spawn` worker builds its own runtime on a new thread and runs
`init_stdlib` there, so it caches too. It reaches the spawning instance only
through `ctx.vm()`, so the policy is recorded on the VM by
`RuntimeCore::load_stdlib` and moved into the worker thread beside the Unicode
generation — a worker caches where its parent was told to, never in the
process-wide directory.

## File layout

A cache file is an 8-byte little-endian hash of its payload, then the payload:

```
[u64 payload hash][bincode StoredBytecode]
```

`bincode` reports that bytes *decoded*, never that they are the bytes this
binary wrote. Without the prefix, eight flipped bytes decode "successfully" and
arrive at the VM as instructions (`invalid opcode 0xff`), and a flip deeper in
a payload is absorbed into stdlib and reported as a hit. A hash mismatch, or a
file shorter than the prefix, is a **miss**: the loader says so and the caller
compiles.

This detects corruption and truncation, not forgery. A writable cache directory
is a code-execution surface like any other loadable artifact.

A store prunes: after the rename, every other `.bin` in the directory is
removed. The key follows the binary, so every rebuild mints a new one and
orphans the last one's file — at ~8 MB each, a day of rebuilds fills a
directory nobody looks at. Pruning runs *after* the rename, never before, so a
store that fails leaves the directory as it found it. A removal that fails is
ignored; it is disk hygiene, not correctness.

One consequence worth knowing: a debug and a release binary sharing one
directory evict each other, so alternating between them costs one stdlib
compile per switch. Give them separate directories (`--cache=`) if that matters.

A store never writes the final path. It writes a temporary file in the same
directory and renames it over the target: two elle processes starting at once
is ordinary, and writing the path directly lets one read the other's
half-written file, or edits an inode a reader already holds open. Same
directory because a rename is atomic only within one filesystem.

## What the cached path does not restore

`TemplateProto.origin` — the span where the source lambda was written — does not
cross the cache. `SendableClosure` carries no field for it, and a span names its
file by an id interned in a process-global table (`syntax/files.rs`) in
interning order, so the same number names a different file in a process that
compiled a different set. Its only reader is `(meta/origin f)`, which reports a
closure's `{:file :line :col}` from that span, so a stdlib closure has an origin
on the compiled path and `nil` on the cached one.

Nothing in the tree depends on it: the corpus exercises `meta/origin` only on
closures it compiles itself. The loss is bounded to the values the cache
restored — a closure a cache-hit runtime compiles still carries its origin — and
`a_cached_stdlib_closure_has_no_origin_but_user_code_keeps_its_own` pins both
halves.

Carrying it means carrying each span's file *spelling* alongside and
re-interning it on load, the way the `names` table already carries symbol
spellings. [The image work](image.md) makes syntax region-native, at which point
a source position is ordinary body data rather than a codec's problem.

## Serialization format

The payload is a single `StoredBytecode` struct
(`src/compiler/stdlib_cache.rs`), **100% owned data** — no `Rc`, no pointers,
no process-local symbol-table ids:

```rust
struct StoredBytecode {
    format_version: u32,
    entry: SendableClosure,               // synthetic entry template
    intern_table: Vec<SendableClosure>,   // intern table of entry-reachable closure constants
    names: Vec<(u64, Box<str>)>,          // spellings, replayed into the loading display memo
    signal_projection: Option<HashMap<String, Signal>>,
    dispatch_wrappers: StoredDispatchRegistry,  // cross-unit monomorphization
    fn_inline: StoredFnInlineRegistry,          // cross-unit HOF-argument inlining
}
```

The two registries ride along because a hit skips the stdlib compile that
populates them. They drive an HIR rewrite in every later compile, so a snapshot
that dropped entries would make the cached path compile user code differently
from the compiled path.

### Why wrap the whole Bytecode in a synthetic entry `ClosureTemplate`?

The stdlib compile product is a `Bytecode`: entry instructions, a constant pool,
and a `child_protos` tree of nested-lambda blueprints, over a hundred of them.
The blueprints hold closures in their own constant pools, and the reference
graph is cyclic — a closure names its template, and a template's constants name
closures. A bespoke scalar format cannot represent that, but elle's send module
(`value/send`, which already moves closures across threads and processes) interns
closure instances by pointer and refers to each by index.

So the `Bytecode` is wrapped as a synthetic `ClosureTemplate` with arity
`Exact(0)` (the entry runs as a thunk and is not JIT'd) and serialized through
`serialize_templates` uniformly:

- `instructions` → bytecode
- `constants` → entry constant pool
- `child_protos` → nested-lambda blueprints
- `frame_release_slots/regions`, `merged_slots` → region release tables
- closure instances are deep-copied and interned by pointer

`format_version`, `signal_projection` and the two registries have no
`ClosureTemplate` field to travel in, so they ride alongside.

### LIR must be preserved

The JIT compiles from `ClosureTemplate.lir_function` in the background. If the
cache dropped LIR, every stdlib function would run **interpreted forever** (no
LIR → never submitted to the JIT worker) — a silent runtime regression, worse
than not caching. LIR is therefore serialized with the templates; only the
`doc`/`syntax` `Rc` fields are skipped (they are already `None` after the
cross-thread conversion, and the JIT never reads them).

### Symbols across processes

A symbol or keyword id is [its name's hash](symbol.md), the same number in
every process, so the ids travel as they stand and need no remapping. What
does not travel is the **spelling**: the loading instance's display memo learned
nothing from a compile it skipped, so every reloaded name would print as
`#<symbol:hash>`. The `names` table carries the spellings the entry and its
templates use, and a load replays them into that memo.

### `frame_release_slots/regions` added to `SendableClosure`

The stdlib's templates carry thousands of release slots between them — the
tables [an abandoned frame runs what it still owes off](region/unwind.md) — so
dropping them was never an option. Both
fields were missing from `SendableClosure`; adding them makes the send/spawn
path and the cache path share one mechanism.

### `SendValue` serializes through a symmetric mirror enum

Hand-written tuple serialization drifts from derived deserialization
(bincode's enum-tag encoding differs), so both directions go through one
derived mirror enum (`src/value/send/mirror.rs`). Struct keys travel as
`SendKey`, [the owning key form](values.md), which owns its bytes and derives
serde directly; a symbol or keyword key travels as its name hash, which names
the same symbol in the loading process. An identity key has no `SendKey` form
and is refused,
which the cache layer turns into a miss. Heap `Value`s are rejected too —
compound literals in the constant pool lower to `MaterializeConst` templates at
compile time and never enter the pool.

## Integration point

`init_stdlib` in `primitives/module_init.rs`:

1. `try_load`: on hit, deserialize and rebuild the `Bytecode`, then execute.
2. On miss (or decode failure): `compile_file`, then `try_store` to disk,
   then execute.
3. Hit and miss share `register_exports` (registering exports into the
   compilation cache's PrimitiveMeta), so both paths behave identically.

The values a hit restores are held Rust-side for the life of the instance, so
no `DecrefValueRegion` ever runs against the region they are born in.
`load_bytecode` therefore deserializes through a process-root allocation
context, and the teardown sweep reclaims the reload by RC like any other root
([the allocation capability](region/ctx.md)).

## Measured effect

Release build, `--trace=boot`, running `(+ 1 2)`:

| Phase | No cache | Cache hit |
|---|---|---|
| obtaining the compiled stdlib | 266ms | 23ms |
| executing it | 3.5ms | 3.8ms |

The front end is the whole of the difference, and deserialization is what
remains of it; a faster encoder or a lazily-loaded LIR is where the rest would
come from. Re-measure with `--trace=boot` rather than reading these numbers —
they move with the machine and with every change to a front-end pass.

## Tests

- `bytecode_roundtrip_preserves_lir_and_closures`: after store → load the
  bytecode is equivalent (instructions identical, scalar-constant kinds/counts
  identical, LIR presence identical) and executes to the same result
  (`5 == 5`).
- `second_runtime_on_a_shared_cache_dir_loads_stdlib_from_it`: two `Runtime`s
  created in sequence over one directory; the second must hit the disk cache and
  produce a fully working stdlib. The assertion is functional — timing is
  asserted in the release-mode boot benchmark, since debug compile time masks
  the difference.
- `a_cache_hit_leaves_no_unexplained_references`
  (`tests/region_process_teardown`): a hit must leave the same post-teardown
  residue a compiled stdlib leaves, with nothing pinned from outside the region
  graph.

## Trade-offs

- **Cache key follows the binary**: any rebuild invalidates automatically; the
  cost is one recompile per new binary. Acceptable.
- **Store failure does not block startup**: the cache is only a speedup; a
  failed write is reported on stderr and the fresh compile proceeds.
