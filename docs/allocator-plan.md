# Allocator-Based Memory Management

## Status: Design phase

## Current state

`Value` is `Copy` — a NaN-boxed `u64`. Heap objects live in a thread-local
arena (`Vec<Box<HeapObject>>`). Nothing frees individual objects. An RAII
guard (`ArenaGuard`) handles macro expansion temporaries via mark/release
on the arena Vec. Everything else — bytecode, closures, constant pools —
accumulates for the lifetime of the thread.

### What the arena does

`ArenaGuard` marks the arena length before macro expansion and truncates
back afterward. `Vec::truncate` drops the `Box<HeapObject>`s, which
drops their contents via Rust's normal `Drop`. This works because
`Syntax::from_value()` deep-copies all data out of the Value heap before
the release point — no surviving references into freed memory.

### What the arena doesn't do

Compilation output survives the guard scope. Each `eval` produces
bytecode, closures, and constants that go into the arena and live
forever. In a long-running process (test suite, REPL, LSP, agent loop),
this is unbounded growth:

| Workload | Arena objects/iter |
|----------|-------------------|
| `eval '(+ 1 2)` | 1 |
| `eval '(defn temp (x) x)` | 4 |
| `eval` with `each` macro | 37 |
| `eval` with nested `each` | 142 |

### What the compiler already knows

The analyzer tracks binding metadata that determines value lifetime:

| Field | On | Meaning |
|-------|----|---------|
| `is_captured` | `BindingInner` | A closure closes over this binding |
| `is_mutated` | `BindingInner` | `set!` targets this binding |
| `is_immutable` | `BindingInner` | `def` (not `var`) |
| `scope` | `BindingInner` | `Parameter`, `Local`, or `Global` |
| `CaptureKind` | `CaptureInfo` | `Local`, `Capture { index }`, `Global { sym }` |

The lowerer tracks slot assignments (`binding_to_slot: HashMap<Binding,
u16>`) and scope boundaries (let body entry/exit, function bodies). This
information is sufficient for escape analysis — determining whether a
value outlives its defining scope.

## Design

### The allocator abstraction

An allocator is the unit of memory management. It knows how to allocate
and how to free (individually or in bulk). Different allocators embody
different strategies — bump allocation with bulk reset, pool allocation
with per-object free, slab allocation with size classes.

The runtime uses allocators internally for all heap allocation. Fibers,
scopes, and shared lifetimes are distinguished by *which allocator*
they use, not by separate mechanisms.

```
Fiber private      = a bump allocator, reset on fiber death; no coordination
Scope allocation   = a sub-region within the fiber's bump, reset at scope exit
Shared             = a refcounted bump allocator, reset when last referencing fiber dies
Permanent storage  = the global allocator (Rc::into_raw), never freed
```

These are all allocators with different lifecycle policies. The compiler
decides which allocator to target at each allocation site via escape
analysis. The VM threads the active allocator through the calling
convention.

The key distinction: **private allocators are per-fiber and
uncoordinated** (fast). **Shared allocators are cross-fiber and
refcounted at the allocator level** (not per-object). Values in shared
allocators are visible to multiple fibers without copying — fiber
exchange is zero-copy by construction.

### Why allocators, not regions

A region is one allocation strategy (bump + bulk reset). An allocator
is the general concept. Bump regions, pool allocators, slab allocators,
and the permanent Rc path are all allocators. Framing the design around
allocators means:

- The internal implementation is uniform (one dispatch mechanism)
- Fibers, scopes, and shared lifetimes compose naturally
- The door is open for future strategies (pool, slab) without
  redesigning the runtime
- If the user ever needs explicit control, the abstraction is already
  there — an allocator is a value you can pass to a fiber or scope

The default is invisible. `(cons 1 2)` allocates into the current
allocator — the programmer never sees it. Explicit allocator selection
is a power-user feature for later, not a requirement.

### Two allocator classes: private and shared

Every fiber has a **private allocator** — a bump allocator that owns
all memory local to that fiber. Only the owning fiber allocates into
it; no coordination is needed. Fiber death resets it, running
destructors for types that need `Drop`, then reclaiming all bump
memory in O(1).

Within a fiber, `let` bodies create **scope allocators** — sub-regions
of the fiber's private bump. Scope exit runs destructors and resets
the bump pointer to the scope's mark. Values that don't escape the
scope are freed here.

Values that escape their fiber — passed to a channel, yielded to a
parent fiber, stored in a shared structure — are allocated in **shared
allocators**. A shared allocator is a separate bump allocator with a
refcount tracking how many fibers hold references into it. When the
last fiber drops its reference (typically by dying), the shared
allocator runs destructors and resets.

