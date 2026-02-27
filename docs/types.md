# Elle Type System

This document defines the authoritative type system for Elle. When code or
other documentation contradicts this document, **this document is correct**.

## Design principle: the mutable/immutable split

Elle follows Janet's approach to collection types: every collection has an
immutable variant and a mutable variant. The immutable variant has bare literal
syntax; the mutable variant uses the same delimiters prefixed with `@`.

| Immutable | Mutable | Delimiters | Immutable example | Mutable example |
|-----------|---------|------------|-------------------|-----------------|
| tuple | array | `[]` | `[1 2 3]` | `@[1 2 3]` |
| struct | table | `{}` | `{:a 1 :b 2}` | `@{:a 1 :b 2}` |
| string | buffer | `""` | `"hello"` | `@"hello"` |

The `@` prefix means "mutable version of this literal." This is the only
syntax difference between the two variants of each pair. The types within each
pair share the same logical structure (sequential indexing, key-value mapping,
byte sequence) but differ in mutability.

### Why this matters

Bracket syntax `[1 2 3]` creates a **tuple** (immutable). If you want a
mutable array, write `@[1 2 3]`. This is not a cosmetic distinction — it
affects what operations are valid:

```lisp
(def t [1 2 3])        ; tuple — immutable
(def a @[1 2 3])       ; array — mutable
(array-set! a 0 99)    ; ok
(array-set! t 0 99)    ; error: tuples are immutable
```

Error values are tuples: `[:division-by-zero "division by zero"]`. This is why
bracket destructuring must work on both tuples and arrays — `try/catch` binds
error tuples, and `[kind msg]` must destructure them.

### Current implementation status

| Feature | Status |
|---------|--------|
| `[...]` → tuple | ✅ Correct |
| `@[...]` → array | ✅ Correct |
| `{...}` → struct | ✅ Correct |
| `@{...}` → table | ✅ Correct |
| `"..."` → string | ✅ Correct |
| `@"..."` → buffer | ✅ Desugars to `(string->buffer "...")` |

---

## All types

Elle has two categories of values: **immediates** (encoded directly in a
NaN-boxed 64-bit word, no heap allocation) and **heap values** (reference-
counted, accessed via pointer).

### Immediate types

These fit in 8 bytes with no allocation.

#### nil

The absence of a value. One of two falsy values (with `#f`).

```lisp
nil             ; literal
(nil? x)        ; predicate
```

Not the same as the empty list `()`. `nil` is falsy; `()` is truthy.

#### boolean

```lisp
#t              ; true (truthy)
#f              ; false (falsy)
(boolean? x)    ; predicate
```

#### integer

48-bit signed integer. Range: -2^47 to 2^47-1.

```lisp
42              ; literal
-17             ; literal
0               ; literal
(number? x)     ; predicate (true for int or float)
```

No automatic coercion to float. Overflow panics.

#### float

IEEE 754 double-precision. NaN and Infinity are heap-allocated to avoid
collision with the NaN-boxing scheme.

```lisp
3.14            ; literal
1e10            ; literal
(number? x)     ; predicate (true for int or float)
```

#### symbol

Interned identifier. Used for variable names, function names.

```lisp
foo             ; literal
'foo            ; quoted
(symbol? x)     ; predicate
```

#### keyword

Self-evaluating interned name. Used for keys, tags, enum-like values.

```lisp
:foo            ; literal
:my-key         ; literal
(keyword? x)    ; predicate
```

#### empty list

The empty list `()`. Terminates proper lists. **Truthy** (it is a value, not
the absence of one).

```lisp
()              ; literal
'()             ; quoted
(empty? x)      ; predicate
```

#### pointer

Raw C pointer. 48-bit address space. FFI only. NULL becomes nil.

```lisp
(pointer? x)    ; predicate
```

---

### Heap types: collections

#### tuple (immutable sequential)

Fixed-length immutable sequence. The immutable counterpart of array.

```lisp
[1 2 3]         ; literal (desired — currently creates array)
(tuple 1 2 3)   ; constructor
```

Error values are tuples: `[:kind "message"]`. Bracket destructuring works on
tuples:

```lisp
(try (/ 1 0) (catch [kind msg] kind))  ; => :division-by-zero
(let (([a b] [1 2])) a)                ; => 1
```

In `match`, bracket patterns `[a b]` match **arrays only** (the `IsArray`
guard rejects tuples). This is intentional — `match` is about type
discrimination. Destructuring in `let`/`def`/`fn` works on both.

#### array (mutable sequential)

Mutable resizable sequence. The mutable counterpart of tuple.

```lisp
@[1 2 3]        ; literal (desired — currently creates list)
(array 1 2 3)   ; constructor
(array-ref a 0) ; indexed access
(array-set! a 0 99) ; mutation
(array-length a)    ; length
(array? x)      ; predicate
```

#### struct (immutable key-value)

Immutable ordered dictionary. The immutable counterpart of table.

```lisp
{:a 1 :b 2}    ; literal
(struct :a 1 :b 2)  ; constructor
(get s :a)      ; access
(struct? x)     ; predicate
```

#### table (mutable key-value)

Mutable ordered dictionary. The mutable counterpart of struct.

```lisp
@{:a 1 :b 2}   ; literal
(table :a 1 :b 2)  ; constructor
(get t :a)      ; access
(put t :a 99)   ; mutation
(del t :a)      ; deletion
(keys t)        ; key list
(values t)      ; value list
(has-key? t :a) ; membership
(table? x)      ; predicate
```

#### string (immutable text)

Immutable interned text. The immutable counterpart of buffer.

