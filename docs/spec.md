# The language specification

<!-- audited: 2026-09-26 -->

What the specification of Elle holds, what it leaves to an implementation, and
how a test says which of the two it checks.

The specification is not written yet. This document owns its design until it
is, and it decides today where every test lives. It replaces the design in
[#1272](https://github.com/elle-lisp/elle/issues/1272).

## Two suites

One corpus used to hold two kinds of test. A file that pinned a calling
convention ran beside a file that pinned what `+` returns, and both passed. When
an optimization removed the call, the first file still passed and tested
nothing. Each proposed repair added a second language: a parameter that changes
nothing, a form that keeps a call a call, a declaration, a flag. A green result
under a second language is evidence about a runtime that no user runs.

So the tests split in two.

**A language test** asserts what the specification promises: values, errors,
signals, fiber states, and whether a program compiles. The claim holds for
every correct implementation, so no optimization can make the test vacuous. The
language suite lives in [tests/lang](../tests/lang/overview.md). It runs the
build as shipped, with no flag. The runtime chooses its tier, and a language
test neither chooses a tier nor observes one.

**An implementation test** checks one implementation: its constants, its
crashes and its mechanisms. Heap growth, region counts, page claims,
use-after-free, JIT promotion and the shape of a call all belong here. A
self-hosted Elle, or an Elle over LLVM, would pass the same language suite with
a different footprint and keep an implementation suite of its own. The
implementation suite is [tests/impl](../tests/impl/overview.md) and the Rust
suite under [tests](../tests/AGENTS.md).

An implementation test enters at the layer it tests. A Rust test builds its
call as LIR or bytecode, beneath every optimization. An Elle file runs on
[the rig](../rig/overview.md), which hosts the same compiler as `elle` and can
configure it in ways `elle` cannot: the JIT off or eager, a diagnostic mode on.
The file names that configuration in a sidecar beside it. Nothing in the
language keeps a shape alive through the optimizer, and no language test needs
it to.

## A build is an implementation

A user runs no test matrix. Each build carries one optimizing tier, and a
program cannot choose another. The JIT is the tier of the default build; an
`mlir` build carries MLIR instead, and a `wasm` build carries WebAssembly
([config](config.md) owns the rules).

CI builds several implementations and runs the language suite on each:
the default build, a build with no JIT, a build on the thread-pool I/O backend,
an MLIR build, a build with no features, and the default build on AArch64 and
macOS. Every build must give the same verdict on every language test. A
disagreement is a defect in the build that disagrees
([ci](analysis/ci.md) lists the jobs).

## Three categories of surface

| Category | Documented | Portable | Example |
|---|---|---|---|
| Language | in the specification | yes | `fiber/new`, `%add`, `(silence)` |
| Implementation extension | by the implementation | no | `jit?`, `fn/bytecode-size`, `compile/run-on`, `arena/count` |
| Implementation test surface | no | no | a rig sidecar that turns the JIT off |

An implementation extension is part of the user build, and its documentation
says that it belongs to this implementation. A language test does not call
one. The specification may define a symbol and leave its value open: it can say
that `arena/count` exists and returns a non-negative integer. No language test
then asserts the value, or any relation between two readings.

The implementation test surface is what the rig adds: configuration that no
program and no user can reach. It never reaches user documentation, because
nothing in a user build accepts it.

## What the specification holds

| Layer | In the specification as | Lowering into it |
|---|---|---|
| Source | Syntax, versioned by epochs; library surface, versioned by `elle/version` and `.surface` | — |
| Core | The expanded forms, with the static semantics that decide acceptance and the dynamic semantics that decide behavior | Expansion, by the macro rules |
| Code | A validated IR with region semantics and binary compatibility rules | Each implementation's own |

**A program observes the core through source only.** Acceptance depends on
inference: an unproven `%`-intrinsic and a loud `(silence)` body are compile
errors. So the specification defines the inference over the core, and no
language test reads an implementation's HIR.

**The code layer enters the specification when three conditions hold.** Each
instruction means something in the specification's own terms, with region
operations defined against its region model. A validator the specification
defines decides whether code is valid, so a proof such as `OperandProof::Int`
becomes a checkable typing fact. Versioning states which changes to compiled
code keep loading. Until then the code format belongs to the implementation.
Conformance tests for the code layer are written in the IR, and they test its
consumers: loaders, VMs, the JIT and the region-snapshot hydrator.

**A features list names the optional parts,** such as proper tail calls or
unbounded non-tail recursion. An epoch versions the required core. A feature
names an optional part that an implementation declares.

**The specification carries commitments that the language suite cannot
test.** The fiber-sympathetic memory model is one. Stated in the
specification, it rules out language features that need a tracing collector,
and it would have marked a reference-counting implementation non-conforming
from its first commit. Its tests live in the implementation suite.

## Images

An image is a region snapshot. Regions are in the specification, so a
specified snapshot layout belongs to it too. The fingerprint that locks an
image to one binary solves one implementation's portability problem, and it is
the current configuration rather than a rule ([image](impl/image.md)). A
snapshot becomes portable once the code layer is in the specification. HIR
fragments in a snapshot are optional data: an implementation that ignores one
loses cross-unit inlining, and every call stays a call.

## What is not written yet

The features list, the static semantics of the core, and the code layer are
not in the specification yet. The code layer waits on its three conditions.
The language documents and the language suite move to a repository of their
own once an implementation other than this one needs them.

## Alternatives this rejects

- **A form or a declaration that keeps a call a call.** The fiber is the unit a
  program controls, not the closure. A language form that fixes a frame, a fuel
  charge or a backtrace entry at a call gives programs control over the
  closure.
- **An unused `&opt _` parameter, or `(def g f)`.** Each relies on a current
  limit of an optimization, not on a rule of the language. The day the
  optimization lifts that limit, the test is vacuous again.
- **A command-line flag or a harness profile for language tests.** Each is a
  second runtime. A user build therefore has no flag that chooses a tier.
- **Language tests against this implementation's HIR.** They make this
  implementation's HIR the specification.
