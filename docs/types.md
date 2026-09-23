# Types

<!-- audited: 2026-09-23 -->

Elle values are 16-byte tagged unions. Every value carries a type keyword
returned by `type` (alias: `type-of`).

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

The full set:

```text
:nil :boolean :integer :float :symbol :keyword
:list :array :@array :struct :@struct
:string :@string :bytes :@bytes :set :@set
:closure :native-fn :box :fiber :parameter
:ptr :managed-ptr :syntax
:lib-handle :ffi-signature :ffi-type
```

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

Closures compare by reference:

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

Sets, struct keys, `distinct`, and `hash` use a *key equivalence*
rather than `=`. It agrees with `=` on numbers — `1` and `1.0` are the
same set element — but is reflexive for NaN (by bit pattern) and
distinguishes `-0.0` from `0.0`, so that a collection holding a NaN
remains findable in a set that contains it:

```lisp
(assert (= (length (set 1 1.0)) 1))       # coercion dedups
(assert (has? (set nan) nan))             # keys are NaN-reflexive
```

Floats are not permitted as struct keys:

```lisp
(def [float-key? err] (protect {1.5 :x}))
(assert (not float-key?))
(assert (= (get err :error) :type-error))
```

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
