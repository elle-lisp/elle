# JIT Compilation

<!-- audited: 2026-09-13 -->

The JIT subsystem compiles hot functions from LIR to native machine code using
Cranelift.

## How JIT Works

1. **Selection**: The VM counts calls to each function. A function that crosses
   the hotness threshold `--jit` sets is submitted for compilation.
2. **Compilation**: `FunctionTranslator` walks the function's LIR and emits
   Cranelift IR, which Cranelift compiles to native code. This runs on the
   `elle-jit` worker thread, or on the VM thread under `--trace=syncjit`.
3. **Caching**: The compiled `JitCode` goes into `jit_cache`, keyed by the
   address of the function's bytecode.
4. **Execution**: The next call to the function dispatches to the native code
   instead of building an interpreter frame.

## Calls

Every call a compiled function makes leaves through a runtime dispatch helper,
which reaches a compiled callee without returning to the interpreter. A tail
call to the executing closure is the exception: it becomes a native loop.

## Limitations

A function is rejected when it contains an instruction the translator does not
handle — `MakeClosure` is the common one — or when Cranelift fails to compile
it. A rejection is recorded and the function is never re-submitted.

Functions that yield and functions with polymorphic signals are compiled like
any other. A yield leaves compiled code through a side-exit that hands the
interpreter a resumable frame.

## See Also

- [AGENTS.md](AGENTS.md) — technical reference for LLM agents
- [docs/impl/jit.md](../../docs/impl/jit.md) — the design document
- [benches/](../../benches/) — performance benchmarks
