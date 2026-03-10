# Elle Demos

Demonstration programs that dogfood Elle with non-trivial algorithms and serve as cross-language comparison implementations.

## Demos

| Demo | Purpose |
|------|---------|
| [aws-sigv4/](aws-sigv4/) | AWS API authentication with string manipulation, datetime handling, and cryptographic hashing |
| [blas/](blas/) | BLAS-style linear algebra operations |
| [cfgviz/](cfgviz/) | Configuration visualization generating control flow graphs |
| [docgen/](docgen/) | Documentation site generator written in Elle |
| [fib/](fib/) | Recursive Fibonacci benchmark measuring function call overhead |
| [logo/](logo/) | Logo turtle graphics implementation |
| [matrix/](matrix/) | Matrix operations reference implementation |
| [microgpt/](microgpt/) | Micro GPT autograd engine and neural network |
| [nqueens/](nqueens/) | N-Queens backtracking algorithm solving the classic chess puzzle |
| [scope-alloc/](scope-alloc/) | Allocator scope testing for memory management |

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

# Other demos
cargo run --release -- demos/blas/blas.lisp
cargo run --release -- demos/logo/logo.lisp
cargo run --release -- demos/matrix/matrix.lisp
```

## Purpose

These demos validate that Elle can express the same algorithms as other Lisps (Chez Scheme, SBCL, Janet) and identify missing features or pain points. They are executable documentation of Elle's capabilities, not tests.

Each demo typically includes implementations for multiple languages to enable side-by-side comparison of idioms, ergonomics, and performance.

See individual demo directories for detailed documentation and cross-language implementations.
