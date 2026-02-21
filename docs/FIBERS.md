# Fibers: Implementation Architecture

This document translates the design in `EFFECTS.md` into concrete Rust
structures, bytecode changes, and a phased implementation plan. An engineer
should be able to read this and build the system.


## Overview

The current VM has three separate control-flow mechanisms:

1. **Exception handlers** — `exception_handlers` stack on VM, `PushHandler`/
   `PopHandler` instructions, `current_exception` field, `Condition` type
2. **Coroutines** — `Coroutine` struct, `ContinuationData`/`ContinuationFrame`,
   `coroutine_stack` on VM, `Yield` instruction, `VmResult::Yielded`
3. **Effect inference** — `Effect { yield_behavior, may_raise }` struct,
   `YieldBehavior` enum with `Pure`/`Yields`/`Polymorphic`

These are replaced by a single mechanism: **fibers with signals**.


## Phase 1: Fiber as Execution Context

### Goal

Move execution state out of VM and into Fiber. The VM becomes a stateless
dispatch engine. All existing behavior (exceptions, coroutines, yields)
continues to work through the fiber mechanism.

### The Fiber struct

```rust
/// Signal type bits. The first 16 are compiler-reserved.
pub type SignalBits = u32;

pub const SIG_OK:       SignalBits = 0;        // no bits set = normal return
pub const SIG_ERROR:    SignalBits = 1 << 0;   // exception / panic
pub const SIG_YIELD:    SignalBits = 1 << 1;   // cooperative suspension
pub const SIG_DEBUG:    SignalBits = 1 << 2;   // breakpoint / trace
pub const SIG_RESUME:   SignalBits = 1 << 3;   // fiber resumption request

/// Signal bit partitioning:
///
///   Bits 0-2:  User-facing signals (error, yield, debug)
///   Bit  3:    VM operation (resume) — not visible to user code
///   Bits 4-15: Reserved for future use
///   Bits 16-31: User-defined signal types
///
/// The VM dispatch loop checks all bits. User code only sees
/// bits 0-2 and 16-31. Bits 3-15 are internal.

/// Fiber status. Matches Janet's model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberStatus {
    /// Not yet started (has closure but hasn't been resumed)
    New,
    /// Currently executing (on the VM's run stack)
    Alive,
    /// Suspended by a signal (waiting for resume)
    Suspended,
    /// Completed normally (returned a value)
    Dead,
    /// Terminated by an unhandled error signal
    Error,
}

/// A single call frame within a fiber.
#[derive(Debug, Clone)]
pub struct Frame {
    /// The closure being executed
    pub closure: Rc<Closure>,
    /// Instruction pointer (byte offset into bytecode)
    pub ip: usize,
    /// Base index in the fiber's operand stack for this frame's temporaries
    pub base: usize,
}

/// The fiber: an independent execution context.
pub struct Fiber {
    /// Operand stack (temporaries — locals are in Frame.closure.env)
    pub stack: Vec<Value>,
    /// Call frame stack
    pub frames: Vec<Frame>,
    /// Current status
    pub status: FiberStatus,
    /// Signal mask: which of this fiber's signals are caught by its parent.
    /// Set at creation time by the parent. Immutable after creation.
    /// The parent checks child.mask & bits to decide catch vs. propagate.
    pub mask: SignalBits,
    /// Parent fiber (Weak to avoid Rc cycles)
    pub parent: Option<Weak<RefCell<Fiber>>>,
    /// Most recently resumed child (for stack traces, not ownership)
    pub child: Option<Rc<RefCell<Fiber>>>,
    /// The closure this fiber was created from
    pub closure: Rc<Closure>,
    /// Dynamic bindings (fiber-scoped state)
    pub env: Option<HashMap<u32, Value>>,
    /// Signal value from this fiber. Canonical location for both
    /// signal payloads and normal return values.
    /// - On signal: set to (bits, payload) before suspending
    /// - On normal return: set to (0, return_value) before completing
    /// Read via (fiber/value f). Always valid after status is not :new/:alive.
    pub signal: Option<(SignalBits, Value)>,
}
```

