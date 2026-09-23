# Differential Tier Testing

<!-- audited: 2026-09-23 -->

A correct closure returns the same value on every execution tier that accepts it, and `compile/run-on` is how a test asks each tier.

Elle can run a closure on up to four tiers, and a fifth runs GPU kernels:

| Tier | Engine | Needs |
|------|--------|-------|
| bytecode | the interpreter | always available |
| jit | Cranelift native code | `--features jit` (default), and a closure the JIT can lower: it refuses `MakeClosure`, `eval`, and struct or named varargs |
| wasm | Wasmtime, one module per closure | `--features wasm`, and a closure with no tail call, signal emission, suspending call or module-less `MakeClosure` |
| mlir-cpu | MLIR and LLVM | `--features mlir`, and a closure `is_mlir_cpu_eligible` admits |
| gpu | SPIR-V on Vulkan, through `gpu:map` | `--features mlir` and the vulkan plugin; not a `compile/run-on` tier |

Each tier is a separate code path with its own value representation,
calling convention, and lowering pass. **A correct closure must produce
the same result on every tier that accepts it.** A disagreement is a bug
— in the lowering, the eligibility predicate, the dispatch, or the
underlying engine.

## Primitive

`(compile/run-on tier f & args)` force-runs `f` on the named tier with the
given arguments and returns the result. `tier` is one of:

- `:bytecode` — the interpreter. The JIT is off for this call, but a nested
  call still goes through ordinary tier dispatch.
- `:jit` — force-compiles through Cranelift, then calls the native code. A
  tail call out of the native code runs its callee once under the bytecode
  interpreter.
- `:wasm` — force-compiles through the tiered Wasmtime backend.
- `:mlir-cpu` — force-compiles through MLIR and LLVM, then calls through the
  `MlirCache`. The result comes back as an integer, a float or a boolean.

```lisp
(assert (= (compile/run-on :bytecode (fn [a b] (+ a b)) 3 4) 7))
(assert (= (compile/run-on :jit (fn [a b] (+ a b)) 3 4) 7))
```

A tier that does not accept the closure signals a structured
`:tier-rejected` error instead of running it. The `:reason` says why:

- `:ineligible` — the tier cannot compile this closure. MLIR-CPU also answers
  this for an argument or a capture that is not an integer or a float.
- `:feature-disabled` — the build does not carry the tier's feature.
- `:unknown-tier` — the keyword names no tier.

```lisp
(def [jit-ok? jit-err]
  (protect (compile/run-on :jit (fn [x] (fn [] x)) 1)))
(assert (not jit-ok?))                        # the JIT refuses MakeClosure
(assert (= (get jit-err :error) :tier-rejected))
(assert (= (get jit-err :reason) :ineligible))

(def [gpu-ok? gpu-err] (protect (compile/run-on :gpu (fn [] 1))))
(assert (= (get gpu-err :reason) :unknown-tier))
```

Arity and type errors in the arguments surface as ordinary errors, not as
rejections.

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
