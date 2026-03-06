# Elle Demos

Demonstration programs that dogfood Elle with non-trivial algorithms and serve as cross-language comparison implementations.

## Demos

| Demo | Purpose |
|------|---------|
| [fib/](fib/) | Recursive Fibonacci benchmark measuring function call overhead |
| [nqueens/](nqueens/) | N-Queens backtracking algorithm solving the classic chess puzzle |
| [matrix-ops/](matrix-ops/) | Pure Lisp matrix operations (multiply, transpose, LU decomposition) |
| [aws-sigv4/](aws-sigv4/) | AWS API authentication with string manipulation, datetime handling, and cryptographic hashing |
| [blas/](blas/) | BLAS-style linear algebra operations (planned) |
| [logo/](logo/) | Logo turtle graphics implementation (planned) |
| [cfgviz/](cfgviz/) | Configuration visualization generating control flow graphs |
| [docgen/](docgen/) | Documentation site generator written in Elle |
| [scope-alloc/](scope-alloc/) | Allocator scope testing for memory management |
| [matrix.lisp](matrix.lisp) | Matrix operations reference implementation |
| [blas.lisp](blas.lisp) | BLAS operations reference implementation |
| [logo.lisp](logo.lisp) | Logo turtle graphics reference implementation |

## Running Demos

```bash
# Fibonacci
cargo run --release -- demos/fib/fib.lisp

# N-Queens
cargo run --release -- demos/nqueens/nqueens.lisp

# AWS SigV4
cargo run --release -- demos/aws-sigv4/sigv4.lisp

# Configuration visualization
cargo run --release -- demos/cfgviz/cfgviz.lisp

# Documentation site generator
cargo build --release && ./target/release/elle demos/docgen/generate.lisp

# Scope allocator
cargo run --release -- demos/scope-alloc/scope-alloc.lisp
```

## Purpose

These demos validate that Elle can express the same algorithms as other Lisps (Chez Scheme, SBCL, Janet) and identify missing features or pain points. They are executable documentation of Elle's capabilities, not tests.

Each demo typically includes implementations for multiple languages to enable side-by-side comparison of idioms, ergonomics, and performance.

See individual demo directories for detailed documentation and cross-language implementations.
