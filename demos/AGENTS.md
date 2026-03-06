# demos

Demonstration programs that serve two purposes: dogfooding Elle with non-trivial algorithms and cross-language comparison.

## Purpose

These demos validate that Elle can express the same algorithms as other Lisps (Chez Scheme, SBCL, Janet) and identify missing features or pain points. They are NOT tests — they are executable documentation of Elle's capabilities.

## Demo categories

| Demo | Purpose | Features tested |
|------|---------|-----------------|
| `nqueens/` | N-Queens backtracking algorithm | Recursion, list operations, functional predicates, backtracking |
| `matrix-ops/` | Pure Lisp matrix operations | Dense matrix representation, numeric computation, performance |
| `aws-sigv4/` | AWS API authentication | String manipulation, datetime handling, URL encoding, FFI (hashing) |
| `fib/` | Fibonacci sequence | Recursion, memoization patterns |
| `blas.lisp` | BLAS-style linear algebra | Numeric computation, array operations |
| `logo.lisp` | Logo turtle graphics | Graphics primitives, state management |
| `cfgviz/` | Configuration visualization | Data transformation, graph generation |
| `scope-alloc/` | Allocator scope testing | Memory management, region-based allocation |

## Running demos

```bash
# N-Queens
cargo run --release -- demos/nqueens/nqueens.lisp

# Matrix operations
cargo run --release -- demos/matrix-ops/matrix.lisp

# AWS SigV4
cargo run --release -- demos/aws-sigv4/sigv4.lisp

# Other demos
cargo run --release -- demos/fib.lisp
cargo run --release -- demos/blas.lisp
cargo run --release -- demos/logo.lisp
```

## Writing new demos

### Structure

Each demo should have:
1. **Header comment** — Explain what the demo demonstrates
2. **Imports** — Load required libraries or modules
3. **Helper functions** — Define utility functions with `defn`
4. **Main logic** — Implement the algorithm
5. **Output** — Display results with `println` or `print`

### Conventions

- Use `defn` for function definitions (prelude macro)
- Use `let*` for local bindings (prelude macro)
- Use `->` and `->>` for threading (prelude macros)
- Use `when` and `unless` for conditionals (prelude macros)
- Use `fold` for reductions
- Use `map` and `filter` for transformations
- **No assertions** — demos are not tests. Use `println` to display results.
- **No error handling** — let errors propagate naturally
- **Display output** — make results visible to the user

### Example structure

```janet
;; N-Queens: Find all valid placements of N queens on an NxN chessboard
;; Tests: recursion, list operations, backtracking

(defn safe? [queens col]
  "Check if placing a queen at col is safe given existing queens"
  (let* ((row (length queens))
         (check-col (fn [q-col]
           (or (= q-col col)
               (= (abs (- row (length queens))) (abs (- col q-col)))))))
    (not (some check-col queens))))

(defn solve-nqueens [n]
  "Find all solutions for N-Queens problem"
  (defn backtrack [queens]
    (if (= (length queens) n)
      (list queens)
      (let ((solutions (list)))
        (for col 0 n
          (when (safe? queens col)
            (set solutions (append solutions (backtrack (append queens (list col)))))))
        solutions)))
  (backtrack (list)))

(println "N-Queens solutions:")
(println (solve-nqueens 4))
```

### Differences from examples/

| Aspect | Demos | Examples |
|--------|-------|----------|
| **Purpose** | Dogfooding, cross-language comparison | Language feature documentation |
| **Assertions** | None — display output | Yes — verify behavior |
| **Error handling** | Let errors propagate | Demonstrate error handling |
| **Size** | Larger algorithms | Small, focused features |
| **Output** | User-visible results | Test assertions |
| **Maintenance** | Compare with other Lisps | Update when language changes |

## Known issues and fixes

See `demos/README.md` for a detailed list of known issues, their status, and how they were fixed.

## Files

| File | Purpose |
|------|---------|
| `nqueens/` | N-Queens backtracking algorithm |
| `matrix-ops/` | Matrix operations (multiply, transpose, LU decomposition) |
| `aws-sigv4/` | AWS API authentication implementation |
| `fib/` | Fibonacci sequence computation |
| `blas.lisp` | BLAS-style linear algebra operations |
| `logo.lisp` | Logo turtle graphics |
| `cfgviz/` | Configuration visualization |
| `scope-alloc/` | Allocator scope testing |
| `README.md` | Detailed documentation and cross-language comparison |
