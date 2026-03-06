# Reference Documentation

This directory contains external language reference documentation from other Lisp implementations (Scheme, Common Lisp, Janet) used as design inspiration for Elle.

## Purpose

These are NOT Elle's implementation documentation. They are reference materials for understanding how other Lisps approach similar problems. Elle's design draws inspiration from these languages but is not a direct port.

## Contents

- **Scheme references**: Chez Scheme, R7RS specifications
- **Common Lisp references**: SBCL, HyperSpec documentation
- **Janet references**: Janet language documentation

## Using These References

When designing Elle features, consult these references to:

- Understand how mature Lisps solve similar problems
- Compare ergonomics and idioms across languages
- Identify best practices and common patterns
- Avoid known pitfalls and design mistakes

## Elle's Design Philosophy

Elle is designed with **Janet ergonomics in mind**, not as a direct port of any other Lisp. Key differences:

- **Immutable by default**: Collections are immutable unless prefixed with `@`
- **Keyword arguments**: Functions use keywords for named parameters
- **Fibers**: Lightweight concurrency via fibers, not threads
- **Effects system**: Explicit effect tracking for optimization
- **Scope allocation**: Region-based memory management for scopes

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`docs/`](../) - Elle's implementation documentation
- [`docs/pipeline.md`](../pipeline.md) - Elle's compilation pipeline
