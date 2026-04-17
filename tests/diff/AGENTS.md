# tests/diff

Cross-tier differential agreement tests. Each script imports
`harness.lisp` (the local test harness) and asserts that the same
closure produces the same result on every compilation tier
(`:bytecode`, `:jit`, `:wasm`, `:mlir-cpu`, `:gpu`).

A disagreement is always a bug in the lowering, the eligibility
predicate, the tier dispatch, or the underlying engine. See
[`docs/impl/differential.md`](../../docs/impl/differential.md) for
the design.

## Running

```bash
make smoke-diff       # all tests, in parallel
./target/debug/elle tests/diff/branch.lisp   # individual test
```

`make smoke-diff` is wired into `make smoke`.

## Adding a new test

1. Create `tests/diff/<name>.lisp` covering one LIR construct or
   primitive (one new `LirInstr` arm in `src/mlir/lower.rs` should
   get one directed test here).
2. Import the harness: `(def diff ((import "tests/diff/harness")))`.
3. Wrap each closure under test with `(diff:assert-agree f & args)`.
   Use multiple input tuples per closure — boundary values
   (zero, negative, max), not just happy-path.
4. End with a final `(println "<test>: OK")` so parallel test output
   stays attributable.

## Files

| File | Coverage |
|------|----------|
| `harness.lisp` | The test harness itself (`try-on`, `call`, `assert-agree`, `prop`) |
| `runonly.lisp` | `compile/run-on` primitive itself (presence, error paths) |
| `lib-smoke.lisp` | Harness exports (`call`, `assert-agree`, `eligible-tiers`) |
| `disagreement.lisp` | The harness's own disagreement-detection path |
| `jit-smoke.lisp` | JIT tier directly via `compile/run-on :jit` |
| `arithmetic.lisp` | `BinOp::{Add,Sub,Mul,Div,Rem}` |
| `branch.lisp` | `Compare`, `Terminator::Branch`, multi-block CFG |
| `locals.lisp` | `LoadLocal` / `StoreLocal`, cross-block memref slots |
| `unary.lisp` | `UnaryOp::{Neg,BitNot}` |
| `bitwise.lisp` | `BinOp::{BitAnd,BitOr,BitXor,Shl,Shr}` |
| `tailcall.lisp` | Tail-call trampoline: GCD, factorial, iterative sum |
| `eligibility.lisp` | Negative tests: tier rejections for ineligible closures |
| `const.lisp` | Zero-arg, constant-returning, identity closures |
| `float.lisp` | Float constants, arithmetic, comparison, negation, mixed promotion, conditional returns |
| `signal.lisp` | Silent closures (silence + muffle) across tiers |
| `squelch.lisp` | Squelch enforcement: mask preserved and enforced across tiers |
| `gpu.lisp` | GPU tier agreement via `gpu:map` (conditional on vulkan) |
| `property.lisp` | Property-based random agreement via `diff:prop` (needs random plugin) |

## Skip handling

The `:mlir-cpu` tier is silently skipped on builds without
`--features mlir`. The `:wasm` tier is skipped without `--features wasm`.
The `:gpu` tier is opt-in via `diff:with-gpu` and only active in
`gpu.lisp`. The harness records skips in the report
(`:skipped {tier ...}`); `assert-agree` only fails if there's a
disagreement among tiers that *did* run. A test still passes when
only `:bytecode` and `:jit` agree.

The `gpu.lisp` and `property.lisp` tests check their prerequisites
(vulkan plugin, random plugin) and `exit 0` with a SKIP message if
not available.
