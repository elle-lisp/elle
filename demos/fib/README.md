# Fibonacci Benchmark

## What This Demo Does

This demo computes the 30th Fibonacci number using a naive recursive algorithm. It's a classic benchmark that measures raw function call overhead — the recursive definition makes ~2.7 million function calls to compute a single result.

**Key features demonstrated:**
- Recursive function definition with `defn`
- Conditional branching with `if`
- Arithmetic operations
- Timing with `clock/monotonic`

## How It Works

The Fibonacci sequence is defined recursively:
- `fib(0) = 0`
- `fib(1) = 1`
- `fib(n) = fib(n-1) + fib(n-2)` for n ≥ 2

```janet
(defn fib (n)
  (if (< n 2) n
    (+ (fib (- n 1)) (fib (- n 2)))))
```

This is the simplest possible implementation — no memoization, no optimization. Each call to `fib(n)` recursively calls `fib(n-1)` and `fib(n-2)`, leading to exponential time complexity O(2^n).

The demo then:
1. Records the start time with `clock/monotonic`
2. Computes `fib(30)`
3. Records the end time
4. Displays the result and elapsed time in milliseconds

## Sample Output

```
fib(30) = 832040
elapsed: 17.292599 ms
```

The result 832040 is correct. The elapsed time (~17ms on modern hardware) reflects the cost of ~2.7 million function calls in Elle's interpreter.

## Elle Idioms Used

- **`defn`** — Prelude macro for function definition. Expands to `(def name (fn params body...))`
- **`if`** — Conditional expression. Returns the value of the taken branch
- **`let`** — Local binding (used implicitly in the timing code)
- **`clock/monotonic`** — Primitive that returns elapsed time since an arbitrary epoch in seconds (as a float)

## Why This Benchmark?

Fibonacci is a standard benchmark because:
1. It's simple to understand and implement
2. It exercises function call overhead heavily
3. It's deterministic and reproducible
4. It's used across many languages for comparison

This demo is useful for:
- Measuring interpreter performance
- Comparing Elle against other Lisps (Janet, Scheme, etc.)
- Understanding the cost of recursive function calls

## Running the Demo

```bash
cargo run --release -- demos/fib/fib.lisp
```

Use `--release` for optimized performance. Debug builds will be significantly slower.
