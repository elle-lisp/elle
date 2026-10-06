# Debugger: what a paused fiber shows

<!-- audited: 2026-10-06 -->

The debug information a code object carries, and the primitives that read a
paused fiber through it.

This document is part of the [debugger design](../debugger.md), and every name
it introduces is proposed unless it says the tree has it today.

## Debug information

A parked frame reaches everything its function's code object holds. `Code`
wraps the function's `ClosureTemplate` ([src/value/code.rs](../../src/value/code.rs)), and
`BytecodeFrame.code` is always live, so inspection reads the template
through the frame. The frame's parked closure register is a possibly-dead
borrow that inspection must never dereference (see the register
invariants in [impl/vm.md](../impl/vm.md)).

| Template field | Maps |
|----------------|------|
| location table | bytecode offset → file, line, column |
| `origin` | the lambda's source span |
| `lir` | SSA, CFG, yield points, call sites; present on a nested lambda's payload, absent on one built from bare `Bytecode` |
| `name` | the binding the lambda was defined under |

Two facts qualify the table. First, the location table is sparse: the
emitter records one entry per LIR instruction and one per block terminator,
not one per bytecode offset ([src/lir/emit/mod.rs](../../src/lir/emit/mod.rs)), and drops all-zero
synthetic spans (`record_location`, [src/compiler/bytecode.rs](../../src/compiler/bytecode.rs)), so
macro-generated code is unmappable. The template stores the entries sorted
by offset (`LocationTable`, [src/value/closure/payload.rs](../../src/value/closure/payload.rs)), and its lookup
answers an exact offset; inspection adds the lookup that resolves an ip to
the nearest preceding entry. Second, `name` is `None` for a lambda that no
`def` or `let` binds, and for code lowered without a symbol table. Lowering
takes the name from the binding (`binder_name`, [src/lir/lower/setup.rs](../../src/lir/lower/setup.rs)) into
`LirFunction.name`, and the template copies it from there
(`TemplateProto::nested_lambda`, [src/value/closure/proto.rs](../../src/value/closure/proto.rs)).

Two additions, both on the template, flowing the same path as the location
table:

- **`local_names`** — `(name, place, index)` entries for everything a
  frame binds. The lowerer's `binding_to_slot` map (declared in
  [src/lir/lower/mod.rs](../../src/lir/lower/mod.rs), filled in [src/lir/lower/emitops.rs](../../src/lir/lower/emitops.rs)) has the data
  and dies before emit; it moves onto `LirFunction`. The place is
  required because bindings live in two address spaces whose indices
  overlap, and in three shapes. A plain local lives in its stack
  slot. An in-lambda mutated-or-captured local is env-celled: its
  reserved stack slot stays `nil` and the value lives in an env cell
  (`allocate_slot_routed` mints the env index). A `letrec` binding
  with a compiled cell — and any captured binding outside a lambda —
  keeps a cell *in* its stack slot, one dereference behind the slot.
  The entry records which shape holds the value, so inspection reads
  the right address space and unwraps only real cells. Parameters
  ride the same table with an env place — they live after the
  captures in the env, not in stack slots, and without entries for
  them a frame's `:locals` would omit every argument. The
  per-function scratch slot (`discard_slot`) holds garbage between
  uses and is excluded; compiler temporaries get a `nil` name. Within
  each address space slots are allocated monotonically and never
  recycled, so one name per index is exact — no ip-ranged table is
  needed. Bindings the lowerer constant-folds away have no slot and
  do not appear.
- **the `Bytecode` local count** — the top-level, `eval`, and module-import
  paths build their template from bare `Bytecode` through
  `Bytecode::into_proto` ([src/compiler/bytecode.rs](../../src/compiler/bytecode.rs)). That copies the
  location map but no local count, and `TemplateProto::new` sets
  `num_locals` to 0, so those code objects claim zero locals while their
  prologue reserves slots. `Bytecode` gains `num_locals`; without it,
  top-level locals render as operand-stack junk. Copying it also arms the
  debug-build locals-integrity assertion for these frames — it is vacuous
  while `reserved_locals` is 0 — which may surface latent violations; the
  phase's tests cover top-level pauses for exactly this reason.

## Inspection primitives

