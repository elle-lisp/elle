# The JIT verifier

<!-- audited: 2026-10-23 -->

Cranelift's verifier checks each function the JIT compiles, and a test turns it on.

## What it checks

The verifier reads the Cranelift IR the translator built, before the backend
compiles it. It refuses IR that is not well formed:

- an operand of the wrong type for its instruction
- a value used where its definition does not dominate the use
- a block that does not end in a terminator
- a jump whose arguments do not match the parameters of its target block
- a call that does not match its callee's signature

Elle code cannot build Cranelift IR, so a refusal is always a defect in the
translator. A refused function becomes a rejection, and the VM prints the error
once ([jit](jit.md)). Without the verifier, the backend compiles the same IR to
code that can answer a wrong value or crash far from its cause.

## Who runs it

The verifier costs a share of every compile. The defects it finds are in the
translator, and the suites drive the translator, so a test runs the verifier
and a user program does not.

| Process | The verifier |
|---------|--------------|
| `elle` | Off, unless `--jit-verify=on` or `ELLE_JIT_VERIFY=on` asks ([config](../config.md)) |
| `elle test` | On |
| `elle-rig`, and `elle-rig test` | On |
| A Rust test that builds a `JitCompiler` | On: the test passes `JitVerifier::On` |
| A Rust test that runs Elle on a VM | As `ELLE_JIT_VERIFY` says |

`elle test` and the rig set `ELLE_JIT_VERIFY=on` in their own environment, so
every process they start runs the verifier too: each `--isolate` child, and
each `elle` a test file starts. The `Makefile` exports the variable to every
recipe, so `make doctest`, `make smoke-plugins` and the Rust tests that
`make test` runs verify as well. Each CI workflow sets the variable for all of
its jobs, so a step that calls `cargo test` without `make` verifies too.

`(vm/config :jit-verify)` reads whether this process runs the verifier.
`vm/config-set` refuses to change it, because the setting belongs to the
process and not to one VM: every VM in the process compiles under it.

```lisp
(assert (boolean? (vm/config :jit-verify)) "a process runs the verifier or does not")
(let [[ok? err] (protect (vm/config-set :jit-verify true))]
  (assert (not ok?) "a program cannot change the verifier")
  (assert (= (get err :error) :argument-error)))
```

## How the compiler takes it

`JitCompiler::new` takes a `JitVerifier` and sets Cranelift's
`enable_verifier` flag from it. `JitCompiler::for_process` reads the process
configuration, and every compiler the VM builds comes from it: the background
worker, the synchronous compile of `--trace=syncjit`, `compile/run-on :jit` and
`fn/disasm-jit`. A Rust test names the setting itself, so a test of the
translator verifies whatever the environment says.
