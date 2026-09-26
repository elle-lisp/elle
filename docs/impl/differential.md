# Differential Tier Testing

<!-- audited: 2026-09-29 -->

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

- `:bytecode` — the interpreter. The JIT is off for this call and for every
  call it makes. A call it makes can still run on the WASM or MLIR tier.
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
  this for an argument or a capture that is not an integer or a float, and
  `:jit` for a closure that suspends, because it cannot hold the suspension.
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
rejections. So does an error the closure raises on a tier that runs it: the
caller receives that error, as it would from an ordinary call.

```lisp
(def [raise-ok? raise-err]
  (protect (compile/run-on :jit (fn [] (error {:error :boom :message "b"})))))
(assert (not raise-ok?))
(assert (= (get raise-err :error) :boom))     # not :tier-rejected
```

## The harness is the set of builds

Cross-tier agreement is enforced by running one language suite on several
implementations ([spec](../spec.md) § A build is an implementation):

- **Every build runs the language suite.** The default build runs it on the
  JIT, a build with no JIT runs it on the interpreter alone, and an MLIR build
  runs it on MLIR, with the interpreter for every function MLIR does not admit.
  A language test states the one answer every build must give,
  so a tier that disagrees fails the build that carries it
  ([ci](../analysis/ci.md)).
- **The rig runs it eager.** The implementation suite runs the language suite
  once more under the rig's `jit-eager.toml` profile, which compiles every
  function on its first call ([rig](../../rig/overview.md) § Profiles). The
  default build compiles only what runs ten times, so this pass is what drives
  the JIT over code that a test calls once.
- A **directed** tier-parity test — one that must pin a specific tier pair on
  a specific construct — is an implementation test. It lives in `tests/impl/`
  and calls `compile/run-on` explicitly, asserting the tiers' results against
  each other (for example `tests/impl/string-push-value.lisp`, which pins
  JIT==VM agreement for `%string-push` on an `@string` value).

The runner itself runs each file once and forces no tier
([docs/test-runner.md](../test-runner.md) § A build is the tier set).

## See also

- [docs/test-runner.md](../test-runner.md) — the runner: isolation, gating,
  and why it has no tier dial
- [impl/mlir.md](mlir.md) — MLIR tier-2 lowering
- [impl/jit.md](jit.md) — Cranelift JIT
- [impl/spirv.md](spirv.md) — SPIR-V emission for the GPU tier
