# The JIT module

<!-- audited: 2026-11-24 -->

Where each stage of the Cranelift JIT lives, how the translator lays out its variables, and the rules every runtime helper keeps.

[The JIT design](../../docs/impl/jit.md) owns selection, the worker, how a call
and a signal leave compiled code, the self-tail-call loop, rejection and cache
identity. [How compiled code builds a closure](../../docs/impl/jit-closure.md)
owns `MakeClosure` and the frame a side exit rebuilds.
[JIT fuel](../../docs/impl/jit-fuel.md) owns the charge, the kill and the pause.
[Signals and JIT](../../docs/signals/jit.md) says what a function's signal
decides here, and [mod.rs](mod.rs) states the calling convention of a compiled
function.

## What a compile refuses

`JitCompiler::compile` refuses what `LirView::jit_refusal`
([view.rs](../lir/code/view.rs)) names, and that function's docstring gives
each reason. The translator lowers every other instruction. `clif_text` does
not ask, so the `Eval` arm in [async_ops.rs](translate/instr/async_ops.rs)
refuses as well.

## The translator's variables

Each LIR register `r` is two Cranelift variables: the tag at `2r` and the
payload at `2r + 1` ([translate.rs](translate.rs)). The translator numbers
three runs of logical slots, and doubles each slot the same way:

| Slot | Holds |
|------|-------|
| `r`, below `num_regs` | LIR register `r` |
| `arg_var_base + i` | parameter `i` as the call bound it: the argument, or a fresh cell around it |
| `local_var_base + s` | stack slot `s` |

`arg_var_base` is `num_regs`, and `local_var_base` is `arg_var_base` plus the
parameter count, with a rest parameter counted once. `bind_params`
([params.rs](translate/params.rs)) binds the parameter variables in the
prologue and again at each self tail call.

Each parameter owns the stack slot of its index ([the lowerer](../lir/lower/overview.md)).
The lowered body copies an uncelled parameter into that slot at entry and reads
it there with `LoadLocal`. A celled parameter's slot holds nil. The environment
index `num_captures + i` names the variable the call bound. A `LoadCapture` or
`StoreCapture` there goes through the cell where `capture_params_mask` sets
bit `i`, and a `LoadCaptureRaw` reads the variable as it is.

An environment index past the parameters, `num_captures + num_params + l`,
names the variable of stack slot `num_params + l`. Where `capture_locals_mask`
sets bit `l`, that variable holds the local's cell. A `LoadCapture` or
`StoreCapture` goes through the cell, and a `LoadCaptureRaw` reads the cell
itself. The interpreter keeps nil in that stack slot instead, and
`rebuild_frame` moves the cell to the environment when the frame parks
([jit-closure.md](../../docs/impl/jit-closure.md)).

The loop header runs `init_locally_defined_vars` on every trip. It sets each
stack-slot variable to nil, or to a fresh cell where `capture_locals_mask` sets
the local's bit. A side exit spills the parameter variables, then every
stack-slot variable, then the site's operands
([spill.rs](translate/spill.rs)).

## The runtime helpers

Compiled code reaches Rust only through `extern "C"` functions named
`elle_jit_*`. A `Value` crosses as two `I64`s, the tag and the payload, and
comes back as a `JitValue` ([value.rs](value.rs)).
[vtable.rs](vtable.rs) declares every helper once per compiler, and
[symbols.rs](vtable/symbols.rs) says what a new helper adds.