```lisp
"hello"         ; literal
(string? x)     ; predicate
```

Strings are interned — equality is O(1).

#### buffer (mutable text)

Mutable byte sequence. The mutable counterpart of string.

```lisp
@"hello"        ; literal (desugars to (string->buffer "hello"))
(buffer? x)     ; predicate
(buffer 72 101)  ; constructor from bytes
(string->buffer "hello")  ; from string (UTF-8 bytes)
(buffer->string buf)      ; to string (UTF-8)
(get buf 0)     ; byte at index (as integer)
(put buf 0 88)  ; set byte at index
(push buf 33)   ; append byte
(pop buf)       ; remove and return last byte
(length buf)    ; byte count
(empty? buf)    ; empty check
(append b1 b2)  ; mutate b1 by extending with b2
(concat b1 b2)  ; return new buffer
```

---

### Heap types: lists

#### cons cell / list

Singly-linked list built from cons cells. Proper lists terminate with `()`.

```lisp
(list 1 2 3)    ; constructor
'(1 2 3)        ; quoted literal
(cons 1 (list 2 3)) ; manual construction
(first l)       ; car
(rest l)        ; cdr
(pair? x)       ; predicate (cons cell?)
(list? x)       ; predicate (cons or empty list?)
(empty? x)      ; predicate (empty list?)
```

Lists are **not** the same as tuples or arrays. Lists are linked; tuples and
arrays are contiguous in memory.

---

### Heap types: functions

#### closure

Compiled function with captured environment.

```lisp
(fn (x) (+ x 1))       ; anonymous
(defn add1 (x) (+ x 1)) ; named (macro)
(closure? x)            ; predicate
```

Closures capture by value. Mutable captures use `LocalCell` (compiler-
managed, auto-unwrapped). The `cell_params_mask` tracks which parameters
need cell wrapping.

#### native function

Rust function registered as a primitive. Not directly constructible from Elle.

```lisp
; No literal syntax. Primitives like +, -, cons are native functions.
```

---

### Heap types: concurrency

#### fiber

Independent execution context with its own stack, call frames, and signal
mask. See `docs/fibers.md` for the full fiber architecture.

```lisp
(fiber/new (fn () body) mask) ; constructor
(fiber/resume f value)        ; resume
(fiber/status f)              ; status keyword
(fiber/value f)               ; last value
(fiber? x)                    ; predicate
```

#### cell

Mutable box. Two variants:

- **User cell** (`box`): explicit creation and dereferencing.
- **Local cell**: compiler-created for mutable captures. Auto-unwrapped by
  `LoadUpvalue`. Users never see these directly.

```lisp
(box 42)        ; create user cell
(unbox c)       ; read
(set-box! c 99) ; write
```

---

### Heap types: metaprogramming

#### syntax object

Wraps a syntax tree node with source location and scope information. Used
during macro expansion for hygiene.

```lisp
; Created by quasiquote, quote, and macro expansion.
; Not typically constructed directly.
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

```lisp
(pointer? x)    ; predicate (matches both raw and managed pointers)
```

---

## Type predicates

| Predicate | Matches |
|-----------|---------|
| `nil?` | nil only |
| `boolean?` | `#t` or `#f` |
| `number?` | integer or float |
| `symbol?` | symbol |
| `keyword?` | keyword |
| `string?` | string |
| `pair?` | cons cell |
| `list?` | cons cell or empty list |
| `empty?` | empty list, empty array, empty tuple, empty table, empty struct |
| `array?` | array |
| `tuple?` | tuple |
| `table?` | table |
| `struct?` | struct |
| `closure?` | closure |
| `fiber?` | fiber |
| `pointer?` | raw C pointer or managed pointer |

**Note**: `array?`, `tuple?`, `table?`, and `struct?` are not yet exposed as
primitives. They need to be added.

## Display format

| Type | Display | Notes |
|------|---------|-------|
| nil | `nil` | |
| boolean | `true` / `false` | |
| integer | `42` | |
| float | `3.14` | |
| symbol | `foo` | Looked up in symbol table |
| keyword | `:foo` | |
| empty list | `()` | |
| string | `hello` | No quotes in Display |
| cons | `(1 2 3)` | `(a . b)` for improper |
| tuple | `[1 2 3]` | Same delimiters as array |
| array | `@[1 2 3]` | Desired. Currently displays as `[1 2 3]` |
| struct | `{:a 1}` | |
| table | `@{:a 1}` | Desired. Currently displays as `{:a 1}` |
| closure | `<closure>` | |
| native fn | `<native-fn>` | |
| fiber | `<fiber:status>` | |
| cell | `<cell value>` | |
| syntax | `#<syntax:...>` | |
| pointer | `<pointer 0x...>` | |

## Truthiness

Exactly two values are falsy:

| Value | Truthy? |
|-------|---------|
| `nil` | No |
| `#f` | No |
| everything else | Yes |

This includes `()`, `0`, `0.0`, `""`, `[]`, `@[]`. All truthy.

## Equality

Value equality (`=`) is structural for collections and interned for
strings/symbols/keywords. Identity is pointer equality for heap objects.

## Mutability summary

| Immutable | Mutable | Shared structure |
|-----------|---------|------------------|
| tuple `[]` | array `@[]` | sequential indexing |
| struct `{}` | table `@{}` | key-value mapping |
| string `""` | buffer `@""` | byte sequence |
| cons/list | — | linked list (always immutable) |
| nil, bool, int, float, symbol, keyword | — | immediates (always immutable) |
| closure | — | always immutable (captures may be mutable via cells) |
| fiber | — | always mutable (internal state) |
