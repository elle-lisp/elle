# Debugger: what a paused fiber shows

<!-- audited: 2026-11-21 -->

The debug information a code object carries, and the primitives that read a
paused fiber through it.

This document is part of the [debugger design](../debugger.md), and every name
it introduces is proposed unless it says the tree has it today.

## Debug information

A parked frame reaches everything its function's code object holds. `Code`
wraps the function's `ClosureTemplate` ([src/value/code.rs](../../src/value/code.rs)),
and `BytecodeFrame.code` is always live, so inspection reads the template
through the frame. The frame's parked closure register is a possibly-dead
borrow, and inspection never dereferences it ([the VM](../impl/vm.md) states
the register's rules).

| Template field | Maps |
|----------------|------|
| location table | bytecode offset → file, line, column |
| `origin` | the lambda's source span |
| `lir` | SSA, CFG, yield points, call sites; present on a nested lambda's payload, absent on one built from bare `Bytecode` |
| `name` | the binding the lambda was defined under |

Two facts qualify the table. First, the location table is sparse. The emitter
records one entry per LIR instruction and one per block terminator, not one
per bytecode offset ([src/lir/emit/mod.rs](../../src/lir/emit/mod.rs)).
`Bytecode::record_location` drops an all-zero synthetic span
([src/compiler/bytecode.rs](../../src/compiler/bytecode.rs)), so an
instruction lowered from one has no entry. The template stores the entries
sorted by offset (`LocationTable`,
[src/value/closure/payload.rs](../../src/value/closure/payload.rs)), and its
lookup answers an exact offset. Inspection adds the lookup that resolves an ip
to the nearest preceding entry.

Second, `name` is `None` for a lambda that no `def` or `letrec` binds, and for
code lowered without a symbol table. Lowering takes the name from the binding
(`binder_name`, [src/lir/lower/setup.rs](../../src/lir/lower/setup.rs)) into
`LirHead::name`. The payload copies it from the frozen function
(`PayloadParts::lambda`, [src/value/closure/arena.rs](../../src/value/closure/arena.rs)).
A `let` names nothing:

```lisp
(letrec [named (fn [] 1)]
  (assert (= (get (fn/signature named) :name) "named") "letrec names its lambda"))
(let [unnamed (fn [] 1)]
  (assert (nil? (get (fn/signature unnamed) :name)) "let names nothing"))
```

Two additions, both on the template, flow the same path as the location table.

**`local_names`** holds a `(name, place, index)` entry for everything a frame
binds. The lowerer's `binding_to_slot` map has the data, and it dies before
emit, so it moves onto `LirHead`. The map is declared in
[src/lir/lower/mod.rs](../../src/lir/lower/mod.rs) and filled in
[src/lir/lower/emitops.rs](../../src/lir/lower/emitops.rs). The place is
required because bindings live in two address spaces whose indices overlap,
and in three shapes:

- A plain local lives in its stack slot.
- An in-lambda local that a closure captures or code assigns is env-celled.
  Its reserved stack slot stays `nil`, and the value lives in an env cell, at
  the env index `allocate_slot_routed` mints.
- A `letrec` binding with a compiled cell, and any captured binding outside a
  lambda, keeps a cell *in* its stack slot, one dereference behind the slot.

The entry records which shape holds the value, so inspection reads the right
address space and unwraps only real cells. Parameters take entries with an
env place, because they live after the captures in the env, not in stack
slots. Without those entries a frame's `:locals` would omit every argument.

The scratch slot of a function (`discard_slot`) holds garbage between uses, so
it takes no entry, and a compiler temporary takes a `nil` name. Each address
space allocates its slots in order and never reuses one, so one name per index
is exact, and no table ranged by ip is needed. A primitive's binding has no
slot, so it has no entry.

**The `Bytecode` local count** closes a gap in the unit's entry. The
top-level, `eval` and module-import paths run a unit's entry, whose payload
`PayloadParts::entry` writes from bare `Bytecode`
([src/value/closure/arena.rs](../../src/value/closure/arena.rs)). That copies
the location map and no local count, so `num_locals` stays 0. Those code
objects claim zero locals while their prologue reserves slots, and top-level
locals would render as operand-stack junk. `Bytecode` gains `num_locals` to
close the gap.

Copying the count also arms the debug-build check that the locals stay intact
for these frames. The check is vacuous while `reserved_locals` is 0, so it may
find latent violations, and the phase's tests cover top-level pauses for that
reason.

## Inspection primitives

Inspection operates on a fiber whose status is `:new`, `:paused`, `:dead` or
`:error`. [tests/lang/fuel.lisp](../../tests/lang/fuel.lisp) pins the keyword
`:paused`. Inspecting an `:alive` fiber is a *checked* error. A running
fiber's handle slot is empty, and an unchecked borrow of it panics
([src/value/fiber/handle.rs](../../src/value/fiber/handle.rs)), so inspection
uses the checked forms `try_with` and `try_with_mut`. A `:new` or `:dead`
fiber has no suspended chain, and `fiber/frames` returns `[]` for it.

| Primitive | Signature | Returns |
|-----------|-----------|---------|
| `fiber/frames` | `(fiber) → array` | frame structs, innermost first |
| `fiber/trace` | `(fiber) → array` | call-site records from `Fiber.call_stack` |
| `fiber/disasm` | `(fiber index) → array` | disassembly lines for frame `index` |

`fiber/frames` declares `:inspect`, because it answers the values another
fiber's frames hold ([capability bits](../signals/bits.md)). A fiber denied
`:inspect` cannot read a paused fiber's locals, as it cannot read a closure's
captures. `fiber/trace` answers names and locations, and `fiber/disasm`
answers code, so neither declares the bit.

A frame struct:

```text
{:kind     :bytecode
 :name     "worker"          # the template name, or "<toplevel>"
 :file     "src/thing.lisp"  # the nearest location-table entry at or before :ip
 :line     42
 :col      7
 :ip       118               # bytecode offset
 :locals   [("acc" 10) ("xs" [1 2 3])]   # via local_names places; parameters included
 :stack    [...]}            # operand stack above the locals
```

The chain is the fiber's `suspended` vec, and not every entry is bytecode. A
`FiberResume` link is a child fiber suspended through `defer` or `protect`, or
any child whose uncaught non-terminal signal passed through. It renders as
`{:kind :fiber :fiber f}`, and a caller recurses with `fiber/frames` on `f`.
Tail calls trampoline in place, so a run of tail calls appears as one frame
holding the last callee's code. Frame count is not logical call depth.

Locals occupy stack slots `[0, reserved_locals)` of the parked frame's stack,
because every body starts on an empty operand stack ([the VM](../impl/vm.md)).
A celled local's value is read through its env cell, per its `local_names`
place; its stack slot is not the value.

**An error parks every frame.** On `:error` the VM parks the raising
activation and then each caller behind it, as a suspend does
([src/vm/execute/nested.rs](../../src/vm/execute/nested.rs)). So
`fiber/frames` on an errored fiber returns the whole chain, and a restart
delivers its value at the raising call while every caller runs on:

```lisp
(defn inspect-inner [] (+ 10 (error {:error :boom})))
(defn inspect-outer [] (+ 1 (inspect-inner)))
(let [f (fiber/new inspect-outer |:error|)]
  (fiber/resume f)
  (assert (= (fiber/resume f 5) 16) "the restart runs on through both callers"))
```

`fiber/trace` reads `Fiber.call_stack`, which holds one record per
interpreter closure call site. A native call pushes none, and a native-tier
dispatch pops its own. A suspend pops the record of each call it leaves, and
an error keeps them, so the records survive the error path alone. Each record
names the entered function and resolves the call's location through the
calling code object's location table
([src/value/fiber/frame.rs](../../src/value/fiber/frame.rs)). It carries no
locals.

**Region rule.** Inspection primitives declare `RegionEffect::Fresh`. The
result allocates in a region minted for the call, and the allocation scan
increfs every cross-region reference the result embeds
([src/value/fiberheap/regionstore/alloc.rs](../../src/value/fiberheap/regionstore/alloc.rs)).
The caller's normal release balances those references. That pins the parked
values' regions for the result's lifetime without touching the fiber.

The terminal-result pin of the swap protocol is the wrong model here, because
it is one-shot. Its releases are the free-time signal scan and, for a
restarted `:error` fiber, the displacement release
(`release_displaced_terminal_signal`). A copy of it would leak one incref per
call in a stepping loop.

Snapshot values are shared, not copied: if the debuggee resumes and mutates an
array, the snapshot sees the mutation. Inspection allocates nothing into the
debuggee's regions, which also keeps observation invisible to replay
([recording and replay](replay.md)). Tests must cover the order inspect, drop
the fiber, then use the result.

`fiber/disasm` wires the disassembler
([src/compiler/bytecode/disasm.rs](../../src/compiler/bytecode/disasm.rs)) to a
frame's `Code`. The current entry point takes raw bytes and bakes offsets into
strings. It gains a structured `(offset, text)` form, so the `*` marker on the
frame's ip needs no string parsing. The disassembler restates each opcode's
operand width by hand, and an opcode it does not list falls through silently.
So Phase 1 adds an exhaustiveness test that pairs every opcode that carries an
operand with its width, and a new opcode cannot silently desync every
following offset.
