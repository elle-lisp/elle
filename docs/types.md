# Types

Elle values are 16-byte tagged unions. Every value carries a type keyword
returned by `type` (alias: `type-of`).

## Type keywords

```lisp
(type-of 42)          # => :integer
(type-of 3.14)        # => :float
(type-of "hello")     # => :string
(type-of :foo)        # => :keyword
(type-of 'foo)        # => :symbol
(type-of true)        # => :boolean
(type-of nil)         # => :nil
(type-of [1 2])       # => :array
(type-of @[1 2])      # => :@array
(type-of {:a 1})      # => :struct
(type-of @{:a 1})     # => :@struct
(type-of @"hi")       # => :@string
(type-of |1 2|)       # => :set
(type-of (fn [] 1))   # => :closure
(type-of +)           # => :native-fn
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

### List predicates and nil

`nil` and `()` are distinct. This table is authoritative:

```text
Expression       Result    Notes
─────────────────────────────────────
(nil? nil)       true      only nil is nil
(nil? ())        false     empty list is NOT nil
(empty? ())      true      empty list is empty
(empty? nil)     error     nil is not a container
(list? ())       true      empty list is a list
(list? nil)      false     nil is not a list
(pair? ())       false     empty list has no car/cdr
(pair? nil)      false
```

## Truthiness

Only `nil` and `false` are falsy. Everything else is truthy —
including `0`, `""`, `()`, `[]`, and `@[]`.

```lisp
(if 0   :yes :no)    # => :yes  — unlike C/Python
(if ""  :yes :no)    # => :yes
(if ()  :yes :no)    # => :yes  — empty list is truthy
(if nil :yes :no)    # => :no
```

## Conversions

### String ↔ number

```lisp
(integer "42")             # => 42
(integer "ff" 16)          # => 255 (radix 2-36)
(integer "1010" 2)         # => 10
(float "3.14")             # => 3.14

(number->string 42)        # => "42"
(number->string 255 16)    # => "ff"
(number->string 255 2)     # => "11111111"
```

### Numeric coercion

```lisp
(integer 3.7)              # => 3 (truncates)
(float 42)                 # => 42.0
```

### To string

`string` converts any value to its string representation:

```lisp
(string 42)                # => "42"
(string :hello)            # => "hello" (no colon)
(string 'hello)            # => "hello"
(string @"hello")          # => "hello" (@string → string)
```

## Equality

`=` is structural equality. It works across mutability boundaries.

```lisp
(= [1 2 3] @[1 2 3])      # => true  — same contents
(= {:a 1} {:a 1})          # => true
(= 1 1.0)                  # => true  — numeric coercion
```

**Precision caveat:** mixed int/float comparisons coerce through f64.
Integers beyond 2^53 may compare equal when they shouldn't:
`(= 9007199254740992 9007199254740993)` returns `true`.

Closures compare by reference:

```lisp
(def f (fn [x] x))
(def g (fn [x] x))
(= f f)                    # => true
(= f g)                    # => false — different objects
```

## Mutability

Collections come in immutable/mutable pairs. Bare syntax is immutable;
`@` makes it mutable. `put` on immutable returns a new copy; `put` on
mutable mutates in place.

```text
immutable    mutable      syntax
───────────────────────────────────
array        @array       [...]  / @[...]
struct       @struct      {...}  / @{...}
string       @string      "..."  / @"..."
bytes        @bytes       (bytes ...)  / (@bytes ...)
set          @set         |...|  / @|...|
```

### freeze and thaw

`freeze` converts mutable → immutable. `thaw` copies immutable → mutable.
Both are shallow.

```lisp
(type-of (freeze @[1 2]))     # => :array
(type-of (thaw [1 2]))        # => :@array
(type-of (freeze @"hi"))      # => :string
```

`deep-freeze` recursively freezes nested mutable collections:

```lisp
(def frozen (deep-freeze @[1 @[2 3]]))
(type-of frozen)               # => :array
(type-of (get frozen 1))       # => :array (inner was also frozen)
```

---

## See also

- [syntax.md](syntax.md) — reader syntax and collection literals
- [arrays.md](arrays.md) — array and @array operations
- [structs.md](structs.md) — struct and @struct operations
- [strings.md](strings.md) — string and @string operations
- [bytes.md](bytes.md) — bytes and @bytes operations
