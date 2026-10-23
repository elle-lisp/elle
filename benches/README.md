# Benchmarks

<!-- audited: 2026-10-23 -->

Criterion and reporting benchmarks for the Elle compiler and VM, run by hand.

[AGENTS.md](AGENTS.md) names each benchmark file, what it measures, and the
groups inside it. No CI job runs a benchmark: a reading comes from a run on
your own machine, compared against a baseline you saved there.

## Running

```bash
# Every benchmark
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

## See also

- [AGENTS.md](AGENTS.md) — the benchmark files and groups
- [Criterion.rs documentation](https://bheisler.github.io/criterion.rs/book/)