### What moves from VM to Fiber

| Currently on VM | Moves to | Notes |
|-----------------|----------|-------|
| `stack` | `Fiber.stack` | Operand stack |
| `call_stack` | `Fiber.frames` | But Frame carries real state now |
| `call_depth` | `Fiber.frames.len()` | Derived |
| `exception_handlers` | Removed | Replaced by signal mask on fiber |
| `current_exception` | `Fiber.signal` | Fiber.signal is canonical; run() returns SignalBits only |
| `handling_exception` | Removed | Implicit in fiber chain structure |
| `coroutine_stack` | `Fiber.child` chain | Parent-child fiber chain |
| `pending_yield` | Removed | Yield-from becomes fiber delegation |

### What stays on VM

| Field | Why it stays |
|-------|-------------|
| `globals` | Shared across all fibers |
| `ffi` | Shared FFI subsystem |
| `modules` | Shared module registry |
| `jit` | Shared JIT cache |
| `hotness` | Shared profiling data |
| `locations` | Top-level location map |
| `scope_stack` | Removed — redundant with lexical scoping via Frame.closure.env |
| `pending_tail_call` | Transient — tail calls complete within one dispatch iteration |

### The VM after Phase 1

```rust
pub struct VM {
    /// Currently executing fiber — owned directly, no Rc/RefCell overhead.
    /// Suspended fibers are wrapped in Rc<RefCell<Fiber>> when stored as Values.
    /// On resume, the child is swapped in; on suspend, swapped out.
    pub fiber: Fiber,
    /// Global bindings (shared across fibers)
    pub globals: Vec<Value>,
    /// FFI subsystem
    pub ffi: FFISubsystem,
    /// Module registry
    pub modules: HashMap<String, HashMap<u32, Value>>,
    pub module: Option<String>,
    pub loaded: HashSet<String>,
    pub search_paths: Vec<PathBuf>,
    /// JIT cache (shared)
    pub jit: HashMap<*const u8, Rc<JitCode>>,
    pub hotness: HashMap<*const u8, usize>,
    /// Top-level location map
    pub locations: LocationMap,
    pub source_loc: Option<SourceLoc>,
    /// Tail call scratch buffer (transient, reused across calls)
    pub tc_cache: Vec<Value>,
    /// Pending tail call info (transient — never crosses a suspension boundary)
    pub pending_tail_call: Option<(Rc<Vec<u8>>, Rc<Vec<Value>>, Rc<Vec<Value>>)>,
}
```

### The run function

The core execution loop changes from a method on VM to a function that
takes a VM and a Fiber:

```rust
/// Execute bytecode in the current fiber until it returns or signals.
///
/// Returns the signal bits only. The signal value (if any) is stored
/// on the fiber's `signal` field — the single source of truth.
/// Normal return: bits == 0, result is on the fiber's operand stack.
/// Signal: bits != 0, value is in fiber.signal.
fn run(vm: &mut VM) -> SignalBits {
    // The dispatch loop works on vm.fiber (owned directly).
    // When a call happens, a new Frame is pushed onto vm.fiber.frames.
    // When a return happens, a Frame is popped.
    // When a signal happens:
    //   1. The value is stored in vm.fiber.signal
    //   2. vm.fiber.status → Suspended
    //   3. run() returns the signal bits
    //
    // On resume (child fiber):
    //   1. Parent fiber is moved into Rc<RefCell<Fiber>>
    //   2. Child fiber is swapped into vm.fiber
    //   3. run() is called recursively
    //   4. When child returns/signals, parent is swapped back in
    //   5. If child signaled and child.mask catches it, parent handles
    //   6. If not caught, child stays suspended, parent also suspends
    //      (signal propagates — entire chain freezes)
}

// Note: recursive run() means the Rust call stack grows with fiber
// nesting depth. Deep fiber chains may stack overflow. A trampoline
// (iterative run loop) is future work if this becomes a problem.
```