The granularity is allocator-level, not per-object. If fiber A and
fiber B both reference values in a shared allocator, and fiber A
finishes, the shared allocator stays alive until fiber B finishes too,
even if fiber B only references one value in it. This is coarser than
per-object refcounting but vastly cheaper — one counter per shared
allocator, not per value.

### Zero-copy fiber exchange

Erlang copies values between process heaps on every message send — O(n)
in the size of the message. Elle avoids this entirely. The escape
analysis determines at compile time whether a value might cross a fiber
boundary. If it might, the value is allocated in a shared allocator
from the start. The receiving fiber gets a pointer to something already
in shared space. No copy, no relocation, no forwarding pointers.

Conservatism works in our favor: if the analysis can't prove a value
stays fiber-local, it defaults to shared allocation. That means more
shared allocation than theoretically necessary, but it's always safe
and always zero-copy on exchange. As the escape analysis tightens, more
values stay private (faster allocation, faster bulk free) without
penalizing the sharing case.

This is the fundamental advantage over Erlang's model for
communication-heavy workloads: the cost of sharing moves from runtime
(O(n) copy per send) to compile time (escape analysis), with the
runtime cost being O(1) pointer passing.

### Escape analysis

The compiler classifies each allocation site into one of three targets:

1. **Scope-local** — allocate in the current scope allocator. Freed at
   scope exit.
2. **Fiber-local** — allocate in the fiber's private allocator. Freed
   at fiber death.
3. **Shared** — allocate in a shared allocator. Freed when the last
   referencing fiber dies.

A value escapes its scope if any of these hold:

1. **Captured.** `is_captured == true`. A closure references it.
2. **Returned.** The value is in return position of its function.
3. **Stored outward.** `set!` targets a binding in an outer scope.
4. **Stored into mutable collection.** `put!` into an outer-scope
   array or table.
5. **Passed to unknown function.** The compiler can't see the callee's
   body. Conservatively assume the argument might be retained.

A value escapes its fiber if any of these hold:

1. **Yielded.** The value is in yield position.
2. **Sent to channel.** Passed to a channel-send primitive.
3. **Stored into shared structure.** Written into a structure that is
   itself in a shared allocator.

If nothing triggers scope escape, the value is scope-local. If it
escapes the scope but not the fiber, it's fiber-local. If it might
escape the fiber, it's shared.

**Conservatism is safe.** Over-escaping means a value lives longer than
necessary — the worst case is identical to today's behavior. Under-
escaping is use-after-free (scope-local) or dangling cross-fiber
pointer (fiber-local when it should be shared) — unsound.

The analysis starts maximally conservative and tightens incrementally:
- Phase 1: everything goes to the fiber's private allocator (no change
  from current behavior, but now per-fiber instead of per-thread)
- Phase 2: bindings provably not captured/returned/passed get scoped
- Phase 3: fiber-escape analysis moves cross-fiber values to shared
- Phase 4: callee-aware analysis narrows the escaping set further

### Calling convention: allocator inheritance

Functions are separately compiled. A callee doesn't know the caller's
allocator context. The solution: **the callee inherits the caller's
allocator.** A single pointer on `FiberHeap` tracks the active
allocator; the callee allocates everything — temporaries and return
values — into it.

On `Call`, the VM saves the current allocator pointer and sets it to
the caller's active scope allocator (or fiber allocator if no scope is
active). On `Return`, the VM restores it. Tail calls inherit it
implicitly — the pointer lives on `FiberHeap`, not on the call frame,
so frame reuse preserves it naturally.

This is Tofte-Talpin's region polymorphism encoded in the calling
convention: the function is polymorphic over the allocator its return
value lives in; the caller instantiates that by passing a concrete
allocator. A deep call chain `f → g → h` doesn't create three
allocators. All three functions allocate into whatever allocator was
active at the call site.

