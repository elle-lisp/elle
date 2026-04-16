# Scope-Based Allocation Demo

## What This Demo Does

This demo measures the effectiveness of scope-based allocation — a compiler optimization that frees heap objects early when they go out of scope, rather than waiting until the fiber exits.

The demo runs allocation-heavy workloads in child fibers and measures:
- **Live objects** — How many heap objects remain after the workload
- **Scope enters** — How many scope regions were created
- **Destructors run** — How many objects were freed early

It demonstrates eight tiers of escape analysis, each recognizing a wider class of "safe" scopes that can release objects early.

## How It Works

### Escape Analysis Tiers

**Tier 1: Primitive Whitelist**

Some primitives are known to return immediates (non-heap values). If a `let` binding allocates temporary data and returns only the result of a whitelisted primitive, the binding qualifies for scope allocation:

```janet
(let ((data @[1 2 3 4 5]))
  (length data))  # length returns an immediate (integer)
```

The `data` array can be freed when the `let` exits, not when the fiber exits.

**Tier 3: Returning Outer Binding**

When a `let` body returns a variable from *outside* the let, the result is safe (allocated before the scope's RegionEnter):

```janet
(let ((outer-val 0))
  (while (< i 10000)
    (let ((temp @[1 2 3]))
      (set outer-val (+ outer-val (length temp)))
      outer-val)  # Returns outer-val, not temp
    (set i (+ i 1))))
```

**Tier 4: Nested Lets Reducing to Arithmetic**

Both the outer and inner `let` qualify. The inner let's bindings are part of the outer scope's region:

```janet
(let ((xs @[10 20 30]))
  (let ((n (length xs)))
    (+ n 1)))  # Returns arithmetic result (immediate)
```

**Tier 5: Match Returning Immediates**

All match arms return keywords or integers → result is safe:

```janet
(let ((tag (mod i 3)))
  (match tag
    (0 :zero)
    (1 :one)
    (_ :other)))  # All arms return keywords (immediates)
```

**Tier 8: Immediate Outward Set**

An outward `set` with a provably immediate value is harmless:

```janet
(var counter 0)
(while (< counter 10000)
  (let ((tmp @[1 2 3]))
    (length tmp))
  (set counter (+ counter 1)))  # Immediate value
```

The `while` block can scope-allocate because the only outward effect is setting `counter` to an immediate.

### Measuring Allocation

The demo uses three primitives:

**`arena/count`** — Returns the number of live heap objects
```janet
(var before (arena/count))
(var i 0)
(while (< i 10000)
  (let ((data @[1 2 3 4 5]))
    (length data))
  (set i (+ i 1)))
(- (arena/count) before)  # Objects freed early
```

**`arena/scope-stats`** — Returns a struct with:
- `:enters` — Number of scope regions created
- `:dtors-run` — Number of destructors run (objects freed early)

```janet
(let ((stats (arena/scope-stats)))
  (print "Enters: ")
  (print (get stats :enters))
  (print "  Dtors: ")
  (println (get stats :dtors-run)))
```

### The Workload

Each tier runs a tight loop that allocates temporary objects:

```janet
(var tier1-scoped
  (run (fn []
    (var before (arena/count))
    (var i 0)
    (while (< i 10000)
      (let ((data @[1 2 3 4 5]))
        (length data))
      (set i (+ i 1)))
    (- (arena/count) before))))
```

The `run` helper executes the workload in a non-yielding child fiber:

```janet
(defn run [thunk]
  "Execute thunk in a non-yielding child fiber."
  (fiber/resume (fiber/new thunk 1)))
```

## Sample Output

```
tier 1 — primitive whitelist (length)
  scoped:   1 live objects after 10k iters
  unscoped: 10001 live objects after 10k iters
  saved:    10000 objects freed early

tier 3 — returning outer binding
  enters:    10001
  dtors-run: 10000

tier 4 — nested lets reducing to arithmetic
  scoped net: 1 live objects after 10k iters

tier 5 — match arms returning keywords
  scoped net: 1 live objects after 10k iters

tier 8 — immediate outward set in while
  enters:    10001  (10000 inner let + 1 while block = 10001)
  dtors-run: 10000

combined — all tiers in one fiber
  net objects: 1
  total sum:   25000
  scope stats: {:dtors-run 15000 :enters 20001}

done.
```

### Interpreting the Results

**Tier 1 (scoped vs unscoped):**
- Scoped: 1 live object (the fiber itself)
- Unscoped: 10,001 live objects (10,000 arrays + fiber)
- Saved: 10,000 objects freed early

This shows that scope allocation freed 10,000 temporary arrays immediately, rather than keeping them alive until the fiber exited.

**Tier 3 (scope stats):**
- Enters: 10,001 (10,000 inner lets + 1 outer while block)
- Dtors-run: 10,000 (each inner let's temporary freed)

**Combined workload:**
- Net objects: 1 (just the fiber)
- Total sum: 25,000 (accumulated result from all tiers)
- Scope stats: 20,001 enters, 15,000 destructors

## Elle Idioms Used

- **`defn`** — Function definition
- **`var` / `set`** — Mutable variables
- **`let`** — Lexical binding (scope-allocated when safe)
- **`while`** — Loop with mutable state
- **`match`** — Pattern matching
- **`fiber/new` / `fiber/resume`** — Create and run fibers
- **`arena/count`** — Query heap object count
- **`arena/scope-stats`** — Query scope allocation statistics

## Why This Demo?

Scope-based allocation is important for:
1. **Memory efficiency** — Freeing objects early reduces peak memory usage
2. **Cache locality** — Freed objects can be reused immediately
3. **GC pressure** — Fewer objects for the garbage collector to track
4. **Predictability** — Deterministic cleanup at scope exit

This demo shows that Elle's compiler can automatically optimize memory management without explicit annotations.

## Running the Demo

```bash
cargo run --release -- demos/scope-alloc/scope-alloc.lisp
```

To see detailed escape analysis statistics, pass `--stats`:
```bash
cargo run --release -- --stats demos/scope-alloc/scope-alloc.lisp
```

This prints the compiler's escape analysis decisions for each scope
(alongside JIT compilation stats) on stderr when the program exits.

## Further Reading

- [Escape Analysis](https://en.wikipedia.org/wiki/Escape_analysis)
- [Region-Based Memory Management](https://en.wikipedia.org/wiki/Region-based_memory_management)
- [Scope-Based Allocation in Elle](../../docs/scope-alloc.md) (if available)