### Signal emission and handling

When code emits a signal (fiber/signal):

1. Signal value is stored in `fiber.signal`
2. Current fiber's status → `Suspended`
3. `run()` returns the signal bits to the parent's resume callsite
4. Parent checks: `child.mask & bits != 0`?
   - **Caught**: Parent handles the signal. The child is suspended
     and reachable via the parent's `child` pointer.
   - **Not caught**: The parent also suspends (storing the same signal
     bits in its own `signal` field). The signal propagates up the
     chain. The entire chain from signaler to eventual handler freezes.

This is Janet's propagation model. When a handler catches a signal,
it can walk the `child` chain to find the originating fiber. Every
fiber in the chain is suspended and inspectable via `fiber/value`.

Non-unwinding recovery works at any depth: the handler resumes the
originator directly. Intermediary fibers resume automatically as
their children return normally.

When an intermediary propagates a signal (doesn't catch it), it stores
`(bits, Value::NIL)` in its own `signal` field — the bits record why it
suspended, but the payload is on the originator. Walk `fiber/child` to
find the originating fiber and read its `fiber/value` for the payload.

### Child lifecycle

The `child` field tracks the most recently resumed child fiber, not
all children. A fiber can create and resume many children over its
lifetime, but `child` only points to the current one.

`child` is set when the parent resumes a child fiber. It is cleared
when:
- The child completes normally (status → Dead)
- The parent resumes a different child (replaced)
- The parent itself completes

Suspended children remain alive as long as some reference (variable,
data structure) holds their `Rc<RefCell<Fiber>>`. An orphaned
suspended fiber (no references) is garbage collected normally via Rc
drop.

### Exception handling becomes signal handling

Current `try`/`catch`/`finally` compiles to fiber operations:

```
1. Create a child fiber with mask = SIG_ERROR
2. Resume the child fiber
3. If child returns normally → use the value
4. If child signals SIG_ERROR → run the catch clause in the parent
5. If finally clause exists → run it unconditionally
```

Phase 1 can keep the existing PushHandler/PopHandler bytecode and
translate it to fiber-local signal handling internally. The surface
syntax migration from `handler-case` to `try`/`catch` happens in
Phase 5.

### Coroutines are not a type

A coroutine is a usage pattern, not a type. It's a fiber whose closure
yields. `(fiber/new my-fn :yield)` creates a fiber — whether you call
that a "coroutine" depends on how you use it. There is no `Coroutine`
heap type in the new model; there are closures and fibers.

The current Coroutine type:
```rust
Coroutine { closure, state, yielded_value, saved_value_continuation, delegate }
```

Becomes a Fiber with `mask: SIG_YIELD`:
```rust
Fiber { closure, status: New, mask: SIG_YIELD, ... }
```

- `fiber/new` → create a Fiber with a signal mask
- `fiber/resume` → resume the Fiber; returns signal bits only; value is in `fiber.signal`, readable via `fiber/value`
- `yield` → emit SIG_YIELD signal
- `fiber/status` → check `fiber.status`
- `yield-from` → set `fiber.child` to the delegate fiber


### Unified primitive signature

All primitives have one signature:

```rust
type PrimitiveFn = fn(&[Value]) -> (SignalBits, Value);
```

/// When the VM dispatch loop calls a primitive and it returns
/// (bits, value), the VM stores the value in the current fiber's
/// `signal` field (if bits != 0) or pushes it onto the operand
/// stack (if bits == 0). This bridges the primitive's tuple return
/// with the fiber's single-source-of-truth signal field.

No primitive has VM access. `VmAwareFn` is eliminated. Operations that
formerly needed the VM now emit signals:

| Old pattern | New pattern |
|-------------|------------|
| `vm.current_exception = err` | Return `(SIG_ERROR, error_value)` |
| `vm.execute_bytecode(...)` | Return `(SIG_RESUME, fiber)` |
| `vm.set_pending_yield(v)` | Return `(SIG_YIELD, v)` |
| `vm.enter_coroutine(...)` | Return `(SIG_RESUME, fiber)` |

The VM dispatch loop is the sole signal handler. Primitives are pure
signal emitters — they describe what should happen, the VM makes it
happen.

Primitive metadata is captured at registration time:

```rust
struct PrimInfo {
    func: PrimitiveFn,
    effect: Effect,
    arity: Arity,
    doc: Option<&'static str>,
}
```

This replaces `NativeFn`, `VmAwareFn`, and the separate effect
registration map with a single structure.


#### Symbol table access

Several primitives (`symbol->string`, `display`, `type-of`) need to
resolve symbol IDs to names. Currently they reach through `unsafe`
thread-local raw pointers to the `SymbolTable`. This must be solved
when `VmAwareFn` is eliminated. Three options, in order of disruption:

**Option A: SIG_LOOKUP signal.** Primitives that need a symbol name
return `(SIG_LOOKUP, symbol_id_value)`. The VM resolves the name and
re-invokes the primitive with the resolved string. Least code change —
the symbol table stays where it is, primitives stay pure, and the VM
mediates. Cost: round-trip per lookup.

**Option B: Frozen Arc snapshot.** The symbol table's `names: Vec<Rc<str>>`
is append-only. After startup, freeze it into an `Arc<Vec<Rc<str>>>` and
install as a safe thread-local. Primitives call `symbol_name(id)` — no
VM, no raw pointers. Cost: thread-local access per lookup, snapshot
refresh on runtime interning.

**Option C: Symbols carry their name.** A `Value::symbol(id)` stores
(or caches) a reference to the interned string. `symbol->string` just
extracts it. Zero-cost reads. Cost: changes symbol representation,
slightly larger symbol values or an extra indirection.

All three are compatible with the unified primitive signature. The
choice can be made during implementation based on what falls out most
naturally. The constraint is: no `unsafe`, no `VmAwareFn`, no raw
pointers to the symbol table.


#### Higher-order primitives and VM operations

**Higher-order functions** (`map`, `filter`, `fold`, `apply`) need to
call closures multiple times. They cannot work with the unified
signature because a primitive can only return one signal per call.
These are implemented in Elle as stdlib functions, not as primitives.
The JIT can optimize them.

**VM operations** (`eval`, `load`, `import-file`) need to compile
and execute code. These remain as VM-level operations (instructions
or special-cased signal handlers), not user-callable primitives in the
traditional sense. `eval` compiles source text and resumes the result
as a fiber. `load` reads a file, compiles it, and executes it. These
are multi-step operations that the VM orchestrates.


### Restarts via signal/resume

The fiber model supports non-unwinding error handling without special
mechanism. See `EFFECTS.md` for the full rationale. The short version:

- A fiber signals and **suspends** (frames intact, not unwound)
- The signal payload includes available recovery options as data
- A handler anywhere up the fiber chain catches the signal
- The handler resumes the suspended child with a recovery choice
- The child dispatches on the resume value and continues

Signals travel **up** the chain (parent links). Recovery choices travel
back **down** the chain (resume calls). This gives us:

- `try`/`catch` (unwinding) = catch signal, don't resume child
- Non-unwinding recovery = catch signal, resume child with value

No new instructions, types, or VM support needed — just patterns over
`fiber` + `signal` + `resume`.


### Terminal vs. resumable signals

Whether a signal is "terminal" or "resumable" is a **handler decision**,
not a signal property. Any signal leaves the child fiber in `Suspended`
status. The handler either:

- **Resumes** the child (delivering a value) → resumable behavior
- **Doesn't resume** the child (lets it be GC'd) → terminal behavior

