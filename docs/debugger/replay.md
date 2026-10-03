# Debugger: recording and replay

<!-- audited: 2026-09-28 -->

How a debugger session records the nondeterministic inputs of a run, and replays
it to answer questions about its past.

This document is part of the [debugger design](../debugger.md), and every name
it introduces is proposed unless it says the tree has it today.

## Execution history

Recording and replay are specified now so the instrumentation points
are designed in; they ship as a later phase.

### What gets recorded

Fibers are single-threaded and cooperative, so there are no thread
interleavings. External nondeterminism enters through primitives
(with two machine-level exceptions, below), so the recorder lives at
the primitive layer: a VM record/replay mode flag consulted at
primitive dispatch. Both schedulers — the async scheduler in
[src/stdlib.lisp](../../src/stdlib.lisp) and the process scheduler in [lib/process.lisp](../../lib/process.lisp) —
are deterministic Elle code driven entirely by these primitives'
results, so both replay without instrumentation.

The seam is not a hand-audited list. `PrimitiveDef` gains a `replay`
class, declared in the `primitive!` tables like `signal` and
`effect`, and a registry test fails on any unclassified primitive —
adding a primitive without deciding its replay class breaks the
build. Three classes:

| Class | Meaning | Examples |
|-------|---------|----------|
| `pure` | deterministic given VM state; never recorded | arithmetic, collections, `string/*` |
| `record` | result logged in call order; replay feeds it back | `io/*`; `file/*`, `port/*`, `read`, `read-all`; `subprocess/*`; `clock/*`, `time/sleep`; `sys/env`, `sys/args`, `sys/argv`, `sys/pid`, `sys/resolve`, `sys/thread-id`, `sys/ip?`; `os/sig-*`; `ev/poll-fd`, `chan/wait-ready`; `ffi/*`, `ptr/to-int`; `debug/memory`, `debug/arena-*`, `vm/tier`, `jit/rejections` |
| `refuse` | no sound seam exists; recording raises an error | `sys/spawn`, `sys/spawn-vm`, `ffi/callback` |

FFI stubbing is sound because every foreign observation flows back
through an `ffi/*` seam: a recorded pointer is just a number until
`ffi/read` dereferences it, and that read replays its recorded value.
`ffi/callback` is refused under recording — a foreign-initiated entry
into Elle has no seam. A subprocess request carries `:exec` alongside
`:io`, but only `:io` selects a backend, so it records on the `:io`
seam like any other request.

Thread refusal is required, not conservative: `sys/thread-state`,
`chan/send`, `chan/recv`, and `chan/try-select` are synchronous
timing reads with no signal, so another thread's timing leaks into a
"single-VM" run invisibly. v2 may admit threads by moving the `chan/*`
family and `sys/thread-state` to the `record` class.

The two machine-level exceptions do not flow through primitives.
First, tier choice: the JIT worker is an OS thread, and whether call
*N* runs interpreted or native depends on its scheduling. Results are
tier-invariant (every build must pass the same language suite,
[spec](../spec.md)), but allocation order is
not guaranteed to be, and the tier observations (`vm/tier`,
`jit/rejections`, `debug/arena-*`) are not. Recording therefore
disables the async worker, promotes tiers synchronously by call
count — deterministic, because call counts are deterministic — and
stores the tier policy in the log header; replay applies the same
policy. Second, heap addresses: see
Identity order, below.

### The log and the clock

The log is a flat array of event structs:

```text
{:t [segment fuel] :ev :io     :value <recorded result>}
{:t [segment fuel] :ev :break  :file "x.lisp" :line 9 :ip 44}
```

The clock is a pair. `segment` counts recorded events — the coarse,
cheap coordinate recorded live. `fuel` addresses a point inside a
segment: replay to the segment start, set the debug flag and
`fiber/set-fuel k`, resume. Recording never meters instructions; only
a replay that needs an intra-segment stop pays for step mode.

### Replay

`debug:replay log &named until` re-runs the program feeding recorded
values back at each seam, in order. Execution between seams is
deterministic, so the replay is bit-for-bit the original run. Every
debugger feature works during replay — which yields **retroactive
breakpoints**: choose the predicate after the crash, replay with it
armed, and observation does not perturb the bug.

That last claim needs two guarantees, stated here because both are
easy to break silently.

First, observation must not allocate into the debuggee's regions.
The design already guarantees this: inspection results are
`Fresh`-allocated in the debugger's regions, and dynamic pauses carry
`nil` payloads.

### Identity order

Second — a prerequisite, not a discipline. Reference-identity values
(closures, parameters, syntax, managed pointers) order and hash by
heap address today ([src/value/repr/traits.rs](../../src/value/repr/traits.rs)): `(hash f)` returns
an address-derived integer, and a set of closures iterates in address
order. Replay runs in a fresh process, and addresses are not
reproducible across processes, so any program that observes such an
order can diverge under replay before the first seam. Phase 5
therefore starts by making identity order deterministic:
reference-identity values order and hash by a per-VM allocation
sequence number, not by address. Fibers already do the equivalent —
they hash by handle identity, which survives relocation — and the
same move makes closure order a pure function of the execution
history. A test pins the result: a recorded run and its replay, in
separate processes, produce the same event log, the same `hash`
values for the same closures, and the same closure-set iteration
order, with breakpoints armed in only one of them.

`debug:bisect log invariant` binary-searches the clock for the first
point where `invariant` fails on the snapshot: log₂(N) replays, each
one batch call. This is the primary agent workflow the design serves.

