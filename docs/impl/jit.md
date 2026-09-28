# JIT

<!-- audited: 2026-09-29 -->

The JIT compiles hot functions from LIR to native code using Cranelift.

## Architecture

```text
LIR → FunctionTranslator → Cranelift IR → Native code → JitCode
```

## Key types

- **`JitCompiler`** — manages the Cranelift `JITModule`, declares
  runtime helper symbols, tracks compilation stats
- **`FunctionTranslator`** — walks LIR basic blocks and instructions,
  emitting Cranelift IR
- **`JitCode`** — wraps the native function pointer; keeps the module
  alive for the code's lifetime
- **`RuntimeHelpers`** — extern symbols the JIT calls back into the
  VM (allocation, region accounting, calls, signal checks)

## Memory flags on emitted loads

Every load the translator emits reads memory the runtime owns and has already
validated: an argument array, a closure environment, a spill slot. Such an
access is aligned and cannot trap, which Cranelift spells
`MemFlagsData::trusted()`.

Cranelift keeps the flag data in a table on the function
(`func.dfg.mem_flags`) and puts a `MemFlags` handle — an index into that table
— on the instruction. `InstBuilder::load` interns for its caller, so the
translator passes `MemFlagsData` by value and holds no handle of its own. That
is worth keeping: a handle means "trusted" only inside the function whose
table minted it, so a handle cached across functions would index a table where
the same slot holds different flags, or nothing at all.

`load_value_slot` ([translate.rs](../../src/jit/translate.rs)) is where the JIT
names those flags.
It emits both halves of a `Value` — tag at `+0`, payload at `+8` — so the
16-byte stride is written once, from `size_of`/`offset_of` rather than as a
literal.

## Stores into stack slots

The JIT hands the runtime a pointer to consecutive `Value`s in a stack slot in
several places: the arguments of a call and of a tail call, the elements of an
array, the captures of a closure, the members of a region group, the pairs of a
parameter frame, and the locals and operands a suspend saves. Each writes a
`Value` as two words into a slot it sized itself.

`store_value_slot` ([translate.rs](../../src/jit/translate.rs)) is the one site
that writes such a `Value`. It takes the slot, the `Value`'s index, and the two
halves, and places the tag and the payload from `size_of` and `offset_of` on
`Value`, as `load_value_slot` does for a read.

Cranelift has no `stack_store` instruction. The builder method that keeps the
name takes the pointer type, emits a `stack_addr`, and stores through that
address with the flag `notrap` and no `aligned`. The JIT passes `I64` as the
pointer type, as it does for every `stack_addr`. It declares its slots with
one-byte alignment, so it cannot claim more. The two tables of the
abandoned-frame walk, and the `JitCtx` slot in the prologue, use the same
method directly.

`FunctionBuilder::finalize` takes the target's frontend configuration, which the
module holds. `finalize_function` ([translate.rs](../../src/jit/translate.rs))
takes the builder and the module and passes `module.target_config()`, so no
caller builds the configuration.

## Arithmetic: the tag-check diamond, and skipping it

A binary operation, a comparison and a negation each compile to a diamond
([fastpath.rs](../../src/jit/fastpath.rs)): test both tags for `TAG_INT`, then either the native
integer instruction or a call to the runtime helper, merging on a two-parameter
block. Division and remainder add a second test for a zero divisor.

An instruction carrying `OperandProof::Int` skips the tag test and the helper
call, leaving the integer instruction inline with no branch and no merge
([lir.md](lir.md) has where the proof comes from). Division keeps its zero test:
Cranelift's `sdiv` traps rather than returning a value, and the proof speaks only
about the operands' type.

Nothing else changes. The elided path is the one the tag test would have chosen,
because the operands are integers.

## Function selection

Functions become JIT candidates based on a hotness threshold, ten calls by
default, which `(vm/config-set :jit N)` changes. The VM increments a counter on
each call; when it crosses the threshold, the function is compiled.

**A non-tail call is counted by whichever tier makes it.** The interpreter
counts in `try_jit_call`; compiled code counts in `elle_jit_call`, on the arm
that finds no compiled code for the callee. Both reach the counter through
`VM::profile_jit_candidate`, the one place a call is counted and a hot function
is submitted.

Counting the interpreter's calls alone stops promotion one level below whatever
has already compiled. A compiled caller no longer reaches its callees through
the interpreter, so a callee called from nowhere else never becomes hot. The
worker's latency masks that, because the caller keeps running interpreted while
Cranelift works. `--trace=syncjit` installs on the first call and leaves no such
window, so it is where the two policies are held to the same answer
([jit-compiled-caller-promotes-callee.lisp](../../tests/impl/jit-compiled-caller-promotes-callee.lisp)).

A tail call is counted by neither tier. It replaces the frame rather than
building one — `tail_call_inner` in the interpreter, the tail-call sentinel in
compiled code — so a function only ever reached in tail position stays
interpreted.