There is no flag on signal types marking them terminal. An uncaught
`SIG_ERROR` at the root fiber is terminal by convention (the root has
nowhere to propagate), but the same error caught by a parent is
resumable if the parent chooses to resume the child.

This is simpler and more powerful than designating signal types as
terminal vs. resumable at definition time.


### Resume value destination

When a suspended fiber is resumed with a value via `fiber/resume`,
the value is pushed onto the fiber's operand stack. The fiber's IP
points to the instruction *after* the signal instruction, so execution
continues with the resume value on top of the stack — as if the signal
expression evaluated to the resume value.

When `fiber/resume` returns to the parent, the signal bits (an integer)
are pushed onto the parent's operand stack. The parent reads the child's
value separately via `(fiber/value child)`. On normal return (bits == 0),
the child's result is also stored in `child.signal` as `(0, return_value)`,
so `fiber/value` works uniformly regardless of how the child completed.


### Finally semantics

`finally` runs unconditionally after the body and any catch clause.
If the `finally` clause itself signals, the `finally` signal takes
precedence (the original signal is lost). This matches Java, Python,
and Janet's `defer` behavior.

If the body completes normally:
1. Run `finally` clause
2. Return body result

If the body signals and a catch clause handles it:
1. Run catch clause
2. Run `finally` clause
3. Return catch result

