# Elle Type System

This document defines the authoritative type system for Elle. When code or
other documentation contradicts this document, **this document is correct**.

## Contents

- [Design principle: the mutable/immutable split](#design-principle-the-mutableimmutable-split)
- [All types](#all-types)
- [Type predicates](#type-predicates)
- [Display format](#display-format)
- [Truthiness](#truthiness)
- [Equality](#equality)
- [Mutability summary](#mutability-summary)

## Design principle: the mutable/immutable split

Elle follows Janet's approach to collection types: every collection has an
immutable variant and a mutable variant. The immutable variant has bare literal
syntax# the mutable variant uses the same delimiters prefixed with `@`.

| Immutable | Mutable | Delimiters | Immutable example | Mutable example |
|-----------|---------|------------|-------------------|-----------------|
| array | @array | `[]` | `[1 2 3]` | `@[1 2 3]` |
| struct | @struct | `{}` | `{:a 1 :b 2}` | `@{:a 1 :b 2}` |
| string | @string | `""` | `"hello"` | `@"hello"` |
| bytes | @bytes | *(no literal)* | `(bytes 1 2 3)` | `(@bytes 1 2 3)` |
| set | @set | `\|\|` | `\|1 2 3\|` | `@\|1 2 3\|` |

The `@` prefix means "mutable version of this literal." This is the only
syntax difference between the two variants of each pair. The types within each
pair share the same logical structure (sequential indexing, key-value mapping,
byte sequence) but differ in mutability.

### Why this matters

Bracket syntax `[1 2 3]` creates an **array** (immutable). If you want a
mutable array, write `@[1 2 3]`. This is not a cosmetic distinction — it
affects what operations are valid:

```janet
(def t [1 2 3])        # array — immutable
(def a @[1 2 3])       # @array — mutable
(array-set! a 0 99)    # ok
(array-set! t 0 99)    # error: arrays are immutable
```

Error values are structs: `{:error :division-by-zero :message "division by zero"}`.
`try/catch` binds error structs, and `{:error kind :message msg}` destructures them.

### Current implementation status

| Feature | Status |
|---------|--------|
| `[...]` → array | ✅ Correct |
| `@[...]` → @array | ✅ Correct |
| `{...}` → struct | ✅ Correct |
| `@{...}` → @struct | ✅ Correct |
| `"..."` → string | ✅ Correct |
| `@"..."` → @string | ✅ Desugars to `(thaw "...")` |

---

## All types

Elle has two categories of values: **immediates** (encoded directly in a
NaN-boxed 64-bit word, no heap allocation) and **heap values** (reference-
counted, accessed via pointer).

### Immediate types

These fit in 8 bytes with no allocation.

#### nil

The absence of a value. One of two falsy values (with `false`).

```janet
nil             # literal
(nil? x)        # predicate
```

Not the same as the empty list `()`. `nil` is falsy# `()` is truthy.

#### boolean

```janet
true              # true (truthy)
false              # false (falsy)
(boolean? x)    # predicate
```

#### integer

48-bit signed integer. Range: -2^47 to 2^47-1.

```janet
42              # decimal literal
-17             # negative literal
0               # zero
0xFF            # hexadecimal (= 255)
0o755           # octal (= 493)
0b1010          # binary (= 10)
1_000_000       # underscores for readability
0xFF_FF         # underscores in hex
(number? x)     # predicate (true for int or float)
```

No automatic coercion to float. Overflow panics.

#### float

IEEE 754 double-precision. NaN and Infinity are heap-allocated to avoid
collision with the NaN-boxing scheme.

```janet
3.14            # literal
1e10            # scientific notation (= 10000000000.0)
1.5e-3          # negative exponent (= 0.0015)
1_000.5_5       # underscores for readability
(number? x)     # predicate (true for int or float)
```

#### symbol

Interned identifier. Used for variable names, function names.

```janet
foo             # literal
'foo            # quoted
(symbol? x)     # predicate
```

#### keyword

Self-evaluating interned name. Used for keys, tags, enum-like values.

```janet
:foo            # literal
:my-key         # literal
(keyword? x)    # predicate
```

#### empty list

The empty list `()`. Terminates proper lists. **Truthy** (it is a value, not
the absence of one).

```janet
()              # literal
'()             # quoted
(empty? x)      # predicate
```

#### pointer

Raw C pointer. 48-bit address space. FFI only. NULL becomes nil.

```janet
(ptr? x)        # predicate (alias: pointer?)
```

---

### Heap types: collections

#### array (immutable sequential)

Fixed-length immutable sequence. The immutable counterpart of @array.

```janet
[1 2 3]         # literal
(array 1 2 3)   # constructor
```

Error values are structs: `{:error :kind :message "message"}`. Struct
destructuring extracts error fields:

```janet
(try (/ 1 0) (catch {:error kind :message msg} kind))  # => :division-by-zero
(let (([a b] [1 2])) a)                                # => 1
```

In `match`, bracket patterns `[a b]` match **arrays only** (the `IsArray`
guard rejects @arrays). This is intentional — `match` is about type
discrimination. Destructuring in `let`/`def`/`fn` works on both.

#### @array (mutable sequential)

Mutable resizable sequence. The mutable counterpart of array.

```janet
@[1 2 3]        # literal
(@array 1 2 3)  # constructor
(array-ref a 0) # indexed access
(array-set! a 0 99) # mutation
(array-length a)    # length
(array? x)      # predicate
```

#### struct (immutable key-value)

Immutable ordered dictionary. The immutable counterpart of @struct.

```janet
{:a 1 :b 2}    # literal
(struct :a 1 :b 2)  # constructor
(get s :a)      # access
(struct? x)     # predicate
```

#### @struct (mutable key-value)

Mutable ordered dictionary. The mutable counterpart of struct.

```janet
@{:a 1 :b 2}   # literal
(@struct :a 1 :b 2)  # constructor
(get t :a)      # access
(put t :a 99)   # mutation
(del t :a)      # deletion
(keys t)        # key list
(values t)      # value list
(has-key? t :a) # membership
(table? x)      # predicate
```

#### string (immutable text)

Immutable interned text. The immutable counterpart of @string.

```janet
"hello"         # literal
(string? x)     # predicate
```

Strings are interned — equality is O(1).

#### @string (mutable text)

Mutable byte sequence. The mutable counterpart of string.

```janet
@"hello"        # literal (desugars to (thaw "hello"))
(@string 72 101) # constructor from bytes
(thaw "hello")  # from string (UTF-8 bytes)
(freeze buf)    # to string (UTF-8, errors on invalid UTF-8)
(get buf 0)     # byte at index (as integer)
(put buf 0 88)  # set byte at index
(push buf 33)   # append byte
(pop buf)       # remove and return last byte
(length buf)    # byte count
(empty? buf)    # empty check
(append b1 b2)  # mutate b1 by extending with b2
(concat b1 b2)  # return new @string
```

#### bytes (immutable binary data)

Immutable byte sequence. No literal syntax. Displays as `#bytes[hex ...]`.

```janet
(bytes 1 2 3)       # constructor from integers
(bytes "hello")     # constructor from string (UTF-8 encoding)
(get b 0)           # byte at index
(length b)          # byte count
(bytes->hex b)      # hex string
(bytes? x)          # predicate (matches bytes or @bytes)
```

#### @bytes (mutable binary data)

Mutable byte sequence. No literal syntax. Displays as `#@bytes[hex ...]`.

```janet
(@bytes 1 2 3)      # constructor from integers
(@bytes "hello")    # constructor from string (UTF-8 encoding)
(get b 0)           # byte at index
(put b 0 99)        # set byte at index
(push b 33)         # append byte
(pop b)             # remove and return last byte
(length b)          # byte count
(bytes->hex b)      # hex @string (preserves mutability)
(bytes? x)          # predicate (matches bytes or @bytes)
```

#### set (immutable unique collection)

Immutable ordered collection of unique values.

```janet
|1 2 3|             # literal
(contains? s 2)     # membership
(set? x)            # predicate (matches set or @set)
```

#### @set (mutable unique collection)

Mutable ordered collection of unique values.

```janet
@|1 2 3|            # literal
(add s 4)           # add element
(del s 1)           # remove element
(contains? s 2)     # membership
(union s1 s2)       # set union
(intersection s1 s2) # set intersection
(difference s1 s2)  # set difference
(set? x)            # predicate (matches set or @set)
```

---

### Heap types: lists

#### cons cell / list

Singly-linked list built from cons cells. Proper lists terminate with `()`.

```janet
(list 1 2 3)    # constructor
'(1 2 3)        # quoted literal
(cons 1 (list 2 3)) # manual construction
(first l)       # car
(rest l)        # cdr
(pair? x)       # predicate (cons cell?)
(list? x)       # predicate (cons or empty list?)
(empty? x)      # predicate (empty list?)
```

Lists are **not** the same as arrays or @arrays. Lists are linked; arrays and
@arrays are contiguous in memory.

---

### Heap types: functions

#### closure

Compiled function with captured environment.

```janet
(fn (x) (+ x 1))       # anonymous
(defn add1 (x) (+ x 1)) # named (macro)
(closure? x)            # predicate
```

Closures capture by value. Mutable captures use `LocalLBox` (compiler-
managed, auto-unwrapped). The `lbox_params_mask` tracks which parameters
need lbox wrapping.

#### native function

Rust function registered as a primitive. Not directly constructible from Elle.

```janet
# No literal syntax. Primitives like +, -, cons are native functions.
```

---

### Heap types: concurrency

#### fiber

Independent execution context with its own stack, call frames, and signal
mask. See `docs/fibers.md` for the full fiber architecture.

```janet
(fiber/new (fn () body) mask) # constructor
(fiber/resume f value)        # resume
(fiber/status f)              # status keyword
(fiber/value f)               # last value
(fiber? x)                    # predicate
```

#### box (lbox)

Mutable box. Two variants:

- **User box** (`box`): explicit creation and dereferencing.
- **Local lbox**: compiler-created for mutable captures. Auto-unwrapped by
  `LoadUpvalue`. Users never see these directly.

```janet
(box 42)        # create user box
(unbox c)       # read
(rebox c 99)    # write
(box? c)        # predicate
```

#### parameter

Dynamic binding. `(make-parameter default)` creates one; calling it reads the
current value. `parameterize` sets it within a scope. Child fibers inherit
parent parameter frames.

```janet
(def *port* (make-parameter :stdout))
(*port*)            # read current value
(parameterize ((*port* :stderr))
  (*port*))         # => :stderr
(parameter? x)      # predicate
```

---

### Heap types: metaprogramming

#### syntax object

Wraps a syntax tree node with source location and scope information. Used
during macro expansion for hygiene.

```janet
# Created by quasiquote, quote, and macro expansion.
# Not typically constructed directly.
```

#### binding

Compile-time metadata for a variable binding. **Never appears at runtime.**
Stores name, scope (parameter/local/global), mutation and capture flags.

---

### Heap types: FFI

These types support the foreign function interface. They are not typically
used in application code.

#### library handle

Handle to a dynamically loaded C library. Created by `ffi/open`.

#### ffi signature

Reified function signature for calling C functions. Created by `ffi/signature`.

#### ffi type descriptor

Compound type descriptor (struct, array) for FFI marshalling.

#### managed pointer

Heap-tracked C pointer. Tracks freed state to prevent use-after-free.
Created by `ffi/malloc`.

```janet
(ptr? x)        # predicate (matches both raw and managed pointers; alias: pointer?)
```

---

## Type predicates

| Predicate | Matches |
|-----------|---------|
| `nil?` | nil only |
| `boolean?` | `true` or `false` |
| `number?` | integer or float |
| `integer?` | integer only |
| `float?` | float only |
| `symbol?` | symbol |
| `keyword?` | keyword |
| `string?` | string (immutable or @string) |
| `pair?` | cons cell |
| `list?` | cons cell or empty list |
| `empty?` | empty list, empty array, empty @array, empty struct, empty @struct, empty @string |
| `array?` | array (immutable or @array) |
| `struct?` | struct (immutable or @struct) |
| `bytes?` | bytes (immutable or @bytes) |
| `set?` | set (immutable or @set) |
| `box?` | box (mutable box) |
| `parameter?` | dynamic parameter |
| `mutable?` | any mutable value (@array, @string, @bytes, @struct, @set, box, parameter) |
| `function?` | closure or native function |
| `closure?` | closure only |
| `primitive?` | native function only |
| `fiber?` | fiber |
| `ptr?` / `pointer?` | raw C pointer or managed pointer |
| `zero?` | zero (integer or float) |
| `type` / `type-of` | returns type as keyword (`:integer`, `:string`, etc.) |

## Display format

| Type | Display | Notes |
|------|---------|-------|
| nil | `nil` | |
| boolean | `true` / `false` | |
| integer | `42` | |
| float | `3.14` | |
| symbol | `'foo` | Looked up in symbol table |
| keyword | `:foo` | |
| empty list | `()` | |
| string | `hello` | No quotes in Display |
| @string | `@"hello"` | |
| cons | `(1 2 3)` | `(a . b)` for improper |
| array | `[1 2 3]` | |
| @array | `@[1 2 3]` | |
| struct | `{:a 1}` | |
| @struct | `@{:a 1}` | |
| set | `\|1 2 3\|` | |
| @set | `@\|1 2 3\|` | |
| bytes | `#bytes[01 02 03]` | |
| @bytes | `#@bytes[01 02 03]` | |
| box | `<box value>` | |
| closure | `<closure>` | |
| native fn | `<native-fn>` | |
| fiber | `<fiber:status>` | |
| parameter | `<parameter id>` | |
| syntax | `#<syntax:...>` | |
| pointer | `<pointer 0x...>` | |

## Truthiness

Exactly two values are falsy:

| Value | Truthy? |
|-------|---------|
| `nil` | No |
| `false` | No |
| everything else | Yes |

This includes `()`, `0`, `0.0`, `""`, `[]`, `@[]`. All truthy. (Note: `[]` is an immutable array, `@[]` is a mutable @array.)

## Equality

Value equality (`=`) is structural for collections and interned for
strings/symbols/keywords. Identity is pointer equality for heap objects.

## Mutability summary

| Immutable | Mutable | Shared structure |
|-----------|---------|------------------|
| array `[]` | @array `@[]` | sequential indexing |
| struct `{}` | @struct `@{}` | key-value mapping |
| string `""` | @string `@""` | text |
| bytes | @bytes | binary data |
| set `\|\|` | @set `@\|\|` | unique values |
| — | box | mutable box (`box`/`unbox`/`rebox`) |
| — | parameter | dynamic binding |
| cons/list | — | linked list (always immutable) |
| nil, bool, int, float, symbol, keyword | — | immediates (always immutable) |
| closure | — | always immutable (captures may be mutable via lboxes) |
| fiber | — | always immutable (internal state is mutable, but the value is not) |