## How a call leaves compiled code

Every call a compiled function makes goes through a runtime dispatch helper:
`elle_jit_call` for a non-tail call, `elle_jit_tail_call` for a tail call. The
helper resolves the callee and looks it up in `jit_cache`, so a compiled caller
reaches a compiled callee without building an interpreter frame.

A tail call to the executing closure is the one call that needs no helper. It
updates the argument variables and jumps to the loop header, so self-recursion
in tail position is a native loop.

Compiled functions never call one another directly, in tail position or out of
it. Each compile owns its own Cranelift module, so a peer is not a function
that module can name. The helper is also what supplies the callee's
environment, checks its arity, counts call depth, and carries the tail-call,
yield and error protocol back to the caller; a direct call would have to
reproduce all of it
([clif.rs](../../src/jit/compiler/tests/clif.rs) pins the self-recursive case).

**A compiled call nests on the native stack until the stack runs low.**
`elle_jit_call` enters a compiled callee as a native call, so a compiled
recursion grows the thread's stack. When less than 512 KiB of that stack
remains, the helper runs the callee in the interpreter instead. The
interpreter keeps every deeper call on fiber frames and enters no compiled
code while the stack stays low ([vm.md](vm.md)). A recursion 100,000 deep
therefore completes with the JIT eager, as a rig sidecar sets it, with the
frames past the watermark interpreted.

## Rejection tracking

Not all functions can be JIT-compiled. The JIT rejects functions that:

- Use features not yet implemented in the translator
- Contain `MakeClosure`, or collect struct or named varargs
- Fail Cranelift verification

**Negative-cache invariant.** A function whose compilation is rejected is
recorded in `jit_rejections` and **never re-submitted**: every subsequent
call falls through to the interpreter directly. The rejection is keyed by the
function's bytecode pointer (see "Cache identity" for why that key is sound),
so a re-submission could only ever reproduce the identical rejection — it is
pure wasted work.

Eager JIT is where this invariant pays. With the JIT eager the hotness
threshold is 0, so *every* call is "hot"; absent the negative cache, each call
to an un-jit'able function re-submits it to the background worker. A single
un-jit'able function called in a hot loop (for example stdlib `-`/`/`, which build a
rest-arg closure → `MakeClosure` rejection) then saturates the JIT worker
thread, re-compiling the same function thousands of times and burning CPU that
dwarfs the program's real work. The `jit/rejections` report exposes a per-
function `:attempts` count; the negative cache holds `attempts == 1` no matter
how many times the function is called.

**Every failed compile is recorded**, whichever kind it is, so the negative
cache covers all of them. A refusal the translator plans for —
`UnsupportedInstruction` or `Polymorphic` — is recorded and says
nothing further. A Cranelift failure or an invalid-LIR result is a defect in
the compiler, so it is recorded and also printed on stderr, once. Every path
that takes a result classifies it through `VM::record_jit_failure`: the
background poll, the diagnostic drain, and the synchronous compile that
`--trace=syncjit` runs. A flag meant to show a codegen failure must not be the
one place that swallows it.

## Cache identity

`jit_cache`, `jit_pending`, and `jit_rejections` key entries by the raw
address of a code object's bytecode (`bytecode().as_ptr()`). A raw address
identifies a function only while that allocation is alive: bytecode lives in a
code object's payload, one per lambda blueprint, in a region the heap releases
when the last blueprint packed into it dies
([region/template.md](region/template.md)). So a dropped compile unit frees its
payload pages, and a later blueprint can land a NEW function's bytecode at a
reused address. A cache entry that outlived its code object would then serve
the old function's code to the new function — which runs the wrong body with
the new closure's env and args, producing healthy-looking wrong values and no
memory corruption.

The invariant that makes the address key sound: **every entry pins the code
object it was keyed by**, from submission until the entry is removed. The
pinned header holds its blueprint, the blueprint holds its cache entry, and
the cache entry holds the payload region — so the address cannot be reused and
a key collision cannot occur. The pin travels: recorded in `jit_pending` at
submit, moved into `jit_cache` (or `jit_rejections`) when the result installs.
The cost is that cached/rejected functions' payloads stay resident for the VM's
lifetime — bounded by the amount of code the program compiles, the same order
as the retained native code itself.

An alternative — validating entries at hit time by content — was rejected:
it puts an O(bytecode) compare (or a hash plus per-template caching) on the
hot dispatch path to detect a situation the pin makes impossible.

Pinning tests: [jit_entry/tests.rs](../../src/vm/jit_entry/tests.rs).