If the body signals and no catch clause matches:
1. Run `finally` clause
2. Re-propagate the original signal (unless `finally` itself signaled)


### Cancel and propagate

**`fiber/cancel`**: Inject an error signal into a suspended fiber.
The fiber transitions to `Error` status. Used for timeouts and task
cancellation.

**`fiber/propagate`**: Re-raise a caught signal, preserving the
child fiber chain for stack traces. Used in `finally` to re-raise
after cleanup.

Both are future work but the fiber model supports them naturally.


## Phase 2: Signal-Based Effect Type

### Goal

Replace `Effect { yield_behavior, may_raise }` with `SignalBits`.

### The new Effect type

```rust
/// Effect is a simple Copy pair — no allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effect {
    /// Base effect bits (what signals this function itself might emit)
    pub bits: SignalBits,
    /// Bitmask of parameter indices whose effects this function propagates.
    /// Bit i set means this function may exhibit parameter i's effects.
    pub propagates: u32,
}

impl Effect {
    pub const fn pure() -> Self {
        Effect { bits: 0, propagates: 0 }
    }

    pub const fn raises() -> Self {
        Effect { bits: SIG_ERROR, propagates: 0 }
    }

    pub const fn yields() -> Self {
        Effect { bits: SIG_YIELD, propagates: 0 }
    }

    pub const fn yields_raises() -> Self {
        Effect { bits: SIG_YIELD | SIG_ERROR, propagates: 0 }
    }

    pub fn is_pure(&self) -> bool {
        self.bits == 0 && self.propagates == 0
    }
    // ... rest unchanged
}

// Effect bounds (constraining what effects parameters may have) are
// deferred to a future phase. When needed, they'll be tracked in the
// analysis environment, not on the Effect struct itself — keeping
// Effect as a simple Copy pair.
```

### Migration mapping

| Old | New |
|-----|-----|
| `Effect::pure()` | `Effect { bits: 0, propagates: 0 }` |
| `Effect::pure_raises()` | `Effect { bits: SIG_ERROR, propagates: 0 }` |
| `Effect::yields()` | `Effect { bits: SIG_YIELD, propagates: 0 }` |
| `Effect::yields_raises()` | `Effect { bits: SIG_YIELD \| SIG_ERROR, propagates: 0 }` |
| `YieldBehavior::Polymorphic({0})` | `Effect { bits: 0, propagates: 1 }` |
| `effect.is_pure()` | `effect.bits == 0 && effect.propagates == 0` |
| `effect.may_raise` | `effect.bits & SIG_ERROR != 0` |
| `effect.yield_behavior == Yields` | `effect.bits & SIG_YIELD != 0` |


## Phase 3: New Bytecode Instructions

### Goal

Replace exception-handling instructions with signal instructions. Add
fiber creation/resume instructions.

