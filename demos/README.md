# Elle Lisp Demos: Dogfooding & Cross-Language Comparison

This directory contains demonstration programs that serve two purposes:

1. **Dogfooding**: Testing Elle itself with non-trivial algorithms
2. **Cross-Language Comparison**: Implementing the same algorithms in multiple Lisps to compare ergonomics, idioms, and performance

## Contents

- [The Philosophy](#the-philosophy)
- [Demo Categories](#demo-categories)
- [Running the Demos](#running-the-demos)
- [Known Issues](#known-issues)
- [Code Organization](#code-organization)
- [Contributing Fixes](#contributing-fixes)
- [What These Demos Test](#what-these-demos-test)
- [Future Demos](#future-demos)
- [Notes for Implementers](#notes-for-implementers)
- [References](#references)

## The Philosophy

Elle is designed with Janet ergonomics in mind. These demos allow us to:

- Validate that Elle can express the same algorithms as other Lisps
- Identify missing features or pain points in Elle
- Compare code clarity and readability across languages
- Understand performance characteristics relative to mature implementations
- Discover and document bugs in Elle that real code would expose

## Demo Categories

### [N-Queens Problem](nqueens/)

A classic backtracking algorithm that solves the N-Queens chess problem. Tests:
- **Recursion depth** and tail-call patterns
- **List operations** (cons, append, reverse, first, rest)
- **Functional predicates** (safe? function to check placement validity)
- **Backtracking logic** (exploring multiple branches, accumulating solutions)

**Results Summary:**
| Language | N=4 | N=8 | Status |
|----------|-----|-----|--------|
| Chez Scheme | 2 ✓ | 92 ✓ | Working |
| SBCL | 2 ✓ | 92 ✓ | Working |
| Janet | 2 ✓ | 92 ✓ | Working |
| Elle | 2 ✓ | 92 ✓ | Working |

### [Matrix Operations](matrix-ops/)

Pure Lisp matrix operations testing numeric computation and performance. Tests:
- **Dense matrix representation** (array of arrays, 2D arrays)
- **Numeric computation** (matrix multiply, transpose, LU decomposition)
- **Performance at different scales** (16x16, 64x64, 256x256 matrices)
- **Loops vs functional iteration** patterns

**Status:**
| Language | 16x16 | 64x64 | 256x256 | Status |
|----------|-------|-------|---------|--------|
| Chez Scheme | ✓ | ✓ | ✓ | Working |
| SBCL | - | - | - | In progress |
| Janet | - | - | - | Planned |
| Elle | - | - | - | Planned |

**Performance (Pure Lisp, no optimization):**
- Chez 256x256 matrix multiply: ~128ms
- Chez LU decomposition: ~0.88ms

### [AWS SigV4 Signing](aws-sigv4/)

AWS API authentication implementation testing string manipulation, datetime handling, and cryptographic operations. Tests:
- **Datetime parsing and formatting** (ISO 8601, AWS format YYYYMMDDTHHMMSSZ)
- **String manipulation** (canonicalization, padding, formatting)
- **URL/URI encoding** (percent encoding for query parameters)
- **Hex conversion** (bytevector to hex string)
- **Cryptographic hashing** (SHA256, HMAC-SHA256 via crypto plugin)

**Status:**
| Language | Parsing | Encoding | Formatting | Hashing | Status |
|----------|---------|----------|------------|---------|--------|
| Chez Scheme | ✓ | ✓ | ✓ | ✗ (FFI) | Partial |
| SBCL | - | - | - | - | Planned |
| Janet | - | - | - | - | Planned |
| Elle | ✓ | ✓ | ✓ | ✓ | Working |

**What This Reveals:**
- How well each Lisp handles string operations
- Need for datetime libraries and utilities
- Cryptographic capabilities (native plugin vs FFI)
- Real-world API authentication patterns

## Running the Demos

### N-Queens

```bash
# Chez Scheme
chezscheme --script nqueens/nqueens.scm

# SBCL
sbcl --script nqueens/nqueens.lisp.cl

# Janet
janet nqueens/nqueens.janet

# Elle
cargo run --release -- nqueens/nqueens.lisp
```

### Matrix Operations

```bash
# Chez Scheme
chezscheme --script matrix-ops/matrix-pure.scm

# SBCL
sbcl --script matrix-ops/matrix-pure.lisp.cl
```

### AWS SigV4 Signing

```bash
# Chez Scheme
chezscheme --script aws-sigv4/sigv4.scm
```

### Expected Output Examples

**N-Queens (correct implementation):**
```
Solving N-Queens for N=4... Found 2 solution(s)
Solving N-Queens for N=8... Found 92 solution(s)
Solving N-Queens for N=10... Found 724 solution(s)
Solving N-Queens for N=12... Found 14200 solution(s)
```

**Matrix Operations (Chez, 256x256):**
```
Large matrix (256x256):
Matrix multiply (256x256): done in ~128.29ms, norm=169254.21
Matrix transpose (256x256): done in ~0.29ms
LU decomposition (256x256): done in ~0.88ms
```

**AWS SigV4 (Chez):**
```
=== Timestamp Parsing Test ===
Input: 2023-02-08T15:30:45Z
Parsed: (2023 2 8 15 30 45)

=== URI Encoding Test ===
Input:  hello world
Encoded: hello%20world

=== DateTime Formatting Test ===
Date (YYYYMMDD): 20230208
DateTime (YYYYMMDDTHHmmSSZ): 20230208T153045Z
```

## Code Organization

Each demo typically has implementations for:
- `demo.scm` - Chez Scheme (reference implementation)
- `demo.lisp.cl` - SBCL Common Lisp
- `demo.janet` - Janet
- `demo.lisp` - Elle

This structure makes it easy to compare implementations side-by-side.

## Contributing Fixes

If you fix one of the known issues:

1. Update the status table above
2. Run the demo to verify it produces correct results
3. Create a pull request with the fix and a note about which issue it closes

## What These Demos Test

### Language Features Tested Across All Demos

| Feature | N-Queens | Matrix Ops | SigV4 |
|---------|----------|-----------|-------|
| **Recursion** | ✓ | - | - |
| **Backtracking** | ✓ | - | - |
| **List operations** | ✓ | - | - |
| **Numeric loops** | - | ✓ | - |
| **2D arrays** | - | ✓ | - |
| **String manipulation** | - | - | ✓ |
| **DateTime handling** | - | - | ✓ |
| **URL encoding** | - | - | ✓ |
| **Cryptographic hashing** | - | - | ✓ (plugin) |

## Future Demos

Planned demonstrations:

- **BLAS/LAPACK FFI** (calling optimized numeric libraries from matrix ops)
- **Symbolic Differentiation** (macro/metaprogramming showcase)
- **Game Tree Search** (Minimax with alpha-beta pruning)
- **JSON Processing Pipeline** (data transformation idioms)
- **HTTP Server** (I/O, networking, concurrency)

## Notes for Implementers

When implementing a demo for a new language:

1. Try to match the algorithm structure of the reference implementation (Chez)
2. Use idiomatic patterns for that language (don't force Scheme-style code)
3. Comment anywhere the language's approach differs from the reference
4. Include debugging output to help diagnose issues if results are incorrect
5. Document any discovered limitations or surprising behaviors

## References

- [Chez Scheme Documentation](https://www.scheme.com/csug10.0/)
- [SBCL Documentation](http://www.sbcl.org/manual/)
- [Janet Documentation](https://janet-lang.org/docs/index.html)
- [Elle Documentation](../docs/)
