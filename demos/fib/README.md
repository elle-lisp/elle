# Fibonacci Benchmark

<!-- audited: 2026-10-21 -->

Naive recursive `fib(30)`: about 2.7 million calls, which measure the cost of a
function call and of integer arithmetic.

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
elapsed: 68.58399299999999 ms
```

That is about 25 ns a call. `<`, `-` and `+` are standard-library functions,
and the compiler replaces each call to them with the function's body
([grafting a call](../../docs/impl/graft.md)). The program's one call passes
`fib` an integer, so type inference proves `n` and each result an integer.
The fold then deletes the type guards, so each operator is one integer
instruction, and the JIT compiles the whole function.

The same function written with the `%` intrinsics
([docs/intrinsics.md](../../docs/intrinsics.md)) folds to the same operations
and takes the same time:

```lisp
(defn fib [n]
  (if (%lt n 2)
    n
    (%add (fib (%sub n 1)) (fib (%sub n 2)))))
```

The proof comes from the calls the program makes. When the program also passes
`fib` as a value, for example to `type-of`, no type reaches `n`, the guards
stay, and `fib(30)` takes about 300 ms. A build with no JIT tier takes about
1.1 s.

## Running the demo

```bash
cargo run --release -- demos/fib/fib.lisp
```

A debug build is many times slower.