### New instructions

| Instruction | Operands | Semantics |
|-------------|----------|-----------|
| `Signal` | bits: SignalBits | Pop value, emit signal (bits, value) |
| `MakeFiber` | mask: SignalBits | Pop closure, create Fiber with signal mask |
| `Resume` | — | Pop value, pop fiber, resume fiber with value |
| `FiberStatus` | — | Pop fiber, push status as keyword |

### Removed instructions

| Instruction | Replaced by |
|-------------|------------|
| `Yield` | `Signal` with SIG_YIELD |
| `PushHandler` | `MakeFiber` + `Resume` (or kept as sugar) |
| `PopHandler` | Fiber boundary (implicit) |
| `CheckException` | Signal check after `Resume` |
| `MatchException` | Pattern match on signal type |
| `BindException` | Normal variable binding |
| `LoadException` | Signal value is a normal value |
| `ClearException` | Implicit on fiber boundary exit |
| `ReraiseException` | Re-emit the signal |
| `CreateHandler` | Removed (unused) |
| `InvokeRestart` | Removed (unused) |

### Transition strategy

We don't remove old instructions immediately. Instead:

1. Add new instructions (`Signal`, `MakeFiber`, `Resume`, `FiberStatus`)
2. Change the emitter to use new instructions for new code
3. Keep old instruction handlers as compatibility shims that translate
   to signal operations internally
4. Remove old instructions once all code uses new ones


## Phase 4: JIT Integration

### Goal

JIT can compile any function, not just pure ones. Signal checks replace
the purity gate.

### Calling convention change

Currently, JIT functions return a single `u64` (the Value bits).
In the fiber model, they return a `(u32, u64)` — signal bits and value:

```rust
/// JIT function signature
type JitFn = unsafe extern "C" fn(
    env: *const u64,      // captures
    args: *const u64,     // arguments
    argc: u32,            // argument count
    vm: *mut (),          // VM pointer (for calling back)
    self_bits: u64,       // self-reference for tail calls
) -> (u32, u64);          // (signal_bits, value_bits)
```

When the JIT calls another function:
```
result = call(callee, args...)
if result.0 != 0 {
    // Signal — propagate it
    return result
}
// Normal return — use result.1 as the value
```

For provably pure callees (effect bits == 0), the signal check is elided.

### Hot function threshold

Currently: JIT at 10 calls, only pure functions.
New: JIT at 10 calls, any function. Pure functions get faster code
(no signal checks on known-pure callees).


## Phase 5: Surface Syntax

### Goal

Expose fibers and signals in the surface language.

### New primitives

```lisp
;; === Creation and control ===

;; Create a fiber from a closure with a signal mask
(fiber/new fn mask) → fiber

;; Resume a fiber, delivering a value
(fiber/resume fiber value) → signal-bits

;; Emit a signal from the current fiber (suspends it)
(fiber/signal bits value) → (suspends)

;; === Introspection ===

;; Lifecycle status
(fiber/status fiber) → keyword  ; :new :alive :suspended :dead :error

;; Signal payload from last signal or return value
(fiber/value fiber) → value

;; Signal bits from last signal
(fiber/bits fiber) → int

;; Capability mask (set at creation, immutable)
(fiber/mask fiber) → int

;; === Chain traversal ===

;; Parent fiber (nil at root)
(fiber/parent fiber) → fiber | nil

;; Most recently resumed child fiber (nil if none)
(fiber/child fiber) → fiber | nil

;; === Internals (for debugging/tooling) ===

;; The closure this fiber wraps
(fiber/closure fiber) → closure

;; The operand stack (for debugging)
(fiber/stack fiber) → vector

;; Dynamic bindings (fiber-scoped state)
(fiber/env fiber) → table | nil
```

### Sugar (macros over primitives)

