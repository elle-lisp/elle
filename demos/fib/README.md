# Fibonacci Benchmark

<!-- audited: 2026-09-22 -->

Naive recursive `fib(30)`: about 2.7 million calls, which measure the cost of a
function call and of generic arithmetic.

## What it computes

The Fibonacci sequence is defined recursively:

- `fib(0) = 0`
- `fib(1) = 1`
- `fib(n) = fib(n-1) + fib(n-2)` for n ≥ 2

```lisp
(defn fib [n]
  (if (< n 2)
    n
    (+ (fib (- n 1)) (fib (- n 2)))))
```

No memoization and no optimization: each call makes two more, so the time
grows as O(2^n). Computing `fib(30)` makes 2,692,537 calls.
[fib.lisp](fib.lisp) times the computation with `clock/monotonic`, which
returns seconds as a float, and prints the result and the elapsed
milliseconds. The same program in Janet, JavaScript, Lua, Python and Scheme
sits beside it for comparison.

## What it measures today

On a release build, with the default adaptive JIT:

```
fib(30) = 832040
elapsed: 15597.389312 ms
```

That is about 6 µs a call. Most of the time goes to generic arithmetic.
`+`, `-` and `<` are standard-library functions that take a variable number of
arguments and check each one's type, so each use is a full call. `-` builds a
closure to loop over its rest arguments, and the JIT refuses a function that
makes a closure, so every subtraction runs in the interpreter.

The `%` intrinsics are single instructions on proven numbers; see
[docs/intrinsics.md](../../docs/intrinsics.md). The same function written
with them runs in about 63 ms on the same machine:

```lisp
(defn fib [n]
  (if (%lt n 2)
    n
    (%add (fib (%sub n 1)) (fib (%sub n 2)))))
```

Performance is not the current focus. The planned path lowers generic
arithmetic to the intrinsics wherever type inference proves the operands are
numbers.

## Running the demo

```bash
cargo run --release -- demos/fib/fib.lisp
```

A debug build is many times slower.
