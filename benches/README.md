# Benchmarks

<!-- audited: 2026-10-26 -->

Criterion and reporting benchmarks for the Elle compiler and VM, run by hand.

No CI job runs a benchmark: a reading comes from a run on your own machine,
compared against a baseline you saved there. A Criterion bench times its cases.
A reporting bench is a plain `main` that prints what it counted.

## The files

| File | Harness | Content |
|------|---------|---------|
| [benchmarks.rs](benchmarks.rs) | Criterion | Wall-clock benchmarks: parsing, symbol interning, compilation, VM execution, end-to-end eval, macro expansion |
| [memory.rs](memory.rs) | reporting | Heap allocations and bytes, total and net, while four programs compile and run: fib, n-queens, a list build, a closure loop |
| [mutable.rs](mutable.rs) | reporting | An `@set` and a `@struct` beside std's `BTreeSet` and `BTreeMap`: insert and delete at 10³, 10⁴ and 10⁵ random keys, exit status 1 past the bound [the design](../docs/impl/mutable.md) sets at 10⁵ |
| [regionrc.rs](regionrc.rs) | reporting | Compile-time RC-coalescing win: value→slot mint reduction (transform 1) and merge-induced self-edges eliminated (transform 2), over the stdlib load and both Elle suites ([region/mechanism.md](../docs/impl/region/mechanism.md)) |
| [fuel.rs](fuel.rs) | Criterion | The fuel charge per loop trip: a silent and a raising loop, tight and wide, on the bytecode and JIT tiers |

Each file is a `[[bench]]` of [the root manifest](../Cargo.toml), and
[benches.rs](../tests/integration/benches.rs) holds this table, the files and
the manifest to one list.

## Running

```bash
# Every benchmark but the fuel bench, which needs a feature
cargo bench

# One benchmark file, or one group inside it
cargo bench --bench benchmarks
cargo bench --bench benchmarks -- macro_expansion

# Compile and run each case once, with no timing
cargo bench --bench benchmarks -- macro_expansion --test

# Save a baseline before a change, then compare after it
cargo bench -- --save-baseline before
cargo bench -- --baseline before
```

Criterion writes its reports under `target/criterion/`, one directory per
case, with the confidence interval of each reading.

## The groups in benchmarks.rs

| Group | Description |
|-------|-------------|
| `parsing` | Reader/lexer throughput for various input shapes |
| `symbol_interning` | First-intern vs repeat-intern cost |
| `compilation` | Full compile pipeline (parse → HIR → LIR → bytecode) |
| `vm_execution` | Raw bytecode execution, no compilation |
| `conditionals` | Branch-heavy execution patterns |
| `end_to_end` | parse + compile + execute combined |
| `scalability` | Throughput vs input size (list construction, arithmetic chains) |
| `memory_operations` | Value copy, list-to-vec |
| `macro_expansion` | Macro expansion throughput: `when_100`, `thread_first_9`, `defn_50` |

### The macro_expansion group

The group times the compile and run of macro-heavy Elle snippets. Each batch
builds a bare instance in its untimed setup: core and the prelude, and no
stdlib. No unit on that instance has expanded a macro, so a case times the
first compile of each transformer and every expansion after it. An instance
that loaded the stdlib has already compiled these transformers, and reuses
them.

- **`when_100`**: 100 `(when true N)` forms. After the first expansion,
  each call reuses the cached transformer closure.
- **`thread_first_9`**: `(-> 1 (+ 2) ... (+ 10))` — 9 applications of
  the thread-first macro. Each step is a recursive macro call; the
  transformer closure is cached after the first.
- **`defn_50`**: 50 `(defn fN (x) ...)` definitions. `defn` desugars
  to `(def name (fn ...))` via the prelude macro.

## The fuel bench — the ladder of charge shapes

Every backward jump and every call charges one unit of fuel
([runtime.md](../docs/runtime.md)). The bench reads what each operation of the
charge costs a loop trip. It runs one silent loop on the JIT under a ladder of
charge shapes, each one operation more than the last:

| Shape | The silent charge site holds |
|-------|------------------------------|
| `none` | nothing: the floor, a loop that checks nothing |
| `decrement` | a load, a subtract and a store |
| `saturating` | the same, and the compare and extend that stop the word at zero: a silent charge with no kill check |
| `branch` | a load, a branch on zero around a subtract and a store: the raising charge less its pause |
| `check` | the branch, and a load of the kill word, a compare and a second branch: the silent charge every build emits ([impl/jit-fuel.md](../docs/impl/jit-fuel.md)) |

Beside the ladder, every tier runs the same loop as a function that may raise,
whose charge is the real pausing one, and the interpreter runs the silent loop
as it charges it. Each loop comes tight, with two intrinsics beside the charge,
and wide, with eight. A reading is trips per second, so a case is the cost of
one trip, charge included.

The shapes are a knob in the translator that only a build with the `ablation`
feature carries ([config.md](../docs/config.md)), and the bench requires that
feature. A production build has no knob and emits the `check` shape at
every silent site. Save a baseline before a change to the charge and compare
after it:

```bash
cargo bench --features ablation --bench fuel -- --save-baseline before
# make the change, then:
cargo bench --features ablation --bench fuel -- --baseline before
```

## See also

- [Criterion.rs documentation](https://bheisler.github.io/criterion.rs/book/)
