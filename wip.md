# WIP: JIT SIGSEGV in nqueens letrec

## Status

`make test` passes. `cargo test --test lib -- integration::jit` has one
SIGSEGV: `test_jit_mutual_recursion_nqueens_small`. This test existed
before our branch (P0 commit 96c0451d) but was masked — the wrong
letrec signal inference caused JIT rejection, so the test ran via
interpreter only.

## Blocking defect

`test_jit_mutual_recursion_nqueens_small` crashes with SIGSEGV in
`prim_first` when called from JIT-compiled code. The corrupt value has
a tag indicating a heap object (TAG_CONS = 16) but its payload points
to invalid memory.

## What we know (confirmed by tests)

1. **Interpreter works.** `test_nqueens_letrec_no_jit` (vm.jit_enabled=false)
   passes. The nqueens algorithm is correct.

2. **File pipeline works.** `cargo run -- --jit=1 nqueens.lisp` passes.
   Top-level `defn` forms use `analyze_file_letrec` (fixpoint signal
   inference) and get JIT-compiled correctly.

3. **Expression-level letrec + JIT = crash.** The Rust test uses `eval()`
   which compiles a `letrec` expression via `analyze_letrec`. The closures
   have captures (sibling references). Solo JIT compilation of these
   closures produces corrupt code.

4. **Simpler letrec + JIT works.** `test_jit_letrec_single_with_captures`,
   `test_jit_letrec_two_closures_with_lists`,
   `test_jit_letrec_forward_ref_multiarg` all pass with JIT. The nqueens
   pattern (5 closures, `append` + `cons` + list traversal) is needed to
   trigger the crash.

5. **Even 4-queens crashes.** `solve-nqueens 4` (only 2 solutions, minimal
   iterations) also SIGSEGV. It's not a depth or iteration count issue.

6. **Letrec signal fix is correct and necessary.** Without it, forward
   refs default to `Signal::yields()` → `SuspendingCall` → JIT rejects.
   The fix seeds `signal_env` with `Silent` for pre-bound letrec bindings.
   See `src/hir/analyze/binding.rs`.

7. **jit_rotation_base is not the cause.** Resetting it before the test
   doesn't help. Refuted.

8. **The JIT-to-JIT fast path in `elle_jit_call` is not the sole cause.**
   Still crashes with the interpreter fallback path (though removing the
   fast path changes the crash characteristics).

## Hypotheses not yet tested

- **Pool rotation during JIT self-tail-call frees caller-allocated values.**
  `check-safe-helper` self-recurses with `(rest remaining)`. The JIT
  self-tail-call path calls `rotate_pools_jit()`. If `remaining` (or the
  cons cells it points to) was allocated in a pool that rotation frees,
  the next iteration reads freed memory. This was hypothesized but the
  test with simple self-recursion + lists (`test_jit_self_tail_call_with_list_rotation`)
  passed, so the issue may require the specific nqueens call pattern.

- **JIT solo compilation with many captures miscomputes env offsets.**
  Nqueens closures capture 3-4 sibling closures. The JIT loads captures
  from `env_ptr[idx * 16]`. If the capture index mapping differs between
  the LIR and the JIT, wrong values are loaded. This hasn't been tested.

- **The `build_closure_env_for_jit` function builds a merged env (captures
  + params) for the interpreter path, but the JIT-to-JIT path uses
  `closure.env` (captures only).** If a function runs via interpreter first
  (with merged env) and then gets JIT-compiled, the JIT code might read
  stale capture data from a previous env layout. This hasn't been tested.

## Changes on this branch (cumulative)

### Committed and pushed

| Commit | Description |
|--------|-------------|
| P0 (96c0451d) | lib/sdl.lisp P0, JIT polymorphic+SuspendingCall, escape fix |
| P1 (60a31a2f) | textures/images/TTF/geometry; upvalue u8→u16 |
| P2 (512f25c0) | audio/input/clipboard/misc; Call+MakeClosure u8→u16 |
| 4509bc1f | (reverted P0 JIT — restored in later commit) |
| 300ea535 | escape analysis: tail_arg_is_safe for immediate callees |
| f74b33a2 | letrec signal seeding, JIT lbox restore, new tests |

### Files changed from main

| File | Change |
|------|--------|
| `src/hir/analyze/binding.rs` | Letrec signal seeding (Silent for forward refs) |
| `src/lir/lower/escape.rs` | `tail_arg_is_safe` + P0 tail-call escape fix |
| `src/lir/types.rs` | `has_suspending_call()` method |
| `src/jit/translate.rs` | P0 SuspendingCall impl + lbox auto-unwrap restored |
| `src/jit/calls.rs` | P0 num_params in metadata |
| `src/jit/compiler.rs` | P0 polymorphic acceptance + num_params |
| `src/jit/suspend.rs` | P0 spill buffer split (params/locals/operands) |
| `src/vm/jit_entry.rs` | P0 debug formatting |
| `src/compiler/bytecode.rs` | u8→u16: upvalue, call, makeclosure |
| `src/lir/emit/mod.rs` | u8→u16: upvalue, call, makeclosure |
| `src/vm/call.rs` | u8→u16: call arg count |
| `src/vm/closure.rs` | u8→u16: makeclosure captures |
| `src/vm/mod.rs` | u8→u16: synthetic bytecode |
| `src/vm/variables.rs` | u8→u16: upvalue index |
| `src/config.rs` | P0 comment cleanup |
| `src/ffi/to_c.rs` | P0 nil→null fix |
| `src/value/repr/accessors.rs` | P0 syntax nil/empty-list fix |
| `src/vm/core.rs` | P0 comment cleanup |
| `src/wasm/*` | P0 wasm changes |
| `lib/sdl.lisp` | P0-P2 SDL bindings |
| `tests/` | Regression tests for all fixes |

## How to reproduce

```bash
cargo test --test lib -- test_jit_mutual_recursion_nqueens_small
# → SIGSEGV

cargo test --test lib -- test_nqueens_letrec_no_jit
# → passes (JIT disabled)
```

## Next steps

1. Write a test that checks whether JIT capture index mapping matches
   the LIR for nqueens closures with multiple captures.

2. Add runtime validation in the JIT entry path to assert that captured
   values have valid tags before executing JIT code.

3. Compare the env layouts between interpreter and JIT paths for the
   specific nqueens closures to find the offset mismatch.

## SDL3 plan

P0-P2 complete. P3 (gamepad, async I/O, cursor, dialogs) blocked on
JIT fix. `lib/sdl.lisp` is at 935 lines with 167 exports.
