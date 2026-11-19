# Types

<!-- audited: 2026-11-18 -->

Elle values are 16-byte tagged unions. Every value carries a type keyword
returned by `type-of` (alias: `type`).

## Type keywords

```lisp
(assert (= (type-of 42) :integer))
(assert (= (type-of 3.14) :float))
(assert (= (type-of "hello") :string))
(assert (= (type-of :foo) :keyword))
(assert (= (type-of 'foo) :symbol))
(assert (= (type-of true) :boolean))
(assert (= (type-of nil) :nil))
(assert (= (type-of ()) :list))
(assert (= (type-of [1 2]) :array))
(assert (= (type-of @[1 2]) :@array))
(assert (= (type-of {:a 1}) :struct))
(assert (= (type-of @{:a 1}) :@struct))
(assert (= (type-of @"hi") :@string))
(assert (= (type-of |1 2|) :set))
(assert (= (type-of (fn [] 1)) :closure))
(assert (= (type-of length) :native-fn))   # a Rust primitive
(assert (= (type-of +) :closure))          # + is a stdlib function
(assert (= (type 42) :integer))
```

The full set, less a plugin's objects, which each answer the type name their
plugin gives them:

```text
:nil :boolean :integer :float :symbol :keyword
:list :array :@array :struct :@struct
:string :@string :bytes :@bytes :set :@set
:closure :native-fn :box :fiber :parameter :thread-handle
:ptr :syntax
:library-handle :ffi-signature :ffi-type
```

A raw C pointer and a managed pointer both answer `:ptr`.

## Type predicates

```text
nil?       boolean?   number?    integer?   float?
symbol?    keyword?   string?    pair?      list?
empty?     array?     struct?    bytes?     set?
box?       fiber?     parameter? ptr?       pointer?
fn?        closure?   native-fn? native?    primitive?
mutable?   immutable? zero?      nonzero?   nonempty?
pos?       neg?       nan?       inf?
```

Predicates ending in `?` return `true` or `false`. Some span
mutability variants: `array?` matches both `:array` and `:@array`,
and likewise for `string?`, `struct?`, `bytes?`, and `set?`.

`fn?` and `callable?` match any callable (closures and native functions).
Use `closure?` or `native-fn?` to distinguish.

```lisp
(assert (and (array? [1]) (array? @[1])))
(assert (and (fn? length) (callable? length) (native-fn? length) (not (closure? length))))
(assert (and (fn? +) (closure? +) (not (native-fn? +))))
```

### List predicates and nil

`nil` and `()` are distinct. These assertions are authoritative; see
[empty-list.md](empty-list.md) for the rationale.

```lisp
(assert (nil? nil))                  # only nil is nil
(assert (not (nil? ())))             # the empty list is NOT nil
(assert (empty? ()))                 # the empty list is empty
(assert (not (first (protect (empty? nil)))))   # nil is not a container
(assert (list? ()))                  # the empty list is a list
(assert (not (list? nil)))           # nil is not a list
(assert (not (pair? ())))            # the empty list has no car/cdr
(assert (not (pair? nil)))
```

## Truthiness

Only `nil` and `false` are falsy. Everything else is truthy —
including `0`, `""`, `()`, `[]`, and `@[]`.

```lisp
(assert (= (if 0   :yes :no) :yes))    # unlike C/Python
(assert (= (if ""  :yes :no) :yes))
(assert (= (if ()  :yes :no) :yes))    # the empty list is truthy
(assert (= (if nil :yes :no) :no))
```

## Conversions

### String ↔ number

```lisp
(assert (= (parse-int "42") 42))
(assert (= (parse-int "ff" 16) 255))      # radix 2-36
(assert (= (parse-int "1010" 2) 10))
(assert (= (parse-float "3.14") 3.14))

(assert (= (number->string 42) "42"))
(assert (= (number->string 255 16) "ff"))
(assert (= (number->string 255 2) "11111111"))
```

### Numeric coercion

```lisp
(assert (= (integer 3.7) 3))              # truncates
(assert (identical? (float 42) 42.0))
```

### To string

`string` converts any value to its string representation:

```lisp
(assert (= (string 42) "42"))
(assert (= (string :hello) "hello"))      # no colon
(assert (= (string 'hello) "hello"))
(assert (= (type-of (string @"hello")) :string))   # @string → string
```

## Equality

`=` is structural equality. It works across mutability boundaries.

```lisp
(assert (= [1 2 3] @[1 2 3]))             # same contents
(assert (= {:a 1} {:a 1}))
(assert (= 1 1.0))                        # numeric coercion
```

`=` is **compositional**: two collections are equal exactly when their
elements are pairwise equal under `=`. For all `a`, `b`:
`(= [a] [b])` ⇔ `(= a b)`. Numeric coercion and IEEE 754 float
semantics therefore apply at every depth, not just at the top level:

```lisp
(assert (= [1] [1.0]))                    # coercion reaches elements
(assert (= {:a [1]} {:a [1.0]}))
(def nan (/ 0.0 0.0))
(assert (not (= nan nan)))                # IEEE 754: NaN ≠ NaN
(assert (not (= [nan] [nan])))            # NaN poisons any value containing it
(assert (= -0.0 0.0))                     # IEEE 754: zeros are equal
```

A consequence of IEEE NaN semantics is that a value containing NaN is
not `=` to anything — including itself. There is no identity shortcut:
`(= v v)` is `false` when `v` holds a NaN anywhere inside.

**Precision caveat:** mixed int/float comparisons coerce through f64.
Integers beyond 2^53 may compare equal when they shouldn't. This too
applies at every depth. Int/int comparison is always exact.

