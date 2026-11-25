# Spares

<!-- audited: 2026-11-25 -->

A warm interpreted call allocates nothing on the Rust heap: it reuses the buffers and environments earlier calls gave back.

## What a call takes

A call needs three things the Rust heap would otherwise supply. The VM keeps
spares of each, and a call takes one and gives it back when it is done with it.

| Spare | A call holds in it | It goes back |
|-------|--------------------|--------------|
| An argument buffer | The arguments the call pops off the operand stack, or reads out of a spliced call's array | When the native or the callee's environment holds the arguments |
| A stack buffer | The caller's operand stack, while the caller waits for an interpreted callee ([vm.md](vm.md)) | When the callee returns and the caller's stack is back on the fiber |
| An environment | A closure body's environment, in the `Rc` its activation runs on | When the activation ends or a tail call replaces it, if nothing else holds the `Rc` |

Argument buffers and stack buffers are one pool of `Vec<Value>`, because each
is a cleared vector of values. A splice's `extend` takes an argument buffer
too, to read its source before it copies it into the array it builds.
Environments are a second pool, of `Rc<Vec<Value>>`.

An environment that something else still holds goes back to nobody. A parked
frame, a suspended chain and a re-entry's caller each keep their own reference.
The last holder frees it, and the next call builds a new environment.

## Bounds

Each pool keeps at most 64 spares, and neither keeps a vector of more than
1,024 values. A deep recursion therefore returns its stack buffers to the
allocator as it unwinds, past the first 64, and an oversized buffer is freed
rather than held.

## What pins it

[alloc_tests.rs](../../src/vm/call/alloc_tests.rs) runs 100 warm calls of each
kind in the interpreter: to a closure, to a native, by a tail call and by a
splice. It reads the Rust heap allocations they make, and fails unless there
are none. A failure names each source line that allocated, and how often.

A region the call makes allocates nothing either
([performance.md](../regions/performance.md)).
