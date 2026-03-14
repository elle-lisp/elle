# benches

Criterion and IAI benchmarks for the Elle compiler and VM.

## Benchmark files

| File | Harness | Content |
|------|---------|---------|
| `benchmarks.rs` | Criterion | Wall-clock benchmarks: parsing, symbol interning, compilation, VM execution, end-to-end eval, macro expansion |
| `iai_benchmarks.rs` | IAI Callgrind | Instruction-count benchmarks: parsing, compilation, execution (deterministic, requires Valgrind) |
| `memory.rs` | Criterion | Heap allocation benchmarks: arena growth, collection construction |

## Benchmark groups in `benchmarks.rs`

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

## `macro_expansion` group (issue #562)

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

## Running

```bash
# Dry-run (compile + single iteration, no timing):
cargo bench --bench benchmarks -- macro_expansion --test

# Full timing run:
cargo bench --bench benchmarks -- macro_expansion

# All benchmarks:
cargo bench
```
