# The JIT code map

<!-- audited: 2026-10-20 -->

A table of every compiled function's entry address and name, for a reader who holds a native stack with JIT frames in it.

## The table

Native samplers (`/usr/bin/sample`, `eu-stack`) cannot name JIT frames: the
code lives in anonymous Cranelift mappings, so a wedged thread's stack shows
`??? (in <unknown binary>)` exactly where the answer is. The registry closes
that gap. Every successful compile, on every thread, records
`(entry address, label)` in one process-global table
([registry.rs](../../src/jit/registry.rs)).

The label is the function's declared name when one exists, else its
smallest-offset source location (`ClosureTemplate::display_label`). Lowering
names only a lambda a `def` or `let` binds, so the location is what identifies
the rest to a reader. The table only grows; entries are never removed,
because a stack captured at any time may reference code whose `JitCode` has
since been dropped.

## Reading it

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
PC alone cut the literal off.

The query answers `nil` when the address is malformed, lies past every
registered block, or its own 16 bytes are not resident. Another line of the
window whose page is gone renders `(unmapped)`: a photograph can carry an
address whose module has since been dropped, and the query must not fault on
it.

The runner prints the peek for each sampled `???` frame beside the map. The
map names the function the frame belongs to, and the peek shows the
instructions the sampled PC is actually parked on, which is what separates
the code the compiler emitted from the bytes the core is executing. A sampler
that parks every sample of a busy thread on ONE address is showing a
single-instruction loop; on AArch64 the word `0x14000000` is `b .` — an
unconditional branch to itself.

Tests: [jit-registry.lisp](../../tests/impl/jit-registry.lisp) and the unit
tests in [registry.rs](../../src/jit/registry.rs).

---

## See also

- [jit.md](jit.md) — the JIT whose compiles fill the table
