# Tail-call reclamation: compile-time drop insertion

## Prior art

Elle's problem — freeing temporaries across tail-call iterations without
runtime scanning — has been solved in the literature. The solutions share
a principle: **move all lifetime decisions to compile time.**

### Tofte-Talpin regions and storage modes (1994-1997)

The ML Kit compiler extended Tofte-Talpin region inference with *storage
mode annotations*. The `atbot` annotation resets a region before storing
a new value, allowing tail-recursive functions to reuse the same region
across iterations. The region is "reset" (all values discarded) at the
bottom of the recursive call, and fresh values are stored into the same
space.

This is the closest precedent to what Elle needs. But `atbot` works
because the ML Kit's type system guarantees values don't escape their
region — region types prevent it. Elle doesn't have region types, so it
must prove non-escape through analysis (which it already does via
`rotation_safe`).

Reference: Birkedal, Tofte, Vejlstrup. "From Region Inference to von
Neumann Machines via Region Representation Inference" (POPL 1996).
Aiken, Fähndrich, Levien. "Better Static Memory Management" (PLDI 1995).

### Perceus (PLDI 2021)

Perceus emits *precise* reference counting instructions at compile time
such that (cycle-free) programs are garbage-free. The key innovation:
*drop specialization* — instead of generic `drop(x)`, the compiler emits
type-specific drops that know the constructor layout and can reuse the
memory slot for a new allocation of the same type.

For tail calls, Perceus inserts drops for dead parameters *before* the
recursive call. The analysis determines which parameters are "consumed"
(their reference count drops to zero) and which are "borrowed" (still
referenced by a tail-call argument). Consumed parameters are freed;
borrowed ones survive.

This is exactly the DropSlot analysis described below. Elle's escape
analysis already answers the "consumed vs borrowed" question.

Reference: Reinking, Xie, de Moura, Leijen. "Perceus: Garbage Free
Reference Counting with Reuse" (PLDI 2021).

### FP² (ICFP 2023)

FP² proves that many functional programs can be executed *fully
in-place* — requiring no allocation or deallocation at all. A function
annotated `fip` (fully in-place) is guaranteed to:
- Use constant stack space (no non-tail calls)
- Pair every deallocation with an allocation of the same size
- Reuse freed memory slots directly via *reuse tokens*

For tail-recursive functions, FP² shows that if the recursive case drops
exactly the values that the next iteration allocates, the compiler can
thread reuse tokens so that no net allocation occurs. The function runs
in O(1) memory with zero allocator overhead.

This is the ceiling for what static analysis can achieve. Elle cannot
implement full FIP today (it lacks the type-directed reuse token
threading), but the analysis that determines "which allocations are
paired with which deallocations" is the same analysis that drives
DropSlot.

Reference: Lorenzen, Leijen, Swierstra. "FP²: Fully in-Place Functional
Programming" (ICFP 2023).

### What these have in common

All three approaches:
1. Make all deallocation decisions at compile time
2. Emit explicit free/drop/reset instructions in the IR
3. Require no runtime scanning, no generation tracking, no snapshots
4. Are O(1) per freed object
5. Compose correctly with nested scopes, control flow, and tail calls

This is the standard. Everything else — pointer snapshots, swap pools,
rotation logs, generation tags — is deviation from 30 years of
established research.

## What Elle already has

Elle's escape analysis is *more powerful* than Perceus for scope
allocation because it has region information: it knows not just "this
value's refcount hit zero" but "this value was allocated in scope S, and
scope S is ending." This lets it batch-free entire scopes via
mark/truncate (RegionExit) — O(1) regardless of how many objects are in
the scope.

For while loops, this works perfectly. The scope is entered, the body
runs, the scope is exited, everything is freed. Loop variables survive
because they're in registers, not in the scope's region.

For tail calls, the scope mechanism fails because the tail-call
arguments are heap values allocated *within* the scope. Freeing the
scope dangles the arguments.

