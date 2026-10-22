# benches

<!-- audited: 2026-10-22 -->

Criterion and reporting benchmarks for the Elle compiler and VM.

## Benchmark files

| File | Harness | Content |
|------|---------|---------|
| [benchmarks.rs](benchmarks.rs) | Criterion | Wall-clock benchmarks: parsing, symbol interning, compilation, VM execution, end-to-end eval, macro expansion |
| [memory.rs](memory.rs) | reporting | Heap allocations and bytes, total and net, while four programs compile and run: fib, n-queens, a list build, a closure loop |
| [regionrc.rs](regionrc.rs) | reporting | Compile-time RC-coalescing win: value→slot mint reduction (transform 1) and merge-induced self-edges eliminated (transform 2), over the stdlib load and both Elle suites |
| [fuel.rs](fuel.rs) | Criterion | The fuel charge per loop trip: a silent and a raising loop, tight and wide, on the bytecode and JIT tiers |

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
Each batch gets a fresh instance (`iter_batched` with `SmallInput`), so the
transformer cache starts cold, as it does for a user's fresh compilation unit.
The instance loads core and the prelude in the batch setup, which Criterion
does not time.

- **`when_100`**: 100 `(when true N)` forms. After the first expansion,
  subsequent calls use the cached transformer closure.
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
stdlib load, a sweep of every file at the top of [tests/lang](../tests/lang/overview.md)
and [tests/impl](../tests/impl/overview.md) (compile-only, failures skipped), and
a deterministic builder-idiom witness. `%pair` lowers as the
inline intrinsic, so the builder merge — hence transform 2 — fires wherever a
builder idiom seeds it.

```bash
cargo bench --bench regionrc
```

## The fuel bench — the ladder of charge shapes

Every backward jump and every call charges one unit of fuel
([runtime.md](../docs/runtime.md)). The bench reads what each operation of the
charge costs a loop trip. It runs one silent loop on the JIT under a ladder of
charge shapes, each one operation more than the last:

| Shape | The silent charge site holds |
|-------|------------------------------|
| `none` | nothing: the floor, a loop that checks nothing |
| `decrement` | a load, a subtract and a store |
| `saturating` | the same, and the compare and extend that stop the word at zero: today's silent charge ([impl/jit.md](../docs/impl/jit.md)) |
| `branch` | a load, a branch on zero around a subtract and a store: the shape of today's raising charge |
| `check` | the branch, and a load of a limit word, a compare and a second branch: the shape of a kill check |

Beside the ladder, every tier runs the same loop as a function that may raise,
whose charge is the real pausing one, and the interpreter runs the silent loop
as it charges it. Each loop comes tight, with two intrinsics beside the charge,
and wide, with eight. A reading is trips per second, so a case is the cost of
one trip, charge included.

The shapes are a knob in the translator that only a build with the `ablation`
feature carries ([config.md](../docs/config.md)), and the bench requires that
feature. A production build has no knob and emits the `saturating` shape at
every silent site. Save a baseline before a change to the charge and compare
after it:

```bash
cargo bench --features ablation --bench fuel -- --save-baseline before
# make the change, then:
cargo bench --features ablation --bench fuel -- --baseline before
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
