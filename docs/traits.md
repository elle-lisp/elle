# Traits

<!-- audited: 2026-09-23 -->

Every heap-allocated value carries a `traits` field — a pointer to a
trait table (struct or @struct). Collection and sequence types get a
shared default traitset stamped at allocation time. Other heap types
start with `nil` traits.

## Reading traits

```lisp
(assert (= (sort (keys (traits [1 2 3]))) (list :Collection :Sequence)))
(assert (= (keys (traits {:a 1})) (list :Collection)))
(assert (nil? (traits 42)))           # an immediate has no traits
(assert (nil? (traits (fn [] 1))))    # a closure starts with nil traits
```

Every array, list, string and bytes value shares one traitset object, and
every set and struct shares another. `identical?` compares contents, so it
cannot show the sharing; a write to the shared table can:

```lisp
(def shared (traits [1 2]))
(put shared :Probe 1)
(assert (= (get (traits [3 4]) :Probe) 1))
(assert (= (get (traits "text") :Probe) 1))
(del shared :Probe)
(assert (nil? (get (traits [5]) :Probe)))
```

## Attaching per-instance traits

`with-traits` creates a new value with a custom trait table:

```lisp
(def point (with-traits [1 2 3] {:type :point}))
(assert (= (traits point) {:type :point}))
(assert (= (length point) 3))
(assert (= (get point 1) 2))
```

The trait table can be an immutable struct or a mutable @struct.
Data operations (get, length, first, etc.) see through traits — they
operate on the underlying data, not the trait table.

## Traits are invisible to equality

Traits do not affect structural equality, ordering, or hashing:

```lisp
(assert (= [1 2 3] (with-traits [1 2 3] {:type :point})))
```

## Protocol dispatch

Primitives like `first`, `rest`, `length`, `empty?`, `has?`, and
`second` dispatch through trait table lookup instead of hardcoded type
cascades. (`second` dispatches via the `:Sequence` `:nth` method.) Each
collection/sequence type gets a shared default traitset stamped at
allocation time.

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

A method struct also holds whatever operator methods the collection
overrides — `:map`, `:fold`, `:reverse` and the rest. § Collection
operators below has the names and the argument order.

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

The fallback is per protocol, not per method. A table that names a
protocol owns every method in it, so `(with-traits (list 1 2)
@{:Sequence {:first …}})` has no `rest` at all — the default `:Sequence`
is out of reach the moment the user table declares one. Attach the
methods to the protocol the value does not otherwise need, or write the
whole protocol out.

### Edge cases

- **Empty list** `()` is an immediate — no traitset. `first` returns
  an error, `rest` returns `()`, `length` returns 0, `empty?` returns
  true. These are handled as pre-checks in the primitives.
- **Syntax objects** support `first`, `rest`, `length`, `empty?` via
  pre-checks (used during macro expansion). No traitset.
- **Symbols and keywords** support `length` via pre-check.
- **nil** supports `length` (returns 0) via pre-check. It does *not*
  support `empty?` — `(empty? nil)` raises a `:type-error`.

## Collection operators

`map`, `filter`, `fold`, `each` and the rest of the operator family are
Elle functions and macros, not primitives, so they dispatch in three
layers. The first layer that answers wins.

1. **A builtin container family answers first.** A list, array, string,
   bytes or syntax object — mutable or not — keeps its own traversal.
   An operator never reads a trait table for one, so
   `(with-traits [1 2 3] …)` still maps as an array, and a plain array
   costs what it always did.
2. **A per-operator method overrides the operator.** When the value's
   trait table carries a method named for the operator, the operator
   calls it and returns its answer. This is how a lazy or a remote
   collection controls what `map` *means*, and not merely how its
   elements stream out.
3. **`:iter` drives everything else.** When the table carries no method
   for this operator but does carry `:iter`, the operator drains the
   iterator and works on the elements it yields.

A value that answers none of the three raises `:type-error`, as before.
A plain set or struct answers none: no default traitset carries `:iter`,
so both convert and iterate exactly as they did.

### Where an operator method lives

An operator method goes in the `:Sequence` protocol or the
`:Collection` protocol. The lookup reads `:Sequence` first and
`:Collection` second, so either one carries it.

The method's name is the operator's own name as a keyword. It takes the
collection first, then the operator's remaining arguments in their
declared order. `(map f coll)` calls `(method coll f)`, `(take n coll)`
calls `(method coll n)`, and `(fold f init coll)` calls
`(method coll f init)`.

```lisp
(def tagged
  (with-traits {:from 0 :to 10}
    @{:Sequence {:map (fn [self f] :mapped)}}))

(assert (= (map (fn [x] x) tagged) :mapped))
```

These operators take a method: `map`, `filter`, `any?`, `all?`, `find`,
`find-index`, `count`, `flatten`, `take-while`, `drop-while`, `distinct`,
`mapcat`, `map-indexed`, `partition`, `interpose`, `sort-by`,
`sort-with`, `take`, `drop`, `update`, `last`, `butlast`, `reverse`, and
`fold`. `reduce` is `fold`, and `keep` is `filter`, so each pair shares
one method name.

### The generic fallback

An operator with no method of its own drains `:iter` into an array and
runs over that. An operator that answers with a collection of the same
kind then rebuilds one: it seeds with the `:Collection` `:empty` method
and adds each element with `:conj`, in iteration order. So `:iter`,
`:empty` and `:conj` together carry the whole operator family.