**Why one pointer, not two.** A more refined design would separate the
return allocator (for values the caller sees) from a scratch allocator
(for the callee's temporaries, freed on return). This requires two
pointers in the calling convention and requires the compiler to
distinguish return-position allocations from temporaries. The
single-pointer design is simpler, still correct (conservative is safe),
and still delivers per-fiber isolation and scope-based bulk free. The
two-pointer optimization is deferred until profiling shows temporary
accumulation is a real problem.

**Yields:** When a fiber yields mid-call-chain, the allocator pointer
must be saved per suspended frame. `SuspendedFrame` must include it
alongside bytecode, constants, env, ip, and stack. On resume, each
frame restores its allocator pointer.

### `Drop` and destructor tracking

Bump allocation doesn't call `Drop`. Most `HeapObject` variants contain
inner allocations from the global allocator (Vec buffers, BTreeMap nodes,
`Box<str>`, Rc data) that would leak if `Drop` isn't called.

**Types safe to skip:** `Cons` (two Copy Values), `Cell` (Copy Value),
`Float` (scalar), `Binding` (Copy fields), `LibHandle` (index).

**Types needing `Drop`:** `String` (Box<str>), `Array` (Vec<Value>),
`Table` (BTreeMap), `Struct` (BTreeMap), `Closure` (Rc<Closure> with
five inner Rc fields), `Tuple` (Vec<Value>), `Buffer`/`Bytes`/`Blob`
(Vec<u8>), `Syntax` (Rc<Syntax>), `Fiber` (FiberHandle), `ThreadHandle`
(Arc), `FFISignature` (Cif), `ManagedPointer` (FFI memory), `External`
(Rc<dyn Any>).

Each scope allocator maintains a destructor list — a Vec of raw pointers
to HeapObjects that need `Drop`. On scope exit, the list is walked in
reverse, `Drop` is called on each, then the bump resets. For scope
regions with only immediates and cons cells, the destructor list is
empty.

**The destructor list lives outside the bump allocator.** `FiberHeap`
owns the scope marks and their destructor lists as normal Rust
allocations, not bump-allocated.

**Drop must not allocate into the resetting scope.** This constraint
is inherited from the current `ArenaGuard` (documented in `heap.rs`
lines 430-437). When Drop cascades through Rc chains (e.g.,
`Rc<Closure>` dropping inner Rc fields), the Rc-managed data is on the
global heap, not in the bump — the cascade doesn't interact with the
resetting scope.

**Fiber death:** Walk all scope destructor lists, then all shared
allocator destructor lists, then reset the fiber's bump. This is O(N)
in destructor-tracked objects, then O(1) for the bulk memory
reclamation.

### The Closure problem

`HeapObject::Closure(Rc<Closure>)` is the most common heap type. The
`Closure` struct contains five `Rc` fields and two `Option<Rc>` fields
(`bytecode`, `constants`, `env`, `symbol_names`, `location_map`,
`jit_code`, `lir_function`). `Frame` and `SuspendedFrame` hold `Rc`
clones for sharing without copies.

The `HeapObject::Closure` wrapper is bump-allocated. The inner `Closure`
data lives on the global heap (via `Rc::new`). Destructor-tracking the
wrapper calls `Drop` on the `Rc<Closure>`, decrementing the refcount.
If `Frame` or `SuspendedFrame` also hold clones, the inner data
survives. The `Closure` is freed when the last `Rc` clone drops —
typically at fiber death when the `frames` Vec is dropped.

Start with destructor tracking for closures. Removing `Rc` from
`Closure` entirely (storing data directly in the allocator, using raw
pointers in Frame/SuspendedFrame) is the clean long-term solution but
a separate refactor.

### Interaction with existing systems

**ArenaGuard** maps to a scope allocator. Macro expansion marks the
fiber's bump before expansion and resets after `from_value()` extracts
the result. The RAII pattern is retained for the Rust-level scope.

**Constant pools** are allocated in the fiber root allocator. For
`eval`'d code, the constants are part of the closure's Rc-managed
data on the global heap — they survive scope resets via Rc semantics.

**Globals** (`VM.globals: Vec<Value>`) outlive any fiber. Values stored
in globals must live in a process-global allocator (never freed, same
as today's `alloc_permanent` path). Process-global allocation for
globals is the simplest correct approach.

**JIT code** calls `alloc()` directly. Under the allocator system, JIT
code must route through the active allocator. This is deferred until
the allocator infrastructure is proven.

## Bytecodes

| Instruction | Operand | Semantics |
|-------------|---------|-----------|
| `RegionEnter` | (none) | Push a new scope allocator onto the fiber's scope stack |
| `RegionExit` | (none) | Pop the top scope allocator, run destructors, reset bump |
| `AllocShared` | allocator_id (u16) | Set allocation target to the specified shared allocator |

`AllocShared` sets a target consumed by the next heap allocation. The
compiler emits it immediately before the expression producing the
escaping value. Two back-to-back `AllocShared` instructions overwrite
(last writer wins). On `RegionExit`, assert that the target is clear.

## Implementation sequence

### Package 1: Per-fiber allocator routing (no behavioral change)

Create `FiberHeap`. Add it to `Fiber`. Route `alloc()` through a
thread-local "current fiber heap" pointer, swapped on fiber transitions.
Initially wraps the same `Vec<Box<HeapObject>>` strategy. `ArenaGuard`
operates on the fiber heap instead of the global arena.

All tests pass unchanged. The change is: allocation is owned by a
fiber-scoped struct instead of a thread-scoped static.

### Package 2: Bump allocator with destructor tracking (no behavioral change)

Replace `Vec<Box<HeapObject>>` in `FiberHeap` with `bumpalo::Bump`.
Add destructor tracking: on `alloc()`, if the HeapObject variant needs
`Drop`, push its pointer onto the current scope's destructor list.
On `ArenaGuard` release, walk the destructor list then reset the bump.

**Destructor tracking is mandatory from day one.** The `ArenaGuard` in
macro expansion depends on mark/release calling Drop (Vec::truncate
drops Box contents). Without destructor tracking, the bumpalo switch
leaks every String, Array, Closure, etc. created during macro expansion.

### Package 3: Scope bytecodes (no behavioral change)

Add `RegionEnter`/`RegionExit` instructions. Emit at `let`/`letrec`/
`block` boundaries. Handle as no-ops in the VM — they push/pop scope
marks but don't restrict allocation. Function bodies do NOT get scope
allocators; they allocate into the caller's context.

Scope exit on early-exit paths (break, return, exception unwind) is
deferred — these are no-ops so missing them is harmless. But the gap
must be documented as debt for Package 5.

### Package 4: Allocator inheritance plumbing (no behavioral change)

Add `active_allocator` pointer to `FiberHeap`. Save/restore on
Call/Return. Add to `SuspendedFrame` for yield/resume. Initialize to
the fiber's root bump on fiber creation. Tail calls inherit implicitly
(pointer on FiberHeap, not on the frame). Single pointer — callee
inherits the caller's allocator for everything.

All write-only until Package 5 — the allocator doesn't read it yet.
Add debug assertions that the pointer is always valid.

### Package 5: Scope allocation (first behavioral change)

The allocator routes to scope allocators by default. `RegionExit`
actually frees memory. The escape analysis starts maximally
conservative: only bindings not captured, not returned, not mutated
outward, and not passed to any function call are scoped.

Scope exit on all control flow paths becomes mandatory. The lowerer
uses a "pending scope exits" stack: when emitting any non-local exit
(Return, Break, exception), emit `RegionExit` for all pending scopes.

The smallest demo: `(let [x (cons 1 2)] (+ (car x) (cdr x)))` — the
cons cell is freed at scope exit, measurably different from today.

### Package 6: Shared allocators and fiber-escape analysis (deferred)

Shared allocators get their own bumps and allocator-level refcounts.
Each fiber that holds references into a shared allocator increments
the count; fiber death decrements it. When the count hits zero, the
shared allocator runs destructors and resets — no per-object
refcounting, no tracing.

This package also introduces fiber-escape analysis (Phase 3 of the
escape analysis): values that might cross fiber boundaries (yields,
channel sends, storage into shared structures) are allocated into
shared allocators from the start. This is what makes fiber exchange
zero-copy — the receiving fiber gets a pointer to something already
in shared space.

Deferred until Package 5 is stable and fiber communication patterns
are exercised enough to validate the analysis.

## Open questions

1. **Destructor tracking overhead.** Measure the Vec push per allocation
   and linear walk per scope exit against the current arena.

2. **Scope granularity.** `RegionEnter`/`RegionExit` at every `let` body
   may be too fine-grained. Measure and coarsen if needed.

3. **Closure Rc removal.** When to remove Rc from Closure (raw pointers
   in Frame/SuspendedFrame, data in the allocator directly)?

4. **bumpalo reset and pointer stability.** After reset, any Value
   holding a pointer into the reset scope is dangling. Safety depends on
   escape analysis correctness — the same unsoundness as today's arena,
   but structured.

5. **First-class allocators.** The internal `ElleAllocator` trait supports
   future user-facing allocator selection (e.g., pool allocator for a
   fiber, arena for a scope). Expose when demonstrated need exists.

6. **Shared allocator granularity.** One shared allocator per fiber group?
   Per channel? Per "communication epoch"? Coarser granularity means less
   bookkeeping but more memory retained. Finer granularity means more
   counters but earlier reclamation. The right answer depends on real
   fiber communication patterns — defer until Package 6.

7. **Shared allocator and long-lived fibers.** If fiber A sends many
   values to fiber B over time, each in a different shared allocator,
   fiber B accumulates references to many shared allocators. If fiber A
   dies, those allocators free. But if fiber A is long-lived, the shared
   allocators accumulate. May need a "generation" or "epoch" mechanism
   within the shared allocator space — investigate when real workloads
   surface the pattern.

## Superseded documents

`docs/heap-arena-plan.md` describes the current arena mechanism. This
design builds on it and eventually replaces it.
