# Allocator Implementation Overview

Reference: [`allocator-plan.md`](allocator-plan.md)

## The question

How much can we stand up with zero behavioral change — all tests pass,
no observable difference — before touching escape analysis?

Four of six packages are pure infrastructure. The fifth is where
behavior changes. The sixth is deferred.

## Packages 1-4: infrastructure (no behavioral change)

### Package 1: FiberHeap routing

Create `FiberHeap`. Add it to `Fiber`. Replace the thread-local
`HEAP_ARENA` with a "current fiber heap" thread-local that gets swapped
on fiber transitions. `alloc()` dispatches to it. `ArenaGuard` operates
on it.

Initially `FiberHeap` wraps the same `Vec<Box<HeapObject>>`. The routing
changes; the backing store doesn't.

**Depends on:** nothing. **Risk:** low. Pure plumbing. If routing is
wrong, tests fail with null derefs or wrong pointers.

### Package 2: Bump allocator with destructor tracking

Replace `Vec<Box<HeapObject>>` with `bumpalo::Bump` inside `FiberHeap`.
Add destructor tracking: each scope mark has a `Vec<*mut HeapObject>`
of objects needing `Drop`. On arena release, walk the list then reset
the bump.

**Destructor tracking is mandatory from day one.** `ArenaGuard` (macro
expansion) depends on mark/release actually calling Drop. Without it,
the bumpalo switch silently leaks every String, Array, Closure, Table,
Struct, Tuple, Buffer, Bytes, Blob, Syntax, Fiber handle, External, and
FFISignature created during macro expansion.

Completeness is enforced by matching on `HeapTag` with no wildcard arm
in the destructor registration path. Missing a variant is a compile
error, not a silent leak.

**Depends on:** Package 1. **Risk:** medium-low. The destructor list is
new code but simple (Vec push, reverse walk). The risk is in the Drop
analysis — missing a variant leaks silently. The existing
`test_arena_guard_raii` and `test_arena_nested_guards` tests validate
the mark/release contract.

### Package 3: Scope bytecodes

Add `RegionEnter`/`RegionExit` to the `Instruction` enum and `LirInstr`.
Emit at `let`/`letrec`/`block` boundaries. Handle as no-ops in the VM
(push/pop scope marks but don't restrict allocation). Function bodies
do NOT get scope allocators.

Scope exit on early-exit paths (break, return, exception) is deferred.
These are no-ops, so missing them is harmless. Document the debt.

**Depends on:** Package 2. **Risk:** low. Mechanical bytecode addition
per the AGENTS.md checklist. Existing tests exercise nested let/letrec
heavily.

### Package 4: Allocator inheritance plumbing

Add `active_allocator: *const bumpalo::Bump` to `FiberHeap`. Save/
restore on Call/Return. Add to `SuspendedFrame` for yield/resume.
Initialize to the fiber's root bump on creation. Tail calls inherit
implicitly (pointer on FiberHeap, not on the frame).

This is a single pointer — the callee inherits the caller's allocator
for everything (temporaries and return values). The two-pointer
refinement (separate return allocator vs. scratch allocator) is
deferred until profiling shows temporary accumulation is a problem.

All write-only until Package 5. Debug assertions validate pointer
validity.

**Depends on:** Packages 2 and 3. **Risk:** low for behavioral change.
Medium for correctness of save/restore — bugs won't surface until
Package 5.

## Package 5: scope allocation (first behavioral change)

The allocator routes to scope allocators by default. `RegionExit` runs
destructors and resets the bump. Escape analysis (maximally conservative)
determines which bindings qualify for scope allocation.

**This is where memory starts being freed earlier.** The smallest demo:

```lisp
(let [x (cons 1 2)]
  (+ (car x) (cdr x)))
;; x freed at scope exit
```

**Two hard parts:**

1. **Scope exit on all control flow paths.** A `break` out of a block
   containing a `let` must emit `RegionExit`. A `return` from inside a
   `let` must emit `RegionExit`. Exception unwinding must emit
   `RegionExit` for every scope on the unwind path. The lowerer uses a
   "pending scope exits" stack: any non-local exit emits `RegionExit`
   for all pending scopes.

2. **Escape analysis soundness.** A false negative (local when it
   escapes) is use-after-free. Start maximally conservative — any use
   outside the defining `let` body marks it escaping. Tighten
   incrementally, each tightening a separate testable change.

**Depends on:** Packages 1-4. **Risk:** high. This is where a bug means
use-after-free. The ~3,000 existing tests are the safety net. Start
conservative and tighten.

## Package 6: shared allocators and zero-copy fiber exchange (deferred)

Shared allocators get their own bumps with allocator-level refcounts
(one counter per allocator, not per object). Each fiber that references
values in a shared allocator holds a reference. Fiber death decrements
the count. When it hits zero, destructors run and the bump resets.

This package also introduces fiber-escape analysis: values that might
cross fiber boundaries (yields, channel sends, storage into shared
structures) are allocated into shared allocators from the start.
Receiving fibers get pointers into shared space — zero-copy exchange.

This is where Elle diverges from Erlang. Erlang copies O(n) per
message send because it has no static knowledge of value destinations.
Elle's escape analysis moves that cost to compile time: conservatively
shared values are pre-placed, and exchange is O(1).

Deferred until Package 5 is stable and fiber communication patterns
are exercised enough to validate the analysis.

## Where the cliffs are

1. **Destructor tracking completeness (Package 2).** Missing a variant
   leaks silently. Mitigated by exhaustive `HeapTag` match.

2. **Scope exit on all paths (Package 5).** Missing a `RegionExit`
   corrupts the scope stack. Mitigated by structured emission in the
   lowerer.

3. **Escape analysis soundness (Package 5).** False negative is
   use-after-free. Mitigated by starting maximally conservative.

## Sequencing

```
Package 1  ─── FiberHeap routing              (low risk, no change)
    │
Package 2  ─── Bump + destructors             (medium-low risk, no change)
    │
Package 3  ─── Scope bytecodes                (low risk, no change)
    │
Package 4  ─── Allocator inheritance plumbing  (low risk, no change)
    │
Package 5  ─── Scope allocation               (HIGH risk, first real change)
    │
Package 6  ─── Shared allocators + zero-copy   (deferred)
```

Packages 1-4 are ~60% of the work and ~10% of the risk. Package 5 is
~20% of the work and ~80% of the risk. Package 6 is where fibers
become genuinely independent memory domains with zero-copy exchange.
