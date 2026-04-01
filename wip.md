# WASM Backend: Current State

## What was fixed (a6eca332)

### 1. include/include-file ✅
`splice_includes()` pre-processes user source before ev/run wrapping.
Both `include` and `include-file` now work in WASM mode.

### 2. Multi-yield resume chain ✅ (was "LBox parameter bug")
Root cause was NOT LBox-specific. Any function with two or more
yielding calls (e.g. two `println`) returned nil from the second
yield onward. The bug was in `drive_resume_chain`: when the innermost
suspension frame re-yielded, stale outer frames from the first yield
remained in the deque and were consumed with the wrong resume value.
Fix: evict stale frames after a re-yield.

### 3. Unified call emission ✅
Removed `emit_yield_through_check` (the old single-block fast path
that read signal from memory[0..4]). Suspending closures now always
use the loop/br_table dispatcher with `emit_call_suspending`.

### 4. Timeout ✅
smoke-wasm timeout raised from 180s to 300s.

## Remaining known issues

### WASM-only timeouts (pre-existing)
Large modules (advanced, coroutines, functional, pipeline, sync)
time out at 30s due to Cranelift compilation cost. They pass with
the Makefile's 300s timeout. redis.lisp also times out (needs a
running redis server anyway).

### port-edge-cases.lisp hangs (pre-existing)
TCP server + fiber interaction hangs in WASM mode. Same behavior
before and after the fixes. Likely a fiber scheduler issue with
nested I/O + accept loops.

### JIT LBox bug (pre-existing, NOT WASM)
`jit-lbox-param-repro.lisp` and `jit-lbox-param-noyield.lisp` fail
when JIT is enabled (any backend). After ~9 iterations the JIT
compiles the hot function and produces "JIT type error: expected
cell". This is a Cranelift JIT bug, not a WASM backend bug. Both
tests pass with `ELLE_JIT=0`.
