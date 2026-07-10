# Differential Tier Testing

Elle compiles closures through up to five execution tiers:

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

## Primitive

```text
(compile/run-on tier f & args)
```

Force-runs `f` on the named tier with the given arguments and returns
the result. `tier` is one of:

- `:bytecode` — pure interpreter (JIT temporarily disabled for the call)
- `:jit` — force-compiles via Cranelift, then dispatches to native code;
  supports tail calls via a trampoline loop
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
arity or type errors. A tier whose *feature* is absent from the build
answers `:reason :feature-disabled` instead of `:ineligible`.

## The harness is the test runner

Cross-tier agreement is enforced by `elle test`
([docs/test-runner.md](../test-runner.md) § Tiers are intrinsic):

- Every **single-form** corpus file is forced onto every tier the build
  carries via `compile/run-on`; when tiers that returned a value
  disagree, the runner records a synthetic `status=diverge` row
  (`tier='*'`, the per-tier values in `reason`) and the run gates
  non-zero. Divergence coverage is therefore the default for the whole
  durable corpus, not a separate suite.
- A **directed** tier-parity test — one that must pin a specific
  tier-pair on a specific construct — lives in `tests/elle/` and calls
  `compile/run-on` explicitly, asserting the tiers' results against
  each other (e.g. `tests/elle/string-push-value.lisp`, which pins
  JIT==VM agreement for `%string-push` on an `@string` value).

There is no separate differential harness or corpus: the runner's
divergence status subsumed it, and per-file gating (`gate!`/`:gated`)
replaces its skip handling.

## See also

- [docs/test-runner.md](../test-runner.md) — the runner: tier matrix,
  divergence rows, gating
- [impl/mlir.md](mlir.md) — MLIR tier-2 lowering
- [impl/jit.md](jit.md) — Cranelift JIT
- [impl/spirv.md](spirv.md) — SPIR-V emission for the GPU tier
