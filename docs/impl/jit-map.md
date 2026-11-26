# The JIT code map

<!-- audited: 2026-11-26 -->

A table of every compiled function's entry address and name, for perf and for a reader of a native stack.

## The table

Native samplers (`/usr/bin/sample`, `eu-stack`) cannot name JIT frames: the
code lives in anonymous Cranelift mappings, so a wedged thread's stack shows
`??? (in <unknown binary>)` exactly where the answer is. The registry closes
that gap. Every successful compile, on every thread, records
`(entry address, label)` in one process-global table
([registry.rs](../../src/jit/registry.rs)).

The label is the function's declared name and the place it was written, as
`name@file:line:col`, and a function with no declared name gets the place
alone. The place is the origin `(meta/origin f)` reports. A function with no
origin falls back to `ClosureTemplate::display_label`, the name or the
smallest-offset source location.

Lowering names a `fn` only when it initializes a `def` or `letrec` binding,
and functions often share a name: the standard library binds many `letrec`
helpers called `go`. perf merges samples by label, so the place keeps two such
functions apart in a profile. The label names the compile alone. A rejection still reports the
function's declared name.

The table only grows; entries are never removed, because a stack captured at
any time may reference code whose `JitCode` has since been dropped.

## Reading it

`(vm/query "jit/map" nil)` renders the table as one `0x<addr> <label>` line per
entry, sorted by address. The test runner prints it after the thread
photograph when a form misses its deadline ([deadline.lisp](../../src/test/deadline.lisp),
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

Tests: [jit-registry.lisp](../../tests/impl/jit-registry.lisp), the unit
tests in [registry.rs](../../src/jit/registry.rs), and the compiler's
[tests](../../src/jit/compiler/tests/mod.rs).

## Profiling with perf

perf names the JIT's code without help from elle. When `PERF_BUILDID_DIR` is
set, cranelift-jit appends a `START SIZE NAME` line to `/tmp/perf-<pid>.map`
for each function it compiles, and `perf record` sets that variable for the
program it starts. The name is the label the table above holds. The file
outlives the process, because perf reads it at report time, so delete it after
you read the profile.

A whole stack needs frame pointers. DWARF unwinding stops at the first JIT
frame, because perf finds no unwind table for code in an anonymous mapping.
Cranelift's x86_64 backend opens a frame in every function it compiles, and
Rust code keeps a frame pointer only when built with
`-Cforce-frame-pointers=yes`. `make perf` builds the `perf` cargo profile with
that flag, and records the run of `FILE` with `--call-graph fp` and the verifier
off. `ARGS` holds the program's own arguments. Each program records to a file
of its own name, so a second profile does not overwrite the first, and
`make hotspot` records and then opens the profile:

```sh
make perf FILE=demos/fib/fib.lisp       # records target/perf-fib.data
make hotspot FILE=demos/fib/fib.lisp    # the same, then opens it in hotspot
```

The perf build carries no embedded blob, so a start that finds no warm-cache
image compiles core.lisp, prelude.lisp and stdlib.lisp, then stores one
([booting from an image](image/boot.md)). The recipe starts the perf build once
before it records. The recorded start then hydrates that image, and the
profile charges its boot a hydration rather than a compile.

Tests: [frames.rs](../../src/jit/compiler/tests/frames.rs) for the JIT's
frames, and [perf_target.rs](../../tests/integration/perf_target.rs) for the
recipe.

---

## See also

- [jit.md](jit.md) — the JIT whose compiles fill the table