```lisp
;; try/catch/finally
(try body
  (catch e handler)
  (finally cleanup))

;; yield
(yield value) → (fiber/signal :yield value)

;; throw
(throw value) → (fiber/signal :error value)

;; Backward compat (thin aliases)
(make-coroutine fn) → (fiber/new fn :yield)
(coroutine-resume co val) → (fiber/resume co val)
(coroutine-status co) → (fiber/status co)
```


## Implementation Order

### Step 1: Fiber struct and HeapObject variant (Phase 1a)

**Files to create/modify:**
- Create `src/value/fiber.rs` — Fiber, Frame, FiberStatus, SignalBits
- Modify `src/value/heap.rs` — add `HeapObject::Fiber(Rc<RefCell<Fiber>>)`
- Modify `src/value/repr/` — add fiber tag, constructors, accessors
- Modify `src/value/mod.rs` — re-exports

**Tests:** Unit tests for Fiber creation, status transitions, stack/frame
operations. No behavioral changes yet.

**Estimated effort:** 2-3 hours

### Step 2: Move execution state to Fiber (Phase 1b)

**The big change.** The dispatch loop (`src/vm/dispatch.rs`) reads and
writes fiber state instead of VM state.

**Files to modify:**
- `src/vm/core.rs` — slim down VM, add `fiber` field
- `src/vm/dispatch.rs` — borrow fiber for stack/frame access
- `src/vm/call.rs` — frame push/pop on fiber, not VM
- `src/vm/stack.rs` — operate on fiber's stack
- `src/vm/variables.rs` — upvalue access through fiber's frame
- `src/vm/control.rs` — return pops fiber's frame
- `src/vm/closure.rs` — closure creation reads fiber's stack
- `src/vm/arithmetic.rs`, `comparison.rs`, `types.rs`, `data.rs`,
  `literals.rs`, `scope.rs` — all stack operations go through fiber
- `src/primitives/coroutines.rs` — rewrite to use Fiber instead of Coroutine
- `src/pipeline.rs` — create root fiber for top-level execution

**Strategy:** This is the riskiest step. The approach should be:
1. Make the dispatch loop generic over "where the stack lives"
2. Initially, the VM creates a root Fiber and delegates to it
3. All existing tests must pass — behavior is identical
4. The Coroutine type remains temporarily as a thin wrapper around Fiber

**Tests:** All 1,768 existing tests must pass. No behavioral changes.

**Estimated effort:** 4-6 hours

### Step 3: Signal-based returns (Phase 1c)

Change the internal return type from `Result<VmResult, String>` to
signal-based returns.

**Files to modify:**
- `src/vm/dispatch.rs` — `run()` returns `(SignalBits, Value)`
- `src/vm/call.rs` — call returns propagate signals
- `src/vm/core.rs` — `VmResult` replaced by signal returns
- Exception handling in dispatch loop uses signal mechanism

**Tests:** All existing tests pass. Exception behavior identical.

**Estimated effort:** 3-4 hours

### Step 4: Effect type migration (Phase 2)

Replace the Effect struct with signal-bits-based Effect.

**Files to modify:**
- `src/effects/mod.rs` — new Effect type
- `src/effects/primitives.rs` — primitive effects as SignalBits
- `src/hir/analyze/` — effect inference produces SignalBits
- `src/lir/` — effect tracking uses SignalBits
- `src/value/closure.rs` — `effect: Effect` field uses new type
- `src/primitives/registration.rs` — effect declarations use new type
- All test files that construct Effect values

**Tests:** All existing tests pass. Effect inference behavior identical.

**Estimated effort:** 3-4 hours

### Step 5: New bytecode instructions (Phase 3)

Add Signal, MakeFiber, Resume, FiberStatus instructions.

**Files to modify:**
- `src/compiler/bytecode.rs` — new instruction variants
- `src/compiler/bytecode_debug.rs` — debug formatting
- `src/vm/dispatch.rs` — handlers for new instructions
- `src/lir/` — LIR support for new instructions
- `src/hir/analyze/` — analysis for fiber/signal forms

