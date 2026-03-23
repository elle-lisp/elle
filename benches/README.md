# Benchmarks

Performance benchmarks for Elle using Criterion. Benchmarks measure compilation time, execution speed, and memory usage across various workloads.

## Running Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run a specific benchmark
cargo bench benchmark_name

# Run with verbose output
cargo bench -- --verbose

# Save results for comparison
cargo bench -- --save-baseline my_baseline

# Compare against a baseline
cargo bench -- --baseline my_baseline
```

## Benchmark Types

| Type | Tool | Measures | Use for |
|------|------|----------|---------|
| **Criterion** | Criterion.rs | Execution time, variance | Detecting regressions |

## What Benchmarks Measure

- **Compilation**: Time to compile Elle code to bytecode
- **Execution**: Time to run compiled code
- **Startup**: Time to initialize the VM
- **Memory**: Heap allocations and peak memory usage
- **JIT**: Performance of JIT-compiled code vs interpreted

## Benchmark Organization

Benchmarks are organized by category:

- **Arithmetic**: Numeric computation performance
- **List operations**: Cons, car, cdr, length
- **String operations**: Concatenation, slicing, searching
- **Closures**: Capture, call overhead
- **Control flow**: If, while, match performance
- **Macros**: Expansion time

## CI Integration

The CI pipeline runs benchmarks and reports regressions:

```
Benchmark Results:
  arithmetic/add: 1.23ms (no change)
  list/length: 4.56ms (+5% regression)
  closure/call: 2.34ms (no change)
```

Regressions are reported but don't fail the build (`fail-on-alert: false`).

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [Criterion.rs documentation](https://bheisler.github.io/criterion.rs/book/)
