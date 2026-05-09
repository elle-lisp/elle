# Traits

Every heap-allocated value carries a `traits` field — a pointer to a
trait table (struct or @struct). Collection and sequence types get a
shared default traitset stamped at allocation time. Other heap types
start with `nil` traits.

## Reading traits

```
(traits [1 2 3])           # => @{:Sequence {...} :Collection {...}}
(traits {:a 1})            # => @{:Collection {...}}
(traits 42)                # => nil (immediate, no traits)
```

All arrays share the same traitset pointer. All lists share the same
traitset pointer. This is identity-equal:

```
(identical? (traits [1 2]) (traits [3 4]))  # => true
```

## Attaching per-instance traits

`with-traits` creates a new value with a custom trait table:

```
(def v (with-traits [1 2 3] {:type :point}))
(traits v)                 # => {:type :point}
```

The trait table can be an immutable struct or a mutable @struct.
Data operations (get, length, first, etc.) see through traits — they
operate on the underlying data, not the trait table.

## Traits are invisible to equality

Traits do not affect structural equality, ordering, or hashing:

```
(= [1 2 3] (with-traits [1 2 3] {:type :point}))  # => true
```

## Protocol dispatch

Primitives like `first`, `rest`, `length`, `empty?`, `has?`, `last`,
`second` dispatch through trait table lookup instead of hardcoded type
cascades. Each collection/sequence type gets a shared default traitset
stamped at allocation time.

### Trait table schema

A trait table is a **mutable @struct shell** mapping protocol keywords
to **immutable method structs**:

```
@{:Sequence   {:first (fn [self] ...)
               :rest  (fn [self] ...)
               :last  (fn [self] ...)
               :nth   (fn [self n] ...)
               :iter  (fn [self] ...)}
  :Collection {:length (fn [self] ...)
               :empty? (fn [self] ...)
               :has?   (fn [self needle] ...)
               :conj   (fn [self item] ...)
               :empty  (fn [self] ...)}}
```

The mutable shell lets users swap entire protocols on a shared traitset.
Immutable method structs avoid RefCell borrow on per-method lookup.

### Which types get which protocols

| Type              | :Sequence | :Collection |
|-------------------|-----------|-------------|
| list (pair / ())  | yes       | yes         |
| array / @array    | yes       | yes         |
| string / @string  | yes       | yes         |
| bytes / @bytes    | yes       | yes         |
| set / @set        | no        | yes         |
| struct / @struct  | no        | yes         |

Immediates (int, float, bool, nil, keyword, symbol) have no traitset.

### Dispatch algorithm

When a primitive like `first` is called on value `v`:

1. Read `v`'s `traits` field (always populated for collection types)
2. Look up `:Sequence` in the @struct (linear scan, 2 keys)
3. Look up `:first` in the method struct (linear scan, 5 keys)
4. Call the method (NativeFn direct call or closure via VM context)

If the value has user-attached traits that lack the requested protocol,
the dispatcher falls back to the default traitset from the registry.
This means `(with-traits [1 2 3] {:tag :my-type})` still supports
`first`, `length`, etc. — the user traits don't mask the defaults.

### Edge cases

- **Empty list** `()` is an immediate — no traitset. `first` returns
  an error, `rest` returns `()`, `length` returns 0, `empty?` returns
  true. These are handled as pre-checks in the primitives.
- **Syntax objects** support `first`, `rest`, `length`, `empty?` via
  pre-checks (used during macro expansion). No traitset.
- **Symbols and keywords** support `length` via pre-check.
- **nil** supports `length` (returns 0) and `empty?` (returns true)
  via pre-check.

### Non-overridable operations

`butlast`, `reverse` are defined in terms of the underlying
implementations, not through trait dispatch. User-defined Sequence
types that only implement the trait protocol won't support these.

## Iterator protocol

`:iter` returns a **fiber**. Each `(yield item)` produces one element.
When the fiber completes (status `:dead`), iteration is done.

```
(def arr [10 20 30])
(def iter-fn (((traits arr) :Sequence) :iter))
(def fib (iter-fn arr))
(fiber/resume fib)   # => 10
(fiber/resume fib)   # => 20
(fiber/resume fib)   # => 30
(fiber/status fib)   # => :dead
```

## Sharing and mutability

Default traitsets are **shared by reference**. All arrays point to the
same @struct. Mutating the shared @struct is visible to all instances.

Per-instance override via `with-traits`:

```
(def v (with-traits [1 2 3]
         @{:Sequence {:first (fn [self] :custom)}}))
(first v)        # => :custom
(first [1 2 3])  # => 1 (default, unaffected)
```

## Custom sequence types

Any value can implement `:Sequence` via `with-traits`:

```
(defn make-range [start end]
  (with-traits {:start start :end end}
    @{:Sequence
      {:first (fn [self] (self :start))
       :rest  (fn [self]
                (if (>= (+ (self :start) 1) (self :end))
                  ()
                  (make-range (+ (self :start) 1) (self :end))))
       :iter  (fn [self]
                (fiber/new (fn []
                  (def @i (self :start))
                  (while (< i (self :end))
                    (yield i)
                    (assign i (+ i 1)))) |:yield|))}}))

(first (make-range 0 10))        # => 0
(first (rest (make-range 0 10))) # => 1
```

## Cross-thread behavior

Default traitsets are thread-local permanent allocations. When sending
a value to another thread:

- Default traits are skipped (sent as NIL). The receiving thread's
  constructors stamp its own registry defaults.
- User-attached traits are deep-copied faithfully.

Detection uses pointer identity against `default_traits_for(tag)`,
not a type heuristic. User-attached @struct traits are preserved.

## Allocation

Default traitsets use `alloc_permanent` (Rc-backed, never reclaimed
by arena scope operations). This ensures they don't interfere with
scope-based arena reclamation. The traitset pointer in a heap object
is just a pointer — no arena bookkeeping overhead.

`collect_heap_children` traces the `traits` field for all heap
variants. Permanent traitsets won't match `slab_owns()` and are
skipped. User-attached traits (arena-allocated via `with-traits`) are
properly traced.

## Performance

The hot path for builtin types:
- Read `traits` field (pointer in the heap object)
- Linear scan for protocol keyword (2 entries in the @struct)
- Linear scan for method keyword (5 entries in the method struct)
- Native function call (same Rust code as the old type cascade)

`lookup_keyword` avoids allocating a `TableKey` — it compares the
keyword discriminant and string directly.

---

## See also

- [types.md](types.md) — type system and heap tags
- [structs.md](structs.md) — struct operations