**Tests:** New tests for each instruction. Integration tests for
fiber-based control flow.

**Estimated effort:** 3-4 hours

### Step 6: Surface syntax and backward compatibility (Phase 5)

Add fiber/resume/signal primitives. Make existing forms compile to
fiber operations.

**Files to modify:**
- Create `src/primitives/fibers.rs` — fiber, resume, signal primitives
- `src/primitives/registration.rs` — register new primitives
- `src/syntax/` or `src/hir/` — macro expansion for try/catch/yield sugar
- `src/primitives/coroutines.rs` — thin wrappers around fiber operations
- `examples/` — new examples, update existing ones

**Tests:** New integration tests. All existing examples pass.

**Estimated effort:** 3-4 hours

### Step 7: JIT integration (Phase 4)

Update JIT calling convention and remove purity restriction.

**Files to modify:**
- `src/jit/compiler.rs` — new calling convention
- `src/jit/bridge.rs` — signal-aware trampolines
- `src/vm/call.rs` — JIT dispatch uses signal returns
- `src/jit/mod.rs` — remove purity gate

**Tests:** JIT tests with non-pure functions. Benchmark regressions.

**Estimated effort:** 4-6 hours

### Step 8: Cleanup (all phases)

Remove deprecated types and code paths.

**Files to remove/gut:**
- `src/value/coroutine.rs` — replaced by fiber.rs
- `src/value/continuation.rs` — continuation capture is now fiber suspension
- Old exception handling instructions from bytecode.rs
- Old exception handler types

**Tests:** All tests pass. No deprecated code remains.

**Estimated effort:** 2-3 hours


## Invariants

These must hold throughout the implementation:

1. **All tests pass at every step.** No step breaks existing behavior.
   The test suite is the contract.

2. **The root fiber exists.** Top-level code runs in a root fiber with
   `mask: !0` (catches everything). There is always a current fiber.

3. **Signals propagate until caught.** An uncaught signal propagates to
   the root fiber. An uncaught error at the root fiber is a runtime error.

4. **Capabilities narrow, never widen.** A child fiber cannot have more
   capability bits than its parent granted.

5. **The fast path is zero-overhead.** When a function doesn't signal
   and the caller doesn't need to handle signals, the overhead vs. the
   current system should be zero or negligible.

6. **Fiber is a Value.** Fibers are first-class — they can be stored in
   variables, passed as arguments, returned from functions.


## Risks and Mitigations

**Risk: Step 2 (state migration) breaks everything.**
Mitigation: Do it incrementally. First add Fiber as a parallel structure
alongside the existing VM state. Run tests. Then switch the dispatch loop
to read from Fiber. Run tests. Then remove the old fields from VM.

**Risk: Performance regression from Rc<RefCell<Fiber>> borrowing overhead.**
Mitigation: In the hot path, borrow the fiber once at the top of the
dispatch loop and hold it for the duration. Use `unsafe` cell access if
profiling shows RefCell overhead matters (document why).

**Risk: Exception handler semantics change subtly.**
Mitigation: The exception integration tests are comprehensive. Run them
after every change. Add property tests for "exception in try/catch
produces same result before and after."

**Risk: JIT calling convention change requires recompilation of all cached code.**
Mitigation: Clear the JIT cache when upgrading. Add a version tag to
JitCode to detect stale entries.


## Open Questions (to resolve during implementation)

1. **Resolved.** `pending_tail_call` stays on VM. Tail calls are transient — they complete within one dispatch iteration and never cross a suspension boundary.

2. **Dynamic bindings representation.** `Option<HashMap<u32, Value>>` on
    Fiber is simple. Should it be a persistent map (for cheap forking)?
    Start simple, optimize later.

3. **Resolved.** Removed. Redundant with lexical scoping via Frame.closure.env.

4. **Signal mask syntax.** Keywords (`:error`, `:yield`) vs. integer
    constants vs. a bitfield constructor macro. Design during Step 5.
