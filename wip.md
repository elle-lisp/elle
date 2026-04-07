# WIP: JIT rotation base fix — DONE

## Status

`make test` passes. The nqueens SIGSEGV (`test_jit_mutual_recursion_nqueens_small`)
is fixed. All nqueens tests pass. No remaining known defects.

## Root cause

`jit_rotation_base` is a single field on `FiberHeap`. The JIT self-tail-call
optimization calls `rotate_pools_jit()`, which sets this field on its first
invocation and rotates relative to it on subsequent calls. The field was
never saved/restored across JIT function calls.

When an inner JIT function (e.g. `check-safe-helper`) self-tail-called,
it set `jit_rotation_base`. After it returned, the field persisted. When
the outer function (e.g. `try-cols-helper`) later self-tail-called and
called `rotate_pools_jit()`, it rotated relative to the inner's stale
base mark — sweeping the outer's live cons cells into the swap pool and
freeing them one rotation later.

## Fix (commit 7bab9ee9)

Save `jit_rotation_base` to `None` before every JIT function call;
restore after return. Two call sites:

- `call_jit` in `src/vm/jit_entry.rs` (initial JIT entry from interpreter)
- `elle_jit_call` in `src/jit/calls.rs` (JIT-to-JIT fast path)

New FiberHeap methods: `save_jit_rotation_base()` / `restore_jit_rotation_base()`.

## Hypothesis testing

| Test | Without fix | With fix |
|------|------------|----------|
| `test_jit_nested_rotation_base_two_deep` | FAILED (corrupt values) | ok |
| `test_jit_nested_rotation_base_three_deep` | SIGKILL (OOM/crash) | ok |
| `test_jit_single_self_tail_rotation_control` | ok | ok |
| `test_jit_mutual_recursion_nqueens_small` | SIGSEGV | ok |

The control (non-nested) test passes regardless, confirming the bug
requires nested JIT calls where the inner function has a self-tail-call
loop.

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
| 7bab9ee9 | fix: save/restore jit_rotation_base across nested JIT calls |

### Files changed from main

| File | Change |
|------|--------|
| `src/value/fiberheap/mod.rs` | `save/restore_jit_rotation_base` methods |
| `src/jit/calls.rs` | Save/restore in `elle_jit_call` JIT-to-JIT path |
| `src/vm/jit_entry.rs` | Save/restore in `call_jit` initial entry |
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

## SDL3 plan

P0-P2 complete. P3 (gamepad, async I/O, cursor, dialogs) ready to proceed.
`lib/sdl.lisp` is at 935 lines with 167 exports.