But the Perceus insight applies: don't free the entire scope. Free
individual dead objects. The compiler knows which objects are dead at the
tail-call point.

## The approach: DropSlot before TailCall

A new bytecode instruction:

```
DropSlot(reg)
```

Semantics: if the Value in `reg` is a heap pointer owned by the current
fiber's pool, return the slab slot to the free list (dealloc_slot) and
run its destructor if needed. If it's an immediate or not owned, no-op.
O(1). One branch on the Value tag.

The compiler emits DropSlot for each provably dead heap binding before
TailCall:

```
; (let* [s (concat "iter-" (number->string n))]
;   (f (- n 1)))

; ... s is in r2, dead at tail-call point ...
DropSlot r2                  ; free s
TailCall f, [r1]
```

### What the compiler proves

At each tail-call site, for each binding in scope:

1. **Is it heap-allocated?** — from the HIR expression type. Literals,
   intrinsic results, and arithmetic are immediate. Concat, cons, struct
   literals, etc. are heap-allocated.

2. **Is it referenced by any tail-call argument?** — walk each arg's HIR
   subtree for Var references to this binding.

3. **Has it escaped to external mutable state?** — `rotation_safe` on
   the enclosing function. If true, no bindings escaped.

A binding is droppable at the tail-call point if (1) AND NOT (2) AND (3).

