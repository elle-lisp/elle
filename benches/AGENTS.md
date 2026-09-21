# benches

<!-- audited: 2026-09-21 -->

Criterion and reporting benchmarks for the Elle compiler and VM.

## Benchmark files

| File | Harness | Content |
|------|---------|---------|
| [benchmarks.rs](benchmarks.rs) | Criterion | Wall-clock benchmarks: parsing, symbol interning, compilation, VM execution, end-to-end eval, macro expansion |
| [memory.rs](memory.rs) | Criterion | Heap allocation benchmarks: arena growth, collection construction |
| [regionrc.rs](regionrc.rs) | reporting | Compile-time RC-coalescing win: value→slot mint reduction (transform 1) and merge-induced self-edges eliminated (transform 2), over the stdlib load and the `tests/elle` corpus |
| [lirshape](lirshape/main.rs) | reporting | Region-native LIR against the Rust-heap `LirFunction`: build, copy, walk, rewrite, teardown, allocator traffic and resident bytes, over the LIR of the three boot sources |

## Benchmark groups in benchmarks.rs

| Group | Description |
|-------|-------------|
| `parsing` | Reader/lexer throughput for various input shapes |
| `symbol_interning` | First-intern vs repeat-intern cost |
| `compilation` | Full compile pipeline (parse → HIR → LIR → bytecode) |
| `vm_execution` | Raw bytecode execution, no compilation |
| `conditionals` | Branch-heavy execution patterns |
| `end_to_end` | parse + compile + execute combined |
| `scalability` | Throughput vs input size (list construction, arithmetic chains) |
| `memory_operations` | Value clone, list-to-vec |
| `macro_expansion` | Macro expansion throughput: `when_100`, `thread_first_9`, `defn_50` |

## The macro_expansion group

Measures the end-to-end cost of expanding macro-heavy Elle snippets.
Each sub-benchmark allocates a fresh VM and SymbolTable per iteration
(`iter_batched` with `SmallInput`) so prelude loading is included in
the measured time — this is intentional: it captures the full pipeline
cost a user would pay for a fresh compilation unit.

- **`when_100`**: 100 `(when true N)` forms. `when` is the most common
  prelude macro. After the first expansion, subsequent calls use the
  cached transformer closure.
- **`thread_first_9`**: `(-> 1 (+ 2) ... (+ 10))` — 9 applications of
  the thread-first macro. Each step is a recursive macro call; the
  transformer closure is cached after the first.
- **`defn_50`**: 50 `(defn fN (x) ...)` definitions. `defn` desugars
  to `(def name (fn ...))` via the prelude macro.

## The regionrc bench — the RC-coalescing measured win

The bench reports how many region-mints the lowerer resolves to a static slot
(transform 1's value→slot reduction) versus leaves value-resolved at the dynamic
boundary, plus the merge-induced self-edges transform 2 eliminates
([region/mechanism.md](../docs/impl/region/mechanism.md)). It is a *reporting*
bench (prints counts, asserts nothing — "the win is measured, not asserted")
driven by the thread-local instrument in `elle::lir::lower::rcstats`, which the
lowerer bumps at each coalescing-candidate site. It measures three things: the
stdlib load, a sweep of every `tests/elle/*.lisp` (compile-only, failures
skipped), and a deterministic builder-idiom witness. `%pair` lowers as the
inline intrinsic, so the builder merge — hence transform 2 — fires wherever a
builder idiom seeds it.

```bash
cargo bench --bench regionrc
```

## The lirshape bench — region-native LIR against the shipped form

The bench encodes the LIR of core.lisp, prelude.lisp and stdlib.lisp into a
region as a fixed-size POD node, then runs the same five operations over both
forms and reports each side by side
([measurements.md](../docs/impl/image/measurements.md) item 7 records what it
answered). Reporting, like regionrc: it prints numbers and asserts only that
the two forms carry the same instruction and operand counts.

Four files, because the encoder and the 88-arm opcode table each earn one:
`main.rs` drives and reports, `node.rs` defines the node, `build.rs` encodes,
`ops.rs` holds each measured operation written twice.

```bash
cargo bench --bench lirshape
```

## Running

```bash
# Dry-run (compile + single iteration, no timing):
cargo bench --bench benchmarks -- macro_expansion --test

# Full timing run:
cargo bench --bench benchmarks -- macro_expansion

# All benchmarks:
cargo bench
```