```lisp
(defn make-bag [items]
  (with-traits {:items items}
    @{:Sequence {:iter (fn [self]
                         (fiber/new (fn []
                                      (each x in (self :items)
                                        (yield x))) |:yield|))}
      :Collection {:empty (fn [self] (make-bag []))
                   :conj (fn [self x]
                           (make-bag (append (self :items) [x])))}}))

(assert (= ((map (fn [x] (* x 2)) (make-bag [1 2 3])) :items) [2 4 6]))
```

`:conj` decides the order of the answer. A `:conj` that appends keeps
iteration order; one that prepends reverses it, the way conj-ing onto a
list does.

The fallback reads the whole collection before it answers, so `find` and
`take` visit every element rather than stopping early. An endless
collection, or one whose elements cost a round trip each, therefore
wants its own per-operator methods. An iterator that raises propagates
the error out of the operator.

### Operators that do not take a method

`each` drives `:iter` alone, so that `break` and `assign` still reach the
surrounding function; there is no `:each` method. `zip` reads `:iter` on
each input it is given. `frequencies` and `group-by` are written with
`each`, so they dispatch the way `each` does.

`each` itself keeps the set and struct arms it always had, and reads
`:iter` ahead of them. So a with-traits struct that carries `:iter`
iterates as its protocol says, and a plain one still walks its
key-value pairs.

### Reading the layers from Elle

The dispatch is five functions, and a collection can call them:

| Function | Answers |
|----------|---------|
| `(trait/method coll name)` | the method `name` from `coll`'s own table, or nil |
| `(trait/op coll name)` | the same, but nil for a builtin container family |
| `(trait/iterable? coll)` | true when `coll`'s elements come from `:iter` |
| `(trait/elements coll)` | `coll`'s elements as an immutable array |
| `(trait/rebuild coll items)` | a collection like `coll` holding `items` |


## Iterator protocol

`:iter` returns a **fiber**. Each `(yield item)` produces one element.
When the fiber completes (status `:dead`), iteration is done.

```lisp
(def arr [10 20 30])
(def iter-fn (get (get (traits arr) :Sequence) :iter))
(def walker (iter-fn arr))
(assert (= (fiber/resume walker) 10))
(assert (= (fiber/resume walker) 20))
(assert (= (fiber/resume walker) 30))
(assert (= (fiber/status walker) :paused))   # one more resume drains it
(fiber/resume walker)
(assert (= (fiber/status walker) :dead))
```

## Sharing and mutability

Default traitsets are **shared by reference**. All arrays point to the
same @struct. Mutating the shared @struct is visible to all instances,
as § Reading traits shows.

Per-instance override via `with-traits`:

```lisp
(def custom (with-traits [1 2 3]
              @{:Sequence {:first (fn [self] :custom)}}))
(assert (= (first custom) :custom))
(assert (= (first [1 2 3]) 1))   # the default, unaffected
```

### `with-traits` returns an independent value

For a mutable collection — `@array`, `@struct`, `@string`, `@bytes`, `@set`,
a box — the store is **copied**, not shared. A later write to the original is
not visible through the traited value:

```lisp
(def original @[1 2])
(def traited (with-traits original {:tag :x}))
(push original 99)
(assert (= original @[1 2 99]))
(assert (= traited @[1 2]))
```

The exceptions are fibers, thread handles, and plugin externals. Those wrap a
handle rather than owning data, so the traited value names the *same* entity
and stays `identical?` to it — a traited fiber is that fiber, not a copy of it.

## Custom sequence types

Any value can implement `:Sequence` via `with-traits`:

```lisp
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

(assert (= (first (make-range 0 10)) 0))
(assert (= (first (rest (make-range 0 10))) 1))
(assert (= (fold + 0 (make-range 0 5)) 10))   # fold drains :iter
```

## Cross-thread behavior

Default traitsets belong to one runtime instance: each heap builds its own
through `init_default_traits`, so each worker thread's VM has its own. When
sending a value to another thread:

- Default traits are skipped (sent as NIL). The receiving thread's
  constructors stamp its own registry defaults.
- User-attached traits are deep-copied faithfully.

Detection uses pointer identity against `default_traits_for(tag)`,
not a type heuristic. User-attached @struct traits are preserved.

## Allocation

Default traitsets are built once per heap into its pinned **root region**
(`alloc_root`), and the per-tag table is stored on the heap itself. They are
not reclaimed by scope-based arena operations, but they are *not* immortal:
the teardown sweep releases the root region, and `reset_default_traits`
clears the heap's table so a read after teardown returns `nil` rather than a
freed pointer. The next instance builds its own. The method handles a
traitset carries *are* native-fns: immediate `prim_id` values that occupy no
region. The traitset pointer in a heap object is just a pointer — no arena
bookkeeping overhead.

The traitset @structs themselves carry `nil` traits, so a protocol primitive
such as `has?` cannot take one: read a traitset with `get` and `keys`.

The `traits` side-field is a cross-region edge enumerated for every
traitable heap variant during region cross-ref accounting
(`find_object_cross_refs`), so the alloc-time incref and the free-time
decref balance symmetrically. User-attached traits (arena-allocated via
`with-traits`) are traced this way.

## Performance

The hot path for builtin types:
- Read `traits` field (pointer in the heap object)
- Linear scan for protocol keyword (2 entries in the @struct)
- Linear scan for method keyword (5 entries in the method struct)
- Native function call (same Rust code as the old type cascade)

`lookup_keyword` compares the keyword's name hash against each entry's key
directly, so it builds no key at all.

---

## See also

- [types.md](types.md) — type system and heap tags
- [structs.md](structs.md) — struct operations
