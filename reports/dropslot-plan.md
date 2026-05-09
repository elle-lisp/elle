# Full plan: compile-time reclamation for Elle

## Scope

Four phases, each shippable independently, each building on the previous.
The grpc-leak (yielding fiber memory growth proportional to yield count)
is a non-negotiable acceptance criterion — fiber death is an optimization
of reclamation, not a backstop against unbounded growth.

## Phase 1: DropSlot infrastructure

### The pool.allocs compatibility problem

DropSlot frees a slab slot but must not leave a stale entry in
pool.allocs (causes double-free when release() later covers the range,
or when the slot is recycled and added to pool.allocs again).

**Solution: per-slot "dropped" bitmap in the Slab.**

Add a bitmap (`Vec<u64>`, 1 bit per slot, 32 bytes per 256-slot chunk)
to Slab. Three operations:

- `mark_dropped(flat)` — set bit. Called by DropSlot.
- `is_dropped(flat)` — check bit. Called by release() before dealloc.
- `clear_dropped(flat)` — clear bit. Called by alloc() when reusing a
  slot (so the new allocation isn't skipped by a future release).

DropSlot: run destructor, call slab.dealloc(ptr), mark_dropped(flat),
decrement alloc_count. Does NOT touch pool.allocs or pool.dtors.

release(): iterate [mark.allocs_len..allocs.len()] in reverse. For
each entry, check is_dropped(flat). If true, skip (already freed).
If false, dealloc as usual. Same for dtors: skip dropped entries
before running drop_in_place.

Truncate pool.allocs/dtors as usual — the stale entries are past the
truncation point and disappear.

alloc(): when reusing a free-list slot, clear_dropped(flat). When
bumping a fresh slot, no action (bitmap is zero-initialized per chunk).

**Complexity:** DropSlot is O(1). release() is O(n) same as today, plus
one bitmap check per entry (O(1) each). alloc() adds one bit clear
when reusing a slot (O(1)). Net overhead: ~32 bytes per 256-slot chunk.

**Why not an intrusive linked list:** pool.allocs has operations that
don't map cleanly — release_refcounted() does in-place compaction via
forward-loop indexing, pop_call_scope_marks_and_release() drains a
middle range. Both are O(n) on Vec today and would be O(n) on a linked
list too. The bitmap achieves O(1) DropSlot without changing the Vec's
operational semantics. If profiling later shows the Vec iteration is a
bottleneck (unlikely — typical scopes have 5-50 entries), the linked
list is available as an optimization, not a correctness fix.

### DropSlot bytecode

New instruction: `DropSlot(reg)`

```rust
Instruction::DropSlot => {
    let reg = read_reg(bytecode, &mut ip);
    let val = stack[reg];
    if val.is_heap() {
        if let Some(ptr) = val.as_heap_ptr() {
            let ho = ptr as *mut HeapObject;
            fiberheap::with_current_heap_mut(|h| {
                if h.pool_owns(ptr) {
                    let flat = h.slab_flat_index(ho);
                    if needs_drop_for_ptr(ho) {
                        unsafe { std::ptr::drop_in_place(ho) };
                    }
                    unsafe { h.pool.dealloc_slot(ho) };
                    h.mark_dropped(flat);
                    h.decrement_alloc_count();
                }
            });
        }
    }
    stack[reg] = Value::NIL;  // idempotent under SIG_FUEL replay
}
```

### Dead-set analysis (new code in lowerer)

At each TailCall emission point:

```rust
fn bindings_dead_at_tailcall(
    scope_bindings: &[(Binding, &Hir)],
    tail_args: &[CallArg],
) -> Vec<Binding> {
    let referenced: HashSet<Binding> = tail_args.iter()
        .flat_map(|arg| collect_var_refs(&arg.expr))
        .collect();
    scope_bindings.iter()
        .filter(|(b, _)| !referenced.contains(b))
        .map(|(b, _)| *b)
        .collect()
}
```

For parameters: same query but over the function's param bindings.

### Emission changes

1. **Before TailCall:** for each dead binding/param, load into register
   (if not already), emit DropSlot. For dead params: emit LoadCapture
   then DropSlot.

2. **begin in tail position:** when lowering `(begin expr1 expr2 ...
   tailcall)`, emit DropSlot instead of Pop for each discarded
   expression result. The discarded value may be heap-allocated; the
   DropSlot runtime tag check handles immediates.

3. **Let scopes with tail-call body:** do NOT emit RegionEnter. The
   RegionExit is unreachable (tail call replaces frame). DropSlot
   handles dead bindings. Call-scoped reclamation (RegionExitCall)
   within the body handles intermediates — it pushes its own marks,
   independent of the let scope.

### What to remove

Everything listed in dropslot-critique.md "REMOVE outright" table.
The trampoline becomes stateless w.r.t. the heap.

### Validation

All 13 tiers of leak.lisp without the `checked?` bypass.
tailcall-reclaim.lisp bounded. Paper trace-throughs for t2-struct,
t2-mutual, t14 patterns. Contracts JIT failure investigated.

---

## Phase 2: Yield-boundary reclamation (grpc-leak fix)

### Root cause (verified in code)

`with_child_fiber` (fiber.rs:46-146) calls `install_outbox()` before
each child execution. install_outbox creates a fresh SlabPool and moves
the previous outbox to `old_outboxes: Vec<Box<SlabPool>>`. Outboxes
are ONLY freed on fiber death (`clear()` / `Drop`).

After N yields, old_outboxes contains N-1 SlabPools. Memory grows
linearly with yield count. For a long-lived gRPC bidi fiber yielding
thousands of times, this is unbounded growth bounded only by connection
death.

### The problem with freeing old outboxes

The parent holds Values that point into old outbox pools. If the parent
stored a yielded value (e.g., pushed into an array), freeing the outbox
would dangle that reference.

### Solution: compile-time escape analysis at resume sites

The compiler already analyzes whether values escape to external mutable
state (rotation_safe, walk_for_outward_set). Apply the same analysis
to the resume handler: the code that runs between `(fiber/resume f)`
and the next `(fiber/resume f)`.

If the yielded value does NOT escape the resume handler (it's consumed
and discarded within the let scope or while body), the old outbox can
be freed at the next resume. The compiler emits a flag or instruction
at the resume site indicating "previous outbox is reclaimable."

```lisp
; Escape analysis on the resume handler:
(while (not= (fiber/status f) :dead)
  (let [result (fiber/resume f)]  ; result is the yielded value
    (send-response result)))       ; does send-response escape result?
; If send-response is a primitive that doesn't store result externally,
; result is dead after the call. The old outbox is reclaimable.
```

### Implementation

1. **New analysis: resume_value_escapes(handler_body, resume_binding)**
   — does the yielded value escape the handler? Reuses the existing
   outward-set analysis machinery.

2. **New bytecode: OutboxReclaim** — emitted after the last use of the
   yielded value in a non-escaping handler. At runtime: tears down the
   oldest outbox in old_outboxes. O(1) — just pop and teardown one
   pool.

3. **Alternative (simpler, less precise):** at each `install_outbox()`,
   if the previous outbox's values are not referenced by any pinned
   (refcount > 0) object in the parent's pool, tear it down. This is a
   runtime refcount check, not a compile-time analysis. Less desirable
   but correct.

4. **Alternative (simplest, correct, not optimal):** deep-copy the
   yielded value from the outbox to the parent's pool at each resume.
   Then tear down the old outbox immediately. Cost: O(value_size) copy
   per resume. For small values (ints, keywords, small structs), this
   is negligible. For large values (big strings), it's proportional but
   unavoidable.

### Validation

grpc-leak test must show bounded growth: d100/d10 ratio < 5.0. The
regression marker assertion flips from `(%gt ratio 5.0)` (asserting
leak) to `(%lt ratio 5.0)` (asserting bounded).

---

## Phase 3: Type-directed reuse analysis

### What the compiler knows today

The type system (hir/types.rs) covers only numeric types: Int, Float,
Number. No compound types. The comment at line 35 says "reserved for
future compound types (Pair, Closure, etc.)."

The escape analysis reduces all heap values to a binary: "immediate" or
"heap-allocated." No distinction between Pair, LString, LStruct,
Closure.

### What the compiler CAN know

For many allocation forms, the HeapTag is syntactically determined:

| HIR form | HeapTag | Known at compile time? |
|----------|---------|----------------------|
| `(cons x y)` / `(pair x y)` | Pair | Yes |
| `(list x y z)` | Pair chain | Yes |
| `(concat a b)` / `(string ...)` | LString | Yes |
| `{:x 1}` | LStruct | Yes |
| `@{:x 1}` | LStructMut | Yes |
| `[1 2 3]` | LArray | Yes |
| `@[1 2 3]` | LArrayMut | Yes |
| `(fn [] ...)` | Closure | Yes |
| `(f x)` (user function) | Unknown | No (without type inference) |

### Extending type inference

Add heap types to TyId:

```rust
enum TyCategory {
    Immediate(ImmediateType),  // existing: Int, Float, Bool, etc.
    Heap(HeapType),            // new
    Unknown,
}

enum HeapType {
    Pair,
    String,
    Struct { field_count: Option<u16> },
    MutableStruct,
    Array { len: Option<u16> },
    MutableArray,
    Closure,
    // ... other HeapTags as needed
}
```

Propagate through:
- Literal forms → known HeapType
- Primitive calls → known HeapType (cons → Pair, concat → String)
- User function calls → propagate return type through fixpoint
  (same framework as callee_result_immediate / callee_return_safe)
- If/cond/match → union of branch types
- Let → body type

### Reuse tokens (FP² model)

When DropSlot frees a value of known HeapType T, and the next iteration
allocates a value of the same HeapType T with the same size, the
compiler can emit:

```
ReuseSlot(dst, src_reg)  ; reuse src_reg's freed slot for dst's allocation
```

Instead of DropSlot(src_reg) + alloc(T), emit ReuseSlot which:
1. Writes the new HeapObject data into the same slot
2. Returns the same pointer (no free-list round-trip)

This eliminates both the deallocation AND the allocation. Net allocator
operations: zero. The function runs in O(1) memory with zero allocator
overhead. This is FP²'s "fully in-place" property.

### When reuse applies

For self-tail-calls where:
- A dead param has HeapType T
- The corresponding tail-call arg allocates HeapType T
- The sizes match (same HeapTag, compatible layout)

Example:
```lisp
(defn f [n s]
  (f (- n 1) (concat "new-" (number->string n))))
; s is dead (HeapType::String), arg 1 allocates HeapType::String
; → ReuseSlot: reuse s's slab slot for the new concat result
```

### When reuse does NOT apply

- Different HeapTypes (dead Pair, new String): DropSlot + alloc
- Unknown type (user function return): DropSlot + alloc
- Non-self-tail-call (mutual recursion): DropSlot + alloc (types may
  differ across functions)
- Variable-size mismatch (dead String of 5 bytes, new String of 100
  bytes): DropSlot + alloc (slab slots are fixed-size, so this actually
  DOES work — all HeapObjects are the same size. Only InlineSlice data
  in the bump arena differs.)

Wait — slab slots are fixed-size (one HeapObject each). The HeapObject
contains an InlineSlice pointer to variable-size data in the bump arena.
ReuseSlot can reuse the slab slot but the bump arena data is separate.
So ReuseSlot always works for the slab slot (same size). The bump arena
data is allocated fresh regardless.

This means reuse tokens are ALWAYS applicable when HeapTypes match —
no size check needed. The slab slot is reused; the bump arena data is
new.

### Validation

Benchmark tail-call loops with and without reuse tokens. The reuse case
should show zero net slab growth AND zero free-list operations (no
dealloc, no alloc — just an in-place write).

---

## Phase 4: Eliminate fiber-death dependency

### Principle

Every reclamation mechanism should bound memory by *operation count*
(iterations, yields, requests), not by *entity lifetime* (fiber death,
connection close, process exit). Fiber death is an optimization — it
reclaims everything at once — not a correctness backstop.

### What this means concretely

| Pattern | Current bound | Required bound |
|---------|--------------|----------------|
| Tail-call loop (no yield) | Fiber death | Per-iteration (Phase 1) |
| While loop (no yield) | Per-iteration (RegionExit) | Already correct |
| While loop (yield) | Per-iteration (RegionRotate) | Already correct (if analysis fires) |
| Tail-call + yield | Fiber death | Per-iteration (Phase 1 + 2) |
| gRPC bidi stream | Connection death | Per-RPC (Phase 2) |
| Long-lived fiber (event loop) | Fiber death | Per-event (Phase 1 + 2) |

### What "done" looks like

Every test in leak.lisp passes without the `checked?` bypass.
The grpc-leak test asserts bounded growth (ratio < 5.0).
No test's reclamation depends on fiber death.

## Ordering and dependencies

```
Phase 1: DropSlot (pool.allocs bitmap + bytecode + dead-set analysis)
  ↓
Phase 2: Yield reclamation (outbox teardown at resume boundaries)
  ↓  ↓
  ↓  Phase 3: Type inference + reuse tokens (optimization, not correctness)
  ↓
Phase 4: Audit — verify no pattern depends on fiber death for bounding
```

Phase 1 and Phase 2 are both correctness requirements. Phase 3 is an
optimization that reduces allocator overhead from O(1)-per-drop to
zero. Phase 4 is verification.

Phase 2 can start in parallel with Phase 1 if the outbox deep-copy
approach is used (it doesn't depend on DropSlot). The compile-time
escape analysis approach for outbox reclamation does depend on Phase 1's
dead-set analysis infrastructure.

## Guardrails (final version)

1. **You're adding runtime state to the trampoline.** The trampoline
   is pure control flow. If it touches the heap, the design is wrong.

2. **A test requires `checked?` to skip its assertion.** Fix the
   analysis, don't gate the test.

3. **You're reasoning about previous iterations.** DropSlot reasoning
   is local. Cross-iteration reasoning means the design has regressed.

4. **You're proposing a mechanism from the rejected table.** Pointer
   snapshots, generation scanning, refcount-pinned rotation,
   release_between, scope-mark rotation for tail calls, bump-arena
   double-buffer, SwapPool / FlipFrame — all rejected. Any proposal
   that is one of these with a different name is rejected.

5. **Memory growth is bounded by entity lifetime, not operation count.**
   If the bound is "fiber death" or "connection close," the design
   leaks. Bound by iteration count or yield count.

6. **You claim a mechanism is O(1) without verifying against
   pool.allocs.** Check: does DropSlot leave a stale entry? Does
   release() skip it? Does alloc() clear the flag on reuse? Trace
   through alloc → DropSlot → alloc(reuse) → release covering the
   original position.

7. **You haven't traced through the grpc-leak scenario.** Before
   claiming yield-boundary reclamation works: trace 3 yields, showing
   outbox creation, old_outbox accumulation, and reclamation. Show
   where the yielded value lives, who references it, and when the
   outbox is torn down.
