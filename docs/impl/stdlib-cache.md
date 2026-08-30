# Standard Library Disk Cache

`stdlib.lisp` (~2850 lines) is recompiled on every process start. The
`compile_file` front end (expand → analyze → regions → lower → emit) costs
~2.4s (release) while the compiled artifact takes ~5ms to execute. The front
end is **deterministic**: same source, same elle binary → same bytecode. This
design turns that work into a one-time cost by serializing the compiled
`Bytecode` to disk, so later processes deserialize instead of recompiling.

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

## Serialization format

The payload is a single `StoredBytecode` struct
(`src/compiler/stdlib_cache.rs`), **100% owned data** — no `Rc`, no pointers,
no process-local symbol-table ids:

```rust
struct StoredBytecode {
    format_version: u32,
    entry: SendableClosure,               // synthetic entry template
    intern_table: Vec<SendableClosure>,   // intern table of entry-reachable closure constants
    signal_projection: Option<HashMap<String, Signal>>,
}
```

### Why wrap the whole Bytecode in a synthetic entry `ClosureTemplate`?

The stdlib compile product is a `Bytecode` (entry instructions + constant pool
+ `child_protos` nested-lambda blueprints). Besides scalars, the constant pool
holds **36 closures with environments and 78 CaptureCells** (`init_stdlib`'s
returned closure, `map`, …), with cycles (closures referencing templates and
templates referencing closures in their constants). A bespoke scalar format
cannot handle this graph, but elle's send module (`value/send`, used to move
closures across threads/processes) already has an intern-table mechanism for
cycles.

So the `Bytecode` is wrapped as a synthetic `ClosureTemplate` with arity
`Exact(0)` (the entry runs as a thunk and is not JIT'd) and serialized through
`serialize_templates` uniformly:

- `instructions` → bytecode
- `constants` → entry constant pool
- `child_protos` → nested-lambda blueprints
- `frame_release_slots/regions`, `merged_slots` → region release tables
- closure instances in the constant pool are deep-copied and interned by pointer

The two extra fields on `StoredBytecode` (`signal_projection`,
`format_version`) don't exist on `ClosureTemplate`, so they ride alongside.

### LIR must be preserved

The JIT compiles from `ClosureTemplate.lir_function` in the background. If the
cache dropped LIR, every stdlib function would run **interpreted forever** (no
LIR → never submitted to the JIT worker) — a silent runtime regression, worse
than not caching. LIR is therefore serialized with the templates; only the
`doc`/`syntax` `Rc` fields are skipped (they are already `None` after the
cross-thread conversion, and the JIT never reads them).

### Symbols across processes

Symbol ids are process-local and cannot be serialized directly. Every
symbol/keyword is carried **by name** and re-interned into the loading
process's table. `LirConst::Symbol` in LIR materializes ids directly, so the
`symbol_names` map is used to remap LIR symbol ids wholesale before
deserialization.

### `frame_release_slots/regions` added to `SendableClosure`

Probing showed 346 templates carrying 4479 release slots — not droppable.
These two fields were missing from `SendableClosure`; adding them makes the
send/spawn path and the cache path share the same machinery and incidentally
fixes a send/spawn omission.

### `SendValue` and `TableKey` serialize through symmetric mirror enums

Hand-written tuple serialization drifts from derived deserialization
(bincode's enum-tag encoding differs), so both directions of each impl go
through one derived mirror enum (`src/value/send/mirror.rs`). A symbol
`TableKey` or symbol `Value` refuses to serialize: it carries only a
process-local id, no name a loader could re-intern — symbols cross by name as
`SendValue::Symbol` or `LirConst::Symbol` instead. Heap `Value`s are likewise
rejected — compound literals in the constant pool lower to `MaterializeConst`
templates at compile time and never enter the pool.

## Integration point

`init_stdlib` in `primitives/module_init.rs`:

1. `try_load`: on hit, deserialize and rebuild the `Bytecode`, then execute.
2. On miss (or decode failure): `compile_file`, then `try_store` to disk,
   then execute.
3. Hit and miss share `register_exports` (registering exports into the
   compilation cache's PrimitiveMeta), so both paths behave identically.

## Measured effect

Release build, cordis-pi startup (`[timing] boot`):

| Scenario | boot time |
|---|---|
| No cache (first compile of stdlib) | 3.13s |
| Cache hit | 1.36–1.86s |
| of which deserialization | 190–300ms |

Saves ~1.5s per start (~50%). Deserialization is the main remaining cost;
future work could lazy-load LIR separately or switch to a faster encoder than
bincode.

## Tests

- `bytecode_roundtrip_preserves_lir_and_closures`: after store → load the
  bytecode is equivalent (instructions identical, scalar-constant kinds/counts
  identical, LIR presence identical) and executes to the same result
  (`5 == 5`).
- `stdlib_cache_hit_produces_working_runtime`: two `Runtime`s created in
  sequence; the second must hit the disk cache and produce a fully working
  stdlib. The assertion is functional — timing is asserted in the release-mode
  boot benchmark, since debug compile time masks the difference.

## Trade-offs

- **Cache key includes the elle version**: compiler changes invalidate
  automatically; the cost is one recompile after upgrading elle. Acceptable.
- **Fate of `Value` scalar serde**: entry constants now go entirely through
  `SendableClosure`, no longer through `StoredValue`; but LIR `ValueConst`
  still uses the scalar serde, so it stays for now.
- **Silent store failure**: the cache is only a speedup; a failed write does
  not block startup.
