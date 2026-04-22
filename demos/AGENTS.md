# demos

Demonstration programs that serve two purposes: dogfooding Elle with non-trivial algorithms and cross-language comparison.

## Purpose

These demos validate that Elle can express the same algorithms as other Lisps (Chez Scheme, SBCL, Janet) and identify missing features or pain points. They are NOT tests — they are executable documentation of Elle's capabilities.

## Demo categories

| Demo | Purpose | Features tested |
|------|---------|-----------------|
| `blas/` | BLAS-style linear algebra | Numeric computation, array operations |
| `cfgviz/` | Configuration visualization | Data transformation, graph generation |
| `docgen/` | Documentation site generator | Full pipeline, file I/O, string processing |
| `fib/` | Fibonacci sequence | Recursion, memoization patterns |
| `logo/` | Logo turtle graphics | Graphics primitives, state management |
| `matrix/` | Matrix operations | Dense matrix representation, numeric computation, performance |
| `microgpt/` | Micro GPT autograd/neural network | Autograd engine, backpropagation, table operations |
| `nqueens/` | N-Queens backtracking algorithm | Recursion, list operations, functional predicates, backtracking |
| `scope-alloc/` | Allocator scope testing | Memory management, region-based allocation |
| `embedding/` | Elle as a shared library | Custom primitives, C-ABI surface, step-based scheduling |

## Running demos

```bash
# N-Queens
cargo run --release -- demos/nqueens/nqueens.lisp

# Fibonacci
cargo run --release -- demos/fib/fib.lisp

# Documentation site generator
cargo build --release && ./target/release/elle demos/docgen/generate.lisp

# Other demos
cargo run --release -- demos/blas/blas.lisp
cargo run --release -- demos/logo/logo.lisp
cargo run --release -- demos/matrix/matrix.lisp
cargo run --release -- demos/scope-alloc/scope-alloc.lisp
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

| Directory | Purpose |
|-----------|---------|
| `blas/` | BLAS-style linear algebra operations |
| `cfgviz/` | Configuration visualization |
| `docgen/` | Documentation site generator |
| `fib/` | Fibonacci sequence computation |
| `logo/` | Logo turtle graphics |
| `matrix/` | Matrix operations (multiply, transpose, LU decomposition) |
| `microgpt/` | Micro GPT autograd/neural network |
| `nqueens/` | N-Queens backtracking algorithm |
| `scope-alloc/` | Allocator scope testing |
| `README.md` | Detailed documentation and cross-language comparison |