Native samplers (`/usr/bin/sample`, `eu-stack`) cannot name JIT frames: the
code lives in anonymous Cranelift mappings, so a wedged thread's stack shows
`??? (in <unknown binary>)` exactly where the answer is. The registry closes
that gap. Every successful compile, on every thread, records
`(entry address, label)` in one process-global table
([registry.rs](../../src/jit/registry.rs)).
The label is the function's declared name when one exists, else its
smallest-offset source location (`ClosureTemplate::display_label`). Lowering
names only a lambda a `def` or `let` binds, so the location is what identifies the rest
to a reader. The table only grows; entries are never removed,
because a stack captured at any time may reference code whose `JitCode` has
since been dropped.

`(vm/query "jit/map" nil)` renders the table as one `0x<addr> <name>` line per
entry, sorted by address. The test runner prints it after the thread
photograph when a form misses its deadline ([exec.lisp](../../src/test/exec.lisp),
`note-timeout-stacks`), so a sampled JIT frame resolves to the nearest
preceding entry — the registry records entry addresses, not sizes, and
Cranelift lays functions out contiguously enough for nearest-preceding to
name the frame.

`(vm/query "jit/peek" "0x<addr>")` renders a window of 32-bit words around a
JIT address: from 16 bytes before it (clamped to the nearest registered
entry) to 48 bytes after, four words per `0x<addr>: <w0> <w1> <w2> <w3>`
line. The window is sized for the AArch64 `LoadExtName` sequence
(`ldr rd, pc+8; b pc+16; .8byte target`): a PC parked just before that
sequence carries the call target inside the window, where four words at the
PC alone cut the literal off. The query answers `nil` when the address is
malformed, lies past every registered block, or its own 16 bytes are not
resident; another line of the window whose page is gone renders
`(unmapped)` — a photograph can carry an address whose module has since
been dropped, and the query must not fault on it. The runner prints the peek
for each sampled `???` frame beside the map: the map names the function the
frame belongs to, and the peek shows the instructions the sampled PC is
actually parked on, which is what separates the code the compiler emitted
from the bytes the core is executing. A sampler that parks every sample of a
busy thread on ONE address is showing a single-instruction loop; on AArch64
the word `0x14000000` is `b .` — an unconditional branch to itself.

## How a signal leaves compiled code

Compiled code has no resume point of its own. An `Emit` spills the frame's
locals and operands and calls `elle_jit_yield`, which installs the signal in
`fiber.signal`. For a suspending signal the helper also parks the frame as a
bytecode frame at the emit's resume ip, so the interpreter runs the
continuation when the fiber resumes. An error emit parks nothing, and the
frame runs the abandoned-frame walk before it returns
([unwind.md](region/unwind.md)).

Either way the function returns `YIELD_SENTINEL`, so the sentinel alone does
not say the function suspended. A reader of the sentinel reads the signal
first, and treats an error or a halt as one: `run_jit`, and the
`compile/run-on :jit` entry ([differential.md](differential.md)).

A call whose callee suspended is the other way out. The check after the call
finds the callee's signal and calls `elle_jit_yield_through_call`, which parks
this frame at the call's resume ip behind the callee's frames. A tail call
parks the same way at the ip past the tail call, where the interpreter's
fall-through block starts, so the resume runs the releases the frame still owed
([park.md](region/park.md)). The emitter records each emit's resume ip, and each
call's and each tail call's, with the operand stack at that point, during LIR
emission. These two helpers read them.

## Configuration

[config.md](../config.md) owns the builds, the JIT threshold, and the policy the
binary starts from. The JIT is the optimizing tier of the default build; a
build with the `mlir` or `wasm` feature carries that tier instead and runs no
JIT. No flag turns the JIT off or makes it eager: the rig does both, for one
implementation test, through a sidecar ([rig](../../rig/overview.md)).
`--dump=stats` prints this tier's compiled and rejected counts on exit, with
the call count behind each rejection.

## Files

| File | Holds |
|------|-------|
| [src/jit/compiler.rs](../../src/jit/compiler.rs) | `JitCompiler`, module management |
| [src/jit/translate.rs](../../src/jit/translate.rs) | `FunctionTranslator`, LIR → Cranelift IR |
| [src/jit/code.rs](../../src/jit/code.rs) | The `JitCode` wrapper |
| [src/jit/vtable.rs](../../src/jit/vtable.rs) | The runtime helper dispatch table |
| [src/jit/dispatch.rs](../../src/jit/dispatch.rs) | JIT dispatch integration with the VM |

---

## See also

- [impl/lir.md](lir.md) — LIR that the JIT translates
- [impl/vm.md](vm.md) — VM fallback and dispatch
- [impl/bytecode.md](bytecode.md) — bytecode alternative
- [impl/mlir.md](mlir.md) — MLIR, the tier an `mlir` build carries instead of this one
- [impl/wasm.md](wasm.md) — WebAssembly backend
- [impl/gpu.md](gpu.md) — GPU compute via SPIR-V + Vulkan
- [impl/differential.md](differential.md) — cross-tier agreement testing