```lisp
(assert (= 9007199254740992 9007199254740993.0))
```

A value of a reference type compares by identity: a closure, a fiber, a
parameter, a thread handle, a syntax object, a pointer, or a plugin's object.
Closures, for example:

```lisp
(def f (fn [x] x))
(def g (fn [x] x))
(assert (= f f))
(assert (not (= f g)))                    # different objects
```

### identical?

`identical?` is the strict relation: no numeric coercion, and floats
compare by bit pattern, so it is reflexive even for NaN. Collections
still compare by contents (under `identical?` recursively); reference
types (closures, fibers) compare by identity.

```lisp
(assert (not (identical? 1 1.0)))         # no coercion
(assert (identical? [nan] [nan]))         # bit-pattern floats
```

### Keys and membership

Sets, struct keys, `distinct`, `hash` and `compare` use a *key
equivalence* rather than `=`. It agrees with `=` on numbers and on
mutability: `1` and `1.0` are the same set element, and so are `[1]` and
`@[1]`. It is reflexive for NaN (by bit pattern) and distinguishes `-0.0`
from `0.0`, so a collection holding a NaN stays findable in a set that
contains it. `distinct` raises on a mutable element today
([#1506](https://github.com/elle-lisp/elle/issues/1506)).

```lisp
(assert (= (length (set 1 1.0)) 1))       # coercion dedups
(assert (has? (set nan) nan))             # keys are NaN-reflexive
(assert (= (hash [1 2]) (hash @[1 2])))   # mutability is not part of a key
(assert (= (hash (set 1 2)) (hash (apply set (range 1 3)))) "two sets built apart")
```

Floats are not permitted as struct keys:

```lisp
(def [float-key? err] (protect {1.5 :x}))
(assert (not float-key?))
(assert (= (get err :error) :type-error))
```

## Order

Every value has a place in one total order. `compare` returns -1, 0 or 1
by it, `sort` sorts by it, and a set and a struct keep their members and
keys in it. `compare` returns 0 exactly when the two values are the same
key, so the order distinguishes what the key equivalence distinguishes.

```lisp
(assert (= -1 (compare 1 2)))
(assert (= 0 (compare 1 1.0)) "numbers order by value")
(assert (= -1 (compare -0.0 0.0)) "-0.0 orders before 0.0")
(assert (= 0 (compare [1] @[1])) "mutability is not part of the order")
(assert (= -1 (compare [1 2] [1 3])) "arrays order element by element")
(assert (= -1 (compare [1] [1 0])) "a prefix orders first")
(assert (= [[0 9] [1 2] [1 3]] (sort [[1 3] [0 9] [1 2]])))
(assert (= 1 (compare "b" "a")))
(assert (= (compare 'a 'b) (- (compare 'b 'a))) "symbols order too")
```

Values of two kinds order by kind, in this sequence: nil, boolean, number,
symbol, keyword, C pointer, string, empty list, list pair, array, bytes,
struct, closure, box, native function, library handle, thread, fiber,
syntax, FFI signature, FFI type, managed pointer, external, parameter, set.
A kind and its mutable twin share one place.

```lisp
(assert (= [nil true 1 :k "a" [1]] (sort ["a" [1] :k 1 true nil])))
(assert (= ["a" () (list 1)] (sort [(list 1) () "a"])) "a list's two forms sit together")
(assert (= (keys {"a" 1 :a 2}) (list :a "a")) "struct keys use the same order")
```

Within a kind:

- `false` orders before `true`.
- Numbers order by value, under the coercion and the precision caveat that
  `=` has, above. `-0.0` orders before `0.0`, and NaN has one fixed place.
- Symbols and keywords order by their name hash. The order is the same in
  every run, and it has no alphabetical meaning.
- Strings, byte sequences, arrays and lists order element by element, and a
  prefix orders first. A struct orders entry by entry, and a set member by
  member, each in this order.
- Closures, fibers and the other reference values order by identity. Their
  place is fixed while they live, and nothing else about it is promised.

## Mutability

Collections come in immutable/mutable pairs. Bare syntax is immutable;
`@` makes it mutable. `put` on immutable returns a new copy; `put` on
mutable mutates in place.

| immutable | mutable | syntax |
|-----------|---------|--------|
| array | @array | `[...]` / `@[...]` |
| struct | @struct | `{...}` / `@{...}` |
| string | @string | `"..."` / `@"..."` |
| bytes | @bytes | `b[...]` / `@b[...]`, or `(bytes ...)` / `(@bytes ...)` |
| set | @set | `\|...\|` / `@\|...\|` |

```lisp
(def fixed [1 2])
(assert (= (put fixed 0 9) [9 2]))
(assert (= fixed [1 2]))                  # the original is unchanged
(def growable @[1 2])
(put growable 0 9)
(assert (= growable @[9 2]))
```

### freeze and thaw

`freeze` converts mutable → immutable. `thaw` copies immutable → mutable.
Both are shallow.

```lisp
(assert (= (type-of (freeze @[1 2])) :array))
(assert (= (type-of (thaw [1 2])) :@array))
(assert (= (type-of (freeze @"hi")) :string))
```

`deep-freeze` recursively freezes nested mutable collections:

```lisp
(def frozen (deep-freeze @[1 @[2 3]]))
(assert (= (type-of frozen) :array))
(assert (= (type-of (get frozen 1)) :array))   # the inner one froze too
```

---

## See also

- [syntax.md](syntax.md) — reader syntax and collection literals
- [arrays.md](arrays.md) — array and @array operations
- [structs.md](structs.md) — struct and @struct operations
- [strings.md](strings.md) — string and @string operations
- [bytes.md](bytes.md) — bytes and @bytes operations
