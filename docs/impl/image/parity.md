# Parity: what a hydrated closure keeps

<!-- audited: 2026-10-25 -->

Why a hydrated closure compiles and promotes as a compiled one does: its code
payload carries the body fragment and the LIR.

[image.md](../image.md) owns the design this serves, and [booting](boot.md)
owns the gates that prove both halves for the boot configuration.

## The two boot modes compile user code alike

The heap is not the whole boot product. A later user compile also reads the
body of each stdlib function, as a `LambdaFragment` encoded in the bytes its
closure's code payload carries. A user compile grafts a call to a stdlib
function from that body ([grafting a call](../graft.md)), and fusion inlines a
named stdlib function from it
([dissolving a named function](../dissolution/inline.md)).

An image boot that left the fragments out would compile user code
*differently* from a source boot: silently worse code, and divergent artifacts
for anything keyed on compiler output. That is not acceptable. The two boot
modes must produce identical user-code compiles.

A fragment is an HIR body closed over its own binding table
([the HIR design](../hir.md)), so it is plain data that crosses a process
boundary as it stands. The payload carries it as bytes, like the docstring, so
an image maps it with the payload, as it maps the LIR, with no re-derivation
from syntax. A hydrated closure therefore grafts as a compiled one does.

## The JIT promotes a hydrated closure

The JIT compiles from the LIR a code payload carries: region-native records in
the payload's own pages ([lir.md](../lir.md)). The fifth foundation put them
there ([foundations](foundations.md)). A function's LIR therefore dumps in the
dumper's own walk and arrives with the mapping, so an image's save and load need
no mechanism of their own for it. `jit_cache` keys on the bytecode address,
which is stable for the hydrated region's life. Machine code itself is never
persisted: Cranelift output bakes absolute addresses and is not relocatable.

A promotion copies. The JIT worker runs on another thread, and a region belongs
to one `RegionStore`, so `prepare_task` copies the function out of the payload
into an owned form the task holds.

Carrying the LIR is not optional for the boot configuration. An image boot
whose stdlib could not reach the JIT tier would trade startup for steady-state
throughput. That is a deal-breaker, and it violates parity: the two boot modes
must be indistinguishable to running code, tiers included. A hydrated stdlib
closure promotes exactly as a compiled one does, so the tier half of parity
holds, as the compile half does.
