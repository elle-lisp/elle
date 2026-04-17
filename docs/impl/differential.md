# Differential Tier Testing

Elle compiles closures through up to four execution tiers:

```text
1. bytecode   — interpreter, always available
2. jit        — Cranelift native code, requires LIR + non-polymorphic
3. wasm       — Wasmtime tiered, requires --features wasm + no tail calls
4. mlir-cpu   — MLIR/LLVM tier-2, requires --features mlir + GPU eligibility
5. gpu        — SPIR-V on Vulkan, via gpu:map (--features mlir + vulkan plugin)
```

Each tier is a separate code path with its own value representation,
calling convention, and lowering pass. **A correct closure must produce
the same result on every tier that accepts it.** A disagreement is a bug
— in the lowering, the eligibility predicate, the dispatch, or the
underlying engine.

The differential testing harness force-runs a closure on each tier and
asserts agreement. It is the single highest-leverage correctness
investment for the multi-backend story: every new `LirInstr` lowering,
every SPIR-V opcode mapping, every eligibility loosening immediately
gets pressure-tested against the tiers that already work.

## Primitive

```text
(compile/run-on tier f & args)
```

Force-runs `f` on the named tier with the given arguments and returns
the result. `tier` is one of:

- `:bytecode` — pure interpreter (JIT temporarily disabled for the call)
- `:jit` — force-compiles via Cranelift, then dispatches to native code
- `:wasm` — force-compiles via Wasmtime tiered backend (only available
  with `--features wasm`; rejects closures with tail calls)
- `:mlir-cpu` — force-compiles via MLIR + LLVM, then invokes via the
  `MlirCache` (only available with `--features mlir`)

If the tier doesn't accept this closure (e.g. polymorphic closure on
JIT, non-int-returning closure on MLIR-CPU), the primitive signals a
structured error:

```text
{:error :tier-rejected :message "..." :tier :mlir-cpu :reason :ineligible}
```

Tier eligibility is independent of arguments — it's a property of the
closure's compiled form. Argument-shape mismatches surface as ordinary
arity or type errors.

## Library

`lib/differential.lisp` wraps the primitive with a comparison harness:

```text
(diff-call f & args)
   → {:agreed true  :tiers <set> :result v}
   | {:agreed false :tiers <set> :results {tier value ...}}

(eligible-tiers f)
   → set of tiers that accept f, e.g. |:bytecode :jit :mlir-cpu|

(assert-agree f & args)   ## convenience for tests; throws on disagreement
   → agreed result, or signals :diff-disagreement
```

`diff-call` always runs `:bytecode`, plus any other tier the closure is
eligible for. The bytecode result is the reference; other tiers are
compared against it with `=`. Tiers that reject the closure are skipped
(not counted as disagreements).

`assert-agree` is the canonical test helper: it succeeds silently when
all eligible tiers agree, signals `:diff-disagreement` with a structured
report when they don't.

## Test layout

```text
tests/diff/
  add.lisp          arithmetic
  branch.lisp       if/else, comparisons
  locals.lisp       let bindings, assign
  letrec.lisp       cross-block local stores
  unary.lisp        neg, not, bit-not
  bitwise.lisp      and, or, xor, shl, shr
  ...
```

Each test imports `lib/differential.lisp` and calls `assert-agree` on a
closure with several input tuples. Failures are loud:

```text
diff-disagreement {
  closure:  (fn [a b] (* a (+ b 1)))
  args:     [3 7]
  tiers:    |:bytecode :jit :mlir-cpu|
  results:  {:bytecode 24 :jit 24 :mlir-cpu 27}
}
```

## Running

```bash
make smoke-diff       # all tests/diff/*.lisp on a build with --features mlir
```

Built into `make smoke` so every PR exercises tier agreement on
GPU-eligible code.

## Phase 1 scope

Phase 1 ships `:bytecode`, `:jit`, `:wasm`, `:mlir-cpu`. The GPU tier
is added in Phase 2 by routing through `(gpu:map f [args])` —
operationally identical to "call on GPU" for one-element batches.
Tail-call loop support, float tolerance modes, property-based
generators, and auto-promoted existing tests follow in later phases.

## See also

- [impl/mlir.md](mlir.md) — MLIR tier-2 lowering
- [impl/jit.md](jit.md) — Cranelift JIT
- [impl/spirv.md](spirv.md) — SPIR-V emission for the GPU tier
- [impl/gpu.md](gpu.md) — Vulkan dispatch for the GPU tier