For parameters (the previous iteration's values):

4. **Is this parameter referenced by any tail-call argument?** —
   `compute_return_params` already computes this for self-tail-calls.
   Extend to mutual tail calls.

A parameter is droppable if NOT (4) AND (3).

### What gets freed

**Dead temporaries:** strings from number->string, concat intermediates,
struct literals used only for field access, closures created and called
within the body. These are the values that scope reclamation
(RegionExit) would free if the scope exit were reachable.

**Dead parameters:** the previous iteration's parameter values that are
not referenced by any current tail-call argument. These are the values
that "rotation" was trying to free.

**Combined effect:** equivalent to region reset (atbot) for tail-call
loops, but selective — only provably dead objects are freed.

## Why the cross-iteration problem disappears

Rotation mechanisms reason about which allocations from iteration N-2
are safe to free during iteration N. This cross-iteration reasoning is
the source of every correctness bug.

DropSlot reasoning is LOCAL to each iteration:

- Iteration N's body creates temporaries and evaluates tail-call args
- Before TailCall, it drops dead bindings (current iteration's waste)
- Before TailCall, it drops dead params (previous iteration's values
  that the current iteration didn't reference via its args)
- The args survive because they were NOT dropped

No cross-iteration tracking. No snapshots. No double-buffer. The
"one-iteration lag" falls out from the parameter lifecycle: N-1's arg
values become N's parameters, and are freed during N when N determines
they're dead.

## Validation gates

These are derived from `tests/elle/leak.lisp` (13 tiers) and
`tests/elle/leak-grpc.lisp` (real-world bidi streaming). Each tier
must pass with bounded memory. The `checked?` gate (which skips
assertions under `--checked-intrinsics`) should be eliminated — the
%-prefixed IMMEDIATE_PRIMITIVES fix already addresses the root cause.

### Gate 0: while-loop scope reclamation (existing, must not regress)

DropSlot does not touch while loops. RegionEnter/RegionExit continues
to handle them. All Tier 0 tests (t0-let-struct, t0-discard-struct,
t0-string, t0-pair, t0-let-pair) must remain bounded.

**Why DropSlot doesn't interfere:** while-loop bodies are not tail-call
positions. RegionExit is reachable. DropSlot is only emitted before
TailCall instructions.

### Gate 1: nested while loops (existing, must not regress)

t1-nested: inner and outer loops both bounded. Same reasoning as Gate 0.

### Gate 2: tail-call rotation — THE PRIMARY TARGET

t2-struct, t2-string, t2-mutual: tail-recursive functions allocating
heap values per iteration. With DropSlot, the allocated struct/string
is dead at the tail-call point (not referenced by any arg) and is
freed.

**New validation:** run these WITHOUT the `checked?` gate. They must
pass with --checked-intrinsics AND without. The %-prefixed names fix
makes escape analysis recognize %sub/%le as immediate, so
rotation_safe=true in both modes.

**Mutual tail recursion (t2-mutual):** t2-even and t2-odd tail-call
each other. DropSlot must handle cross-function tail calls. The dead
struct `{:parity :even :n n}` is in a begin form preceding the tail
call — it's a discarded expression, provably dead. The analysis must
recognize "discarded expression in begin before tail call" as dead.

### Gate 3: yielding while loops

t3-yield-struct, t3-yield-string, t3-yield-multi: fiber yields
mid-iteration. DropSlot does not directly apply here (while loops,
not tail calls). Scope reclamation via RegionRotate handles these.
Must not regress.

### Gate 4: correctness under rotation

t4-return: tail-call return value survives. With DropSlot, the return
value (a string) must NOT be dropped — it flows to the base case, not
to a tail-call arg. The analysis must distinguish "tail call in
recursive branch" from "return in base case."

t4-accum: accumulator threaded through tail calls survives. The
accumulator IS a tail-call arg — not dropped. Must survive 10,000
iterations.

t4-yield: yielded heap values survive per-iteration scope release.
DropSlot doesn't interact with yield (different mechanism). Must not
regress.

### Gate 5: fiber lifecycle

t5-one-shot through t5-protect: fiber creation/destruction in while
loops. DropSlot doesn't interact. Must not regress.

### Gates 6-9: collection HOFs, conversions, strings, structs

Tier 6 (reduce, fold, zip, sort, reverse, distinct, take, drop,
group-by, frequencies), Tier 7 (->array, ->list, freeze, slice, keys,
values, merge), Tier 8 (string ops), Tier 9 (struct patterns): all
while-loop scope reclamation. DropSlot doesn't interact. Must not
regress.

### Gate 10: combined realistic patterns

t10-call-chain, t10-let-chain, t10-each-array, t10-format,
t10-pipeline: while loops with complex bodies. Must not regress.

t10-push-accum: currently a known regression marker (asserts NOT
bounded). This test pushes heap values into an outer mutable array —
a genuine escape. DropSlot cannot help here. Requires drop-on-overwrite
(future: Perceus-style drop specialization for put/assign).

### Gate 11: refcount mutation reclamation

t11-put-overwrite, t11-put-struct, t11-set-array, t11-roster,
t11-binding-reassign: overwritten heap values freed via deferred
refcounting. DropSlot doesn't interact (these are while loops with
refcount-aware rotation). Must not regress.

### Gate 12: user functions in while loops

t12-user-struct through t12-wrap-map: calling user functions that
allocate internally, from a while loop body. The callee's allocations
are freed by the callee's own scope mechanisms. The caller's scope
reclamation handles the caller's temporaries. Must not regress.

### Gate 13: value-flow propagation

t13-factory through t13-heap-struct-field: closures obtained through
factories, aliases, conditionals, struct fields. These test the
rotation-safety analysis for while-loop scope reclamation. Must not
regress.

### Gate 14 (new): tail-call + inner scope interaction

New tests that combine tail calls with inner scopes:

```lisp
; Tail call with let scope in body — DropSlot replaces unreachable RegionExit
(defn t14-let-tailcall [n]
  (if (<= n 0) (arena/count)
    (let* [s (concat "iter-" (number->string n))]
      (t14-let-tailcall (- n 1)))))

; Tail call with RegionExitCall in body (call-scoped reclamation)
(defn t14-call-scoped [n]
  (if (<= n 0) (arena/count)
    (begin
      (concat "a" (number->string n))  ; call-scoped reclamation frees intermediate
      (t14-call-scoped (- n 1)))))

; Tail call with multiple dead bindings
(defn t14-multi-dead [n]
  (if (<= n 0) (arena/count)
    (let* [a (string "a-" n)
           b {:x n}
           c (concat a (number->string n))]
      (t14-multi-dead (- n 1)))))

; Tail call with some live, some dead params
(defn t14-mixed-params [n alive dead]
  (if (<= n 0) alive
    (t14-mixed-params (- n 1) (cons n alive) (string "dead-" n))))
; dead is overwritten with a fresh string — old dead is droppable
; alive is referenced by arg — not droppable
```

### Gate 15 (new): --checked-intrinsics parity

ALL Tier 2 tests must pass identically with and without
--checked-intrinsics. The `checked?` gate must be removed. If a test
requires `checked?` to skip its assertion, the escape analysis has a
gap that must be fixed before the DropSlot work proceeds.

### Gate 16 (aspirational): gRPC bidi streaming leak

The grpc-leak test (leak-grpc.lisp) currently asserts LINEAR growth
as a regression marker. This leak involves fibers yielding for I/O
during bidi streaming — the fibers' arena regions are never reclaimed.

DropSlot does not directly fix this (it's a fiber lifecycle issue, not
a tail-call issue). But it establishes the principle: compile-time
analysis determines what's dead, and explicit instructions free it.
Future work should apply the same principle to fiber-scoped regions.

## What changes in the compiler

### New bytecode: DropSlot

```rust
Instruction::DropSlot => {
    let reg = read_reg(bytecode, &mut ip);
    let val = stack[reg];
    if val.is_heap() {
        if let Some(ptr) = val.as_heap_ptr() {
            fiberheap::with_current_heap_mut(|h| {
                if h.pool_owns(ptr) {
                    let ho_ptr = ptr as *mut HeapObject;
                    if needs_drop_for_tag(ho_ptr) {
                        unsafe { std::ptr::drop_in_place(ho_ptr) };
                    }
                    unsafe { h.pool.dealloc_slot(ho_ptr) };
                }
            });
        }
    }
}
```

### Lowerer: emit DropSlot before TailCall

In the tail-call lowering path:

1. Evaluate all tail-call args into registers (existing code)
2. Compute the set of arg-referenced bindings (walk each arg's HIR for
   Var references)
3. For each in-scope binding NOT in that set: if the binding's init is
   heap-allocating and the function is rotation_safe, emit DropSlot
4. For each function parameter NOT referenced by any arg: if the
   function is rotation_safe, emit DropSlot for the param register
5. Emit TailCall (existing code)

**For scopes whose body IS a tail call:** skip RegionEnter emission.
The DropSlots replace the unreachable RegionExit. For scopes where
some branches are tail calls and some aren't: emit RegionEnter; in the
tail-call branch emit DropSlot (RegionExit unreachable); in the
non-tail branch emit RegionExit as usual.

### Escape analysis: no changes

rotation_safe already answers "does the body escape heap values to
external mutable state?" compute_return_params already answers "which
params flow to the return?" The DropSlot analysis queries these
existing results.

## What changes in the runtime

Remove from FiberHeap: rotation_log, drain_rotation_log(), dealloc_ptrs(),
reset_alloc_count(), rotation_log.push() in alloc().

Remove from VM: RotationState struct, rotation.advance() in trampoline.

The trampoline becomes pure control flow — it swaps bytecode/env/
constants/location_map. It does not touch the heap.

## Future work (from the literature, not for this branch)

### Reuse tokens (FP²)

When the compiler can prove that a DropSlot at the tail-call boundary
is paired with an allocation in the next iteration of the same HeapTag
and size, it can thread a *reuse token* — a pointer to the freed slot
— into the allocation, bypassing the free list entirely. This turns
DropSlot + alloc into a single in-place mutation. O(0) allocator
overhead.

Requires: tag-aware analysis of allocation sites in the recursive case.
The DropSlot infrastructure enables this without further changes to the
scope or rotation mechanisms.

### Drop specialization (Perceus)

Generic DropSlot runs the destructor and returns the slot. Specialized
drops know the HeapTag and can skip the destructor check (most tags
don't need Drop). For Pair (the most common tail-call allocation),
drop specialization eliminates the branch entirely.

Requires: emit tag-specific DropSlotPair/DropSlotString/etc. The
lowerer already knows the expression type for most allocations.

### Borrowing analysis (Lorenzen 2022)

Parameters that are only read (not stored externally) can be marked
"borrowed." Borrowed parameters don't need DropSlot — they were
never owned by this iteration. The caller retains ownership.

For tail calls, borrowed parameters are those that appear in tail-call
args by reference (e.g., `(cons n acc)` borrows `acc`). They survive
because the caller (the trampoline) holds the env. This is what
happens today implicitly — formalizing it as borrowing lets the
compiler skip DropSlot emission for borrowed params, reducing the
number of drop instructions.

### TRMC (Leijen & Lorenzen 2023)

Tail recursion modulo context transforms `(cons x (f y))` so that the
cons cell is allocated first, the tail call fills in the cdr, and the
whole operation runs in constant stack space. Combined with reuse
tokens, this enables O(1) cons-list construction in tail position.

## Guardrails

### How to know you're going off track

1. **You're adding runtime state to the trampoline.** The trampoline
   should be stateless w.r.t. the heap. If it tracks allocations, marks,
   snapshots, or refcounts, the design has moved analysis to runtime.

2. **You're emitting DropSlot for a binding you can't prove is dead.**
   Either prove it at compile time or don't emit the drop. There is no
   "conservative" option — there's correct and incorrect.

3. **You need to reason about previous iterations.** DropSlot reasoning
   is local to the current iteration. If you find yourself thinking
   about "iteration N-2," you've reintroduced cross-iteration coupling.

4. **You're scanning pool.allocs or any allocation list.** DropSlot is
   O(1). dealloc_slot is O(1). If anything scans, the design is wrong.

5. **A test requires `checked?` to skip its assertion.** That means the
   analysis has a gap. Fix the analysis, don't gate the test.

6. **You're reimplementing one of the rejected approaches under a new
   name.** If your mechanism has a Vec of pointers, a mark to scan from,
   a generation tag to check, or a refcount to bump, it's one of the
   approaches in the rejected table. Stop and re-derive from first
   principles.

### Before writing code

1. Paper trace-throughs for: t2-struct, t2-mutual, t14-let-tailcall,
   t14-mixed-params. Show every DropSlot emission and every surviving
   value.

2. Show the LIR diff for each: the DropSlot instructions that appear,
   the RegionEnter/RegionExit instructions that disappear.

3. Identify the patterns where the analysis cannot prove a binding is
   dead. These are the acceptable leaks. Enumerate them.

4. Run leak.lisp without `checked?` gates and catalog which tiers
   currently fail under --checked-intrinsics. The %-prefixed fix should
   close most gaps. Any remaining gap must be understood before
   proceeding.

Sources:
- [Perceus: Garbage Free Reference Counting with Reuse (PLDI 2021)](https://dl.acm.org/doi/10.1145/3453483.3454032)
- [FP²: Fully in-Place Functional Programming (ICFP 2023)](https://dl.acm.org/doi/10.1145/3607840)
- [Better Static Memory Management (PLDI 1995)](https://www2.eecs.berkeley.edu/Pubs/TechRpts/1995/CSD-95-866.pdf)
- [Tofte-Talpin Region-based Memory Management (1997)](https://dl.acm.org/doi/10.1006/inco.1996.2613)
- [Optimizing Reference Counting with Borrowing (Lorenzen 2022)](https://antonlorenzen.de/master_thesis_perceus_borrowing.pdf)
- [Tail Recursion Modulo Context (Leijen & Lorenzen 2023)](https://www.microsoft.com/en-us/research/wp-content/uploads/2022/07/trmc.pdf)
- [FP² Technical Report (Lorenzen, Leijen, Swierstra 2023)](https://www.microsoft.com/en-us/research/wp-content/uploads/2023/07/fip.pdf)
