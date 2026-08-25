# JIT

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
  VM (allocation, GC barriers, signal checks)

## Function selection

Functions become JIT candidates based on a hotness threshold
(default 10, controlled by `--jit=N`). The VM increments a counter on
each call; when it crosses the threshold, the function is compiled.

## Rejection tracking

Not all functions can be JIT-compiled. The JIT rejects functions that:

- Use features not yet implemented in the translator
- Fail Cranelift verification

**Negative-cache invariant.** A function whose compilation is rejected is
recorded in `jit_rejections` and **never re-submitted**: every subsequent
call falls through to the interpreter directly. The rejection is keyed by the
function's bytecode pointer (see "Cache identity" for why that key is sound),
so a re-submission could only ever reproduce the identical rejection — it is
pure wasted work.

This invariant is load-bearing under eager JIT. With `--jit=eager` the hotness
threshold is 0, so *every* call is "hot"; absent the negative cache, each call
to an un-jit'able function re-submits it to the background worker. A single
un-jit'able function called in a hot loop (e.g. stdlib `-`/`/`, which build a
rest-arg closure → `MakeClosure` rejection) then saturates the JIT worker
thread, re-compiling the same function thousands of times and burning CPU that
dwarfs the program's real work. The `jit/rejections` report exposes a per-
function `:attempts` count; the negative cache holds `attempts == 1` no matter
how many times the function is called.

## Cache identity

`jit_cache`, `jit_pending`, and `jit_rejections` key entries by the raw
address of the template's bytecode allocation (`bytecode.as_ptr()`). A raw
address identifies a function only while that allocation is alive: templates
are ordinary reclaimable data (region-allocated per `MakeClosure`, sharing
one `Rc<Vec<u8>>` bytecode buffer per lambda proto), so a dropped compile
unit frees its bytecode buffers, and a later compile can allocate a NEW
function's bytecode at a reused address. A cache entry that outlived its
template would then serve the old function's code to the new function —
which runs the wrong body with the new closure's env and args, producing
healthy-looking wrong values and no memory corruption.

The invariant that makes the address key sound: **every entry pins the
bytecode `Rc` it was keyed by** (`JitEntryPin`), from submission until the
entry is removed. A pinned allocation cannot be freed, so its address cannot
be reused, so a key collision cannot occur. The pin travels: recorded in
`jit_pending` at submit, moved into `jit_cache` (or `jit_rejections`) when
the result installs. The cost is that cached/rejected functions' bytecode
stays resident for the VM's lifetime — bounded by the amount of code the
program compiles, the same order as the retained native code itself.

An alternative — validating entries at hit time by content — was rejected:
it puts an O(bytecode) compare (or a hash plus per-template caching) on the
hot dispatch path to detect a situation the pin makes impossible.

Pinning tests: `src/vm/jit_entry/tests.rs`.

Native samplers (`/usr/bin/sample`, `eu-stack`) cannot name JIT frames: the
code lives in anonymous Cranelift mappings, so a wedged thread's stack shows
`??? (in <unknown binary>)` exactly where the answer is. The registry closes
that gap. Every successful compile — solo and batch, on every thread — records
`(entry address, label)` in one process-global table (`src/jit/registry.rs`).
The label is the function's declared name when one exists, else its
smallest-offset source location (`ClosureTemplate::display_label`) — lowering
names almost nothing, so the location is what actually identifies a function
to a reader. The table only grows; entries are never removed,
because a stack captured at any time may reference code whose `JitCode` has
since been dropped.

`(vm/query "jit/map" nil)` renders the table as one `0x<addr> <name>` line per
entry, sorted by address. The test runner prints it after the thread
photograph when a form misses its deadline (`src/test.lisp`,
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

## Yield-through-call

For functions that call other functions which might yield, the JIT
collects yield-site metadata during LIR emission. This enables proper
save/restore sequences so a yielded fiber can resume into JIT code.

## CLI flags

```text
--jit=0       Disable JIT entirely
--jit=N       Compile after N-1 calls (default: --jit=11, threshold 10)
--jit=1       Compile on first call
--stats       Print compilation stats on exit
```

## Files

```text
src/jit/compiler.rs    JitCompiler, module management
src/jit/translate.rs   FunctionTranslator, LIR → Cranelift IR
src/jit/code.rs        JitCode wrapper
src/jit/vtable.rs      Runtime helper dispatch table
src/jit/dispatch.rs    JIT dispatch integration with VM
```

---

## See also

- [impl/lir.md](lir.md) — LIR that the JIT translates
- [impl/vm.md](vm.md) — VM fallback and dispatch
- [impl/bytecode.md](bytecode.md) — bytecode alternative
- [impl/mlir.md](mlir.md) — MLIR tier-2 path consulted before Cranelift
- [impl/wasm.md](wasm.md) — WebAssembly backend
- [impl/gpu.md](gpu.md) — GPU compute via SPIR-V + Vulkan
- [impl/differential.md](differential.md) — cross-tier agreement testing
