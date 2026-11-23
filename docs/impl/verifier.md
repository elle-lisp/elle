# The JIT verifier

<!-- audited: 2026-11-22 -->

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
and a release user program does not. The verifier is an assertion about the
translator's output, so a build with debug assertions runs it by default.

| Process | The verifier |
|---------|--------------|
| `elle` | As `--jit-verify=` says ([config](../config.md)). Without the flag, on in a build with debug assertions and off in a release build |
| `elle test`, and `elle-rig test` | On |
| `elle-rig` | On, whatever its flags say |
| An `--isolate` child | On, unless the `--isolate` flags say `off`: the runner passes `--jit-verify=on` ahead of them |
| A `make doctest` or `make smoke-plugins` run | On: the recipe passes `--jit-verify=on` |
| A Rust test that builds a `JitCompiler` | As the test names it: the translator's tests pass `JitVerifier::On` |
| A Rust test that runs Elle on a VM | On: the `dev` and `ci` profiles the Rust tests build carry debug assertions |
| An `elle` a test file starts | As its flags and its build say |

No environment variable carries the setting. A process hands it to a child on
the child's command line, so the command line says everything the child runs
under. A test that starts `elle` tests the command a user types, and names
`--jit-verify=on` only when it means to.

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
`fn/disasm-jit`. A Rust test of the translator builds its compiler with
`JitCompiler::new` and names the setting itself.
