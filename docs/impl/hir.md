# HIR — High-level IR

The HIR pass converts expanded syntax trees into a typed intermediate
representation. It resolves bindings, computes captures, and infers
signal profiles.

## Key types

- **`Hir`** — a node in the HIR tree, carrying `HirKind`, source
  location, and inferred `Signal`
- **`HirKind`** — the node variant: literal, variable reference, call,
  lambda, let, if, begin, etc.
- **`Binding`** — resolved variable reference (local, capture, primitive,
  primitive)
- **`Signal`** — inferred effect profile (Pure, Yields, Polymorphic)

## What analysis does

1. **Binding resolution** — names → `Binding` references with
   scope depth and index
2. **Capture analysis** — which free variables a closure captures,
   whether they are mutable
3. **Signal inference** — interprocedural: traces call chains to
   determine whether a function can yield, error, or is pure
4. **Tail position marking** — identifies calls in tail position for
   TCO
5. **Special form analysis** — `if`, `let`, `begin`, `block`,
   `match`, `defmacro`, etc. each have dedicated handlers

## Signal inference

Three signal categories:
- **Pure** — no signals possible
- **Yields** — may yield (`:io`, `:yield`, `:fuel`, etc.)
- **Polymorphic** — signal behavior depends on a parameter
  (e.g., `(map f xs)` — signals depend on `f`)

`silence` constrains a parameter to be pure at compile time.
The inference propagates through call chains interprocedurally.

## Files

```text
src/hir/expr.rs           Hir and HirKind definitions
src/hir/analyze/mod.rs    Main analysis entry point
src/hir/analyze/binding.rs  Binding resolution
src/hir/analyze/forms.rs  Special form handlers
src/hir/analyze/special.rs  More special forms
```

---

## See also

- [impl/lir.md](lir.md) — lowering HIR to LIR
- [impl/reader.md](reader.md) — parsing before analysis
- [signals](../signals/index.md) — signal system design