Inspection operates on a fiber whose status is `:new`, `:paused`,
`:dead`, or `:error`. The keyword is `:paused` — [tests/lang/fuel.lisp](../../tests/lang/fuel.lisp)
pins it. Inspecting an `:alive` fiber is a *checked* error, not a
convention: a running fiber's handle slot is empty and an unchecked
borrow panics ([src/value/fiber/handle.rs](../../src/value/fiber/handle.rs)). The checked forms
(`try_with`/`try_with_mut`) exist there; inspection uses them. `:new`
and `:dead` fibers have no suspended chain; `fiber/frames` returns
`[]` for them.

| Primitive | Signature | Returns |
|-----------|-----------|---------|
| `fiber/frames` | `(fiber) → array` | frame structs, innermost first |
| `fiber/trace` | `(fiber) → array` | call-site records from `Fiber.call_stack` |
| `fiber/disasm` | `(fiber index) → array` | disassembly lines for frame `index` |

A frame struct:

```text
{:kind     :bytecode
 :name     "worker"          # the template name, or "<toplevel>"
 :file     "src/thing.lisp"  # nearest location_map entry at or before :ip
 :line     42
 :col      7
 :ip       118               # bytecode offset
 :locals   [("acc" 10) ("xs" [1 2 3])]   # via local_names places; parameters included
 :stack    [...]}            # operand stack above the locals
```

The chain is the fiber's `suspended` vec, and not every entry is
bytecode. A `FiberResume` link — a child fiber suspended through
`defer` or `protect`, or any child whose uncaught non-terminal signal
passed through — renders as `{:kind :fiber :fiber f}`; callers
recurse with `fiber/frames` on `f`. Tail calls trampoline in place,
so a run of tail calls appears as one frame holding the last
callee's code. Frame count is not logical call depth.

Locals occupy stack slots `[0, reserved_locals)` of the parked frame's
stack — closure bodies execute on a fresh stack with base 0. A celled
local's value is read through its env cell, per its `local_names`
place; its stack slot is not the value.

**Errors keep one frame.** On `:error` the VM parks exactly one
resumable activation and drops the rest ([src/vm/call/inner.rs](../../src/vm/call/inner.rs)
returns without parking the caller chain). Which frame survives is
path-dependent: a first resume parks the fiber body's root activation
([src/vm/fiber/resume.rs](../../src/vm/fiber/resume.rs)); a later resume parks the innermost
re-entered frame and drops the *outer* parked frames
([src/vm/core/resume.rs](../../src/vm/core/resume.rs)). `fiber/frames` on an errored fiber returns
that single resumable frame. `fiber/trace` compensates:
`Fiber.call_stack` survives exactly the error path — the suspend path
pops it — and holds one record per *interpreter* closure call site
(natives never push one; the native-tier dispatches pop theirs). Each
record carries a name and an ip resolved through the record's own
`location_map` — names and locations, no locals. Phase 3 upgrades this under the debug flag: the
error path parks frames exactly as suspension does, so error
snapshots carry every activation and resume delivers the recovery
value to the error point. Production error handling (restarts) is
unaffected — the flag is off.

**Region rule.** Inspection primitives declare `RegionEffect::Fresh`:
the result allocates in a region minted fresh for the call, and the
allocation scan increfs every cross-region reference the result
embeds ([src/value/fiberheap/regionstore/alloc.rs](../../src/value/fiberheap/regionstore/alloc.rs)), balanced by the
caller's normal release. That pins the parked values' regions for the
result's lifetime without touching the fiber. The terminal-result pin
from the swap protocol is the wrong model here: it is one-shot — its
releases are the free-time signal scan and, for a restarted `:error`
fiber, the displacement release (`release_displaced_terminal_signal`)
— and copying it would leak one incref per call in a stepping loop. Snapshot values are shared, not
copied — if the debuggee resumes and mutates an array, the snapshot
sees the mutation. Inspection allocates nothing into the debuggee's
regions; this also keeps observation invisible to replay (see
[recording and replay](replay.md)). Tests must cover
inspect-then-drop-fiber-then-use ordering.

`fiber/disasm` wires the disassembler
([src/compiler/bytecode/disasm.rs](../../src/compiler/bytecode/disasm.rs)) to a frame's `Code`. The current
entry point takes raw bytes and bakes offsets into strings; it gains
a structured `(offset, text)` form so the `*` marker on the frame's
ip needs no string parsing. The disassembler hand-duplicates operand
widths with a silent fallthrough, so Phase 1 adds an exhaustiveness
test pairing every operand-bearing opcode with its width — a new
opcode must not silently desync every following offset.

