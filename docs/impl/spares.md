# Spares

<!-- audited: 2026-11-25 -->

A warm interpreted call allocates nothing on the Rust heap: it reuses the buffers and environments earlier calls gave back.

## What a call takes

A call needs four things the Rust heap would otherwise supply. The VM keeps
spares of each, and a call takes one and gives it back when it is done with it.

| Spare | A call holds in it | It goes back |
|-------|--------------------|--------------|
| An argument buffer | The arguments the call pops off the operand stack, or reads out of a spliced call's array | When the native or the callee's environment holds the arguments |
| A stack buffer | The caller's operand stack, while the caller waits for an interpreted callee ([vm.md](vm.md)) | When the callee returns and the caller's stack is back on the fiber |
| An environment | A closure body's environment, in the `Rc` its activation runs on | When the activation ends or a tail call replaces it, if nothing else holds the `Rc` |
| A deferred list | The releases an activation took over from its tail calls ([owner.md](region/owner.md)) | When the activation ends and runs them |

Argument buffers and stack buffers are one pool of `Vec<Value>`, because each
is a cleared vector of values. A splice's `extend` takes an argument buffer
too, to read its source before it copies it into the array it builds.
Environments are a second pool, of `Rc<Vec<Value>>`, and deferred lists a
third. Every activation takes a deferred list when it opens, whether or not a
tail call fills it.

An environment that something else still holds goes back to nobody. A parked
frame, a suspended chain and a re-entry's caller each keep their own reference.
The last holder frees it, and the next call builds a new environment.

## What a call reads in place

A tail call's instruction lists the stash slots of its borrowed arguments and
the arenas it strands, and every call lists its offers
([handoff.md](region/handoff.md)). The VM reads each list where it lies in the
bytecode, and copies none of them. The late mints a tail call makes are a set of
argument positions, which the call keeps in one word.

## Bounds

Each pool keeps at most 64 spares, and no pool keeps a vector with room for
more than 1,024 entries. A deep recursion therefore returns its stack buffers to
the allocator as it unwinds, past the first 64, and an oversized buffer is freed
rather than held.

## What pins it

[alloc_tests.rs](../../src/vm/call/alloc_tests.rs) runs 100 warm calls of each
kind in the interpreter: to a closure, to a native, by a tail call, by a
splice, by a tail call that borrows an argument, and by a call that builds an
array. It reads the Rust heap allocations they make, and fails unless there
are none. A failure names each source line that allocated, and how often.

A region the call makes allocates nothing either
([performance.md](../regions/performance.md)).