| Helpers | File |
|---------|------|
| Arithmetic, bitwise, unary and conversion | [arith.rs](runtime/arith.rs) |
| Comparison | [compare.rs](runtime/compare.rs) |
| Tag tests, `type-of`, and the intrinsic data operations | [ops.rs](runtime/ops.rs) |
| Pairs, arrays, heap literals, and the collection predicates | [build.rs](data/build.rs) |
| `MakeClosure` | [closure.rs](data/closure.rs) |
| The destructuring and or-nil accessors | [destructure.rs](data/destructure.rs) |
| Capture cells, environment stores, and rest lists | [cell.rs](data/cell.rs) |
| A call, by the callee's kind | [callops.rs](calls/callops.rs) |
| A tail call, and a spliced call of either kind | [tail.rs](calls/callops/tail.rs), [arraycall.rs](calls/callops/arraycall.rs) |
| A native call, a primitive's signal, a capability denial, and the owned-argument retain | [calls.rs](calls.rs) |
| A call's hand-off word | [handoff.rs](calls/handoff.rs) |
| The side exits and the signal check | [suspend.rs](suspend.rs) and [suspend/](suspend/) |
| Region counts, adoption, and group frees | [region.rs](dispatch/region.rs) |
| The region-remap frame | [remap.rs](dispatch/remap.rs) |
| Struct access | [structops.rs](dispatch/structops.rs) |
| Array push and extend, parameter frames, and the signal bound | [dispatch.rs](dispatch.rs) |

[dispatch.rs](dispatch.rs) re-exports the `calls`, `data` and `suspend`
helpers, so the symbol table names every helper `dispatch::` or `runtime::`.

Every helper keeps these rules:

- A helper that raises sets `vm.fiber.signal` and answers nil. The check after
  the site finds the signal ([checks.rs](helpers/checks.rs)). A few helpers
  raise nothing. The arithmetic and comparison helpers in [runtime/](runtime.rs),
  and `elle_jit_first` and `elle_jit_rest`, print a type error or an overflow
  to stderr and answer nil. The intrinsic helpers in [ops.rs](runtime/ops.rs)
  panic where an intrinsic's contract breaks.
- Only `elle_jit_yield`, `elle_jit_yield_through_call` and
  `elle_jit_fuel_pause` write `fiber.suspended`, and `elle_jit_kill` writes the
  signal alone. Compiled code writes neither field. It reads the signal through
  `elle_jit_has_signal`.
- A helper that allocates reaches the driving VM's heap through the VM pointer
  or the `JitCtx` its call passes ([ctx.md](../../docs/impl/region/ctx.md)).
  No helper reads a heap from the thread.

## Files

| File | Holds |
|------|-------|
| [mod.rs](mod.rs) | The types every stage shares, `JitError`, `JitRejectionInfo` and the calling convention |
| [compiler.rs](compiler.rs) and [compiler/](compiler/) | `JitCompiler`: the module, the verifier and abandon flags, the prologue, the block walk, and when each block seals |
| [translate.rs](translate.rs) and [translate/](translate/) | `FunctionTranslator`: each instruction and terminator, the parameter binding, the spills, the fuel charge |
| [helpers.rs](helpers.rs) and [helpers/](helpers/) | What the translator reaches for: constants, helper-call shapes, register pairs, the signal check and the park |
| [fastpath.rs](fastpath.rs) | The tag-check diamond of each arithmetic, comparison and unary operation |
| [code.rs](code.rs) | `JitCode`: the entry pointer and what it must outlive |
| [value.rs](value.rs) | `JitValue` and the two sentinels |
| [children.rs](children.rs) | The child table a compile reads `MakeClosure` headers from |
| [worker.rs](worker.rs) | The worker thread, `JitTask` and `prepare_task` |
| [stats.rs](stats.rs) | `JitStats`: a VM's compiles and their cost |
| [registry.rs](registry.rs) | The code-address registry ([jit-map.md](../../docs/impl/jit-map.md)) |
| [vtable.rs](vtable.rs) and [vtable/](vtable/) | Each helper's symbol and signature |

Each module keeps its unit tests in a `tests` module beside it. The
hand-built LIR corpus is [jit.rs](../../tests/integration/jit.rs).

To tell a codegen fault from a race at the worker boundary, run under
`--trace=syncjit`. [jit_entry.rs](../vm/jit_entry.rs) says how to read the
answer.
