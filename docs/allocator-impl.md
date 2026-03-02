# Allocator Implementation Overview

Reference: [`allocator-plan.md`](allocator-plan.md)

## The question

How much can we stand up with zero behavioral change — all tests pass,
no observable difference — before touching escape analysis?

Four of six packages are pure infrastructure. The fifth is where
behavior changes. The sixth is deferred.

## Packages 1-4: infrastructure (no behavioral change)

### Package 1: FiberHeap routing

Create `FiberHeap`. Add it to `Fiber`. Add a "current fiber heap"
thread-local that child fibers install during execution. `alloc()`
dispatches to the current fiber heap if one is installed, otherwise
falls back to `HEAP_ARENA`. `ArenaGuard` operates on whichever is
active.

Initially `FiberHeap` wraps the same `Vec<Box<HeapObject>>`. The routing
changes; the backing store doesn't.

> **Amendment (discovered during implementation):** The root fiber does
> NOT get a FiberHeap installed. Only child fibers (created via
> `fiber/new`) get FiberHeap routing during `with_child_fiber`. The root
> fiber continues allocating from `HEAP_ARENA` (the thread-local
> fallback).
>
> **Rationale:** The original plan had `VM::new()` install the fiber
> heap and `VM::drop()` uninstall it. This causes SIGSEGV because
> `eval_source()` creates a VM, runs code, returns a `Value`, then
> drops the VM — returned Values point into the fiber heap which gets
> destroyed on VM drop. Keeping the root fiber on `HEAP_ARENA` means
> root-allocated Values survive VM destruction, which is the existing
> lifetime contract.
>
> **Consequences:**
> - `VM::new()` does NOT install a fiber heap
> - `VM::drop()` does NOT uninstall anything (no Drop impl needed)
> - `with_child_fiber` installs the child's heap before executing,
>   restores the parent's heap (or null for root) after
> - `pipeline.rs` save/restore is still needed for the compilation
>   cache VM (it creates a child VM context)
> - The fallback to `HEAP_ARENA` in the dispatch functions IS the root
>   fiber's allocator — this is by design, not a missing feature

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
Initialize to the child fiber's root bump on creation (root fiber has
no FiberHeap; see Package 1 amendment). Tail calls inherit implicitly
(pointer on FiberHeap, not on the frame).

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

## Package 6: shared allocators and zero-copy fiber exchange

**Status: COMPLETE.**

Parent-owned `SharedAllocator` provides zero-copy inter-fiber value
exchange. When a yielding child fiber allocates heap objects, those
allocations route to a shared allocator owned by the parent (or by
the child itself for root→child chains). The parent reads yielded
values directly — no deep copy.

Key design decisions:
- **Parent-owned model**: `Box<SharedAllocator>` on parent's
  `FiberHeap.owned_shared`. Child gets raw `*mut SharedAllocator`.
  No Rc, no RefCell, no runtime borrow checks on the allocation path.
- **Downward propagation**: In A→B→C chains, B propagates its
  `shared_alloc` pointer down to C. All values end up in A's shared
  alloc. A outlives B and C via nested `with_child_fiber` on the Rust
  call stack.
- **Effect gate**: Only yielding fibers (`Effect::Yields`) get shared
  allocators. Non-yielding fibers are unaffected.
- **Conservative routing**: `!shared_alloc.is_null()` → route ALL
  allocations to shared. Simple, correct, wasteful for temporaries.
  Tightening is a future change.
- **No new bytecode**: Uses existing `active_allocator` from Package 4.
- **Per-resume creation (tech debt)**: Each resume creates a new shared
  allocator. Old ones accumulate in `owned_shared` until `clear()`.

**Depends on:** Packages 1-5. **Risk:** low. The routing predicate only
activates for yielding child fibers. All existing tests pass unchanged.

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
Package 1  ─── FiberHeap routing              ✅ COMPLETE
    │
Package 2  ─── Bump + destructors             ✅ COMPLETE
    │
Package 3  ─── Scope bytecodes                ✅ COMPLETE
    │
Package 4  ─── Allocator inheritance plumbing  ✅ COMPLETE
    │
Package 5  ─── Scope allocation               ✅ COMPLETE
    │
Package 6  ─── Shared allocators + zero-copy   ✅ COMPLETE
```

All six packages are complete. The allocator infrastructure is fully
operational. Future work: tighten escape analysis to enable scope
allocation, optimize shared alloc reuse across resume cycles (M2).
