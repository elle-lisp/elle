# FFI

<!-- audited: 2026-10-13 -->

Elle calls C functions through libffi, with keyword type descriptors, reified signatures and explicit marshalling.

The design follows Janet's FFI. The implementation lives in
[src/ffi](../src/ffi/AGENTS.md), which also documents the marshalling layer and
its invariants. The primitives live in
[src/primitives/loading.rs](../src/primitives/loading.rs) and
[src/primitives/memory.rs](../src/primitives/memory.rs).

## Contents

- [Quick start](#quick-start)
- [Library lifetime](#library-lifetime)
- [Type descriptors](#type-descriptors)
- [Compound types](#compound-types)
- [Signatures](#signatures)
- [Calling C functions](#calling-c-functions)
- [Memory management](#memory-management)
- [Callbacks](#callbacks)
- [The `ffi/defbind` macro](#the-ffidefbind-macro)
- [Pointers and NULL](#pointers-and-null)
- [Errors](#errors)
- [Signals and capabilities](#signals-and-capabilities)


## Quick start

```lisp
# Load the current process (dlopen(NULL))
(def libc (ffi/native nil))

# Look up a symbol
(def sqrt-ptr (ffi/lookup libc "sqrt"))

# Create a signature: return type, [arg types]
(def sqrt-sig (ffi/signature :double [:double]))

# Call it
(ffi/call sqrt-ptr sqrt-sig 2.0)         # => 1.4142135623730951

# Or use the convenience macro
(ffi/defbind sqrt libc "sqrt" :double [:double])
(sqrt 2.0)                               # => 1.4142135623730951
```


## Library lifetime

`ffi/native` holds a loaded library in a process-global registry
([registry.rs](../src/ffi/registry.rs)), one entry per canonical path. The
registry never calls `dlclose`, so the mapping lasts for the life of the
process, as a plugin's does. The handle `ffi/native` returns is a per-VM id
into this registry, and `ffi/lookup` resolves symbols through it.

This makes FFI safe across worker threads. Many C libraries, for example
libgit2 through OpenSSL, register thread-local destructors with
`pthread_key_create`. If such a library were unmapped when a worker that used
it exited, glibc's thread-exit destructor walk would jump into unmapped code
and crash the process. The mapping stays, so the destructor always runs
against mapped code, and a worker needs no teardown.

Library loading needs Unix: Linux, macOS or a BSD. On any other platform,
`ffi/native` signals an error. A Linux-style spec such as `"libm.so.6"` is
rewritten to the host's native form (`.dylib` or `.dll`) before loading, so
the same call is portable.

Teardown is optional and explicit. A program may declare a library's ordered
teardown, a zero-argument C symbol such as `git_libgit2_shutdown`. It runs the
declared teardowns when it decides that teardown is safe, after its worker
threads have quiesced. `ffi/on-unload` resolves the symbol at once, so a typo
errors at declaration and not at shutdown. `ffi/run-teardowns` calls every
declared symbol in reverse load order:

```lisp
# Guarded because libgit2 may not be installed; when it is, this loads it,
# declares its shutdown hook, and runs it for real.
(def [have-git? git-lib] (protect (ffi/native "libgit2.so")))
(if (not have-git?)
  (println "SKIP: libgit2 not installed")
  (do
    (ffi/on-unload git-lib "git_libgit2_shutdown")  # declare the teardown destructor
    (ffi/run-teardowns)))                           # run them (reverse load order)
```

The runtime never runs a teardown by itself. A teardown such as
`git_libgit2_shutdown` deletes the pthread key it created, which would race a
detached worker that is still exiting. A teardown also never unmaps the
library, which is what keeps a late worker's destructor safe. A program that
skips teardown is always safe.


## Type descriptors

A keyword names each C type. `TypeDesc` in
[types.rs](../src/ffi/types.rs) maps each keyword to its C equivalent.

### Primitive types

| Keyword | C type | Size (bytes) | Notes |
|---------|--------|-------------|-------|
| `:void` | `void` | — | Return type only, not valid for an argument |
| `:bool` | `bool` (`_Bool`) | 1 | See [Booleans](#booleans) |
| `:bool32` | `int32_t` used as a boolean | 4 | See [Booleans](#booleans) |
| `:i8` | `int8_t` | 1 | |
| `:u8` | `uint8_t` | 1 | |
| `:i16` | `int16_t` | 2 | |
| `:u16` | `uint16_t` | 2 | |
| `:i32` | `int32_t` | 4 | |
| `:u32` | `uint32_t` | 4 | |
| `:i64` | `int64_t` | 8 | |
| `:u64` | `uint64_t` | 8 | |
| `:float` | `float` | 4 | Accepts an integer or a float |
| `:double` | `double` | 8 | Accepts an integer or a float |
| `:int` | `int` | platform | Typically 4 bytes |
| `:uint` | `unsigned int` | platform | |
| `:long` | `long` | platform | 8 bytes on LP64 |
| `:ulong` | `unsigned long` | platform | |
| `:char` | `char` | 1 | Read as signed |
| `:uchar` | `unsigned char` | 1 | |
| `:short` | `short` | platform | Typically 2 bytes |
| `:ushort` | `unsigned short` | platform | |
| `:size` | `size_t` | platform | 8 bytes on 64-bit |
| `:ssize` | `ptrdiff_t` | platform | 8 bytes on 64-bit |
| `:ptr` | `void *` | platform | A pointer, or nil for NULL |
| `:string` | `const char *` | platform | See [Calling C functions](#calling-c-functions) |

### Booleans

`:bool` is the C99 `bool`, which C also spells `_Bool`. It is one byte. A
write stores 1 for a truthy value and 0 for a falsy one. A read takes any
nonzero byte as `true`.

Declare a C99 `bool` return as `:bool`, never as `:int`. The x86-64 and
AArch64 calling conventions define only the low byte of a returned `bool`. A
compiler may leave other bits in the rest of the register. Read as an `int`,
a function that returned `false` can look like it returned `true`.

`:bool32` is a boolean held in a 32-bit integer. GLib's `gboolean`, Win32's
`BOOL` and Vulkan's `VkBool32` all have this shape. It is four bytes. A write
stores 1 or 0. A read takes any nonzero value as `true`.

```lisp
(assert (= (ffi/size :bool) 1) ":bool is one byte")
(assert (= (ffi/align :bool) 1) ":bool aligns to one byte")
(assert (= (ffi/size :bool32) 4) ":bool32 is four bytes")
(assert (= (ffi/size (ffi/struct [:u8 :bool :u8])) 3) "a :bool field packs like a byte")

(def flags (ffi/malloc 4))
(ffi/write flags :bool32 true)
(assert (= (ffi/read flags :u32) 1) ":bool32 writes all four bytes")
(ffi/write flags :u8 0)
(ffi/write (ptr/add flags 1) :u8 255)
(assert (= (ffi/read flags :bool) false) ":bool reads only its own byte")
(ffi/free flags)
```

### Type introspection

```lisp
(ffi/size :i32)     # => 4
(ffi/size :double)  # => 8
(ffi/size :void)    # => nil
(ffi/align :double) # => 8
(ffi/align :ptr)    # => 8  (on 64-bit)
```


## Compound types

### Structs

`ffi/struct` makes a struct type descriptor from an array or a list of field
types. Fields are positional and unnamed. The layout follows C: each field
starts at the next offset aligned for it, and the size rounds up to the
largest field alignment.

```lisp
# struct { int32_t x, double y }
(def point-type (ffi/struct [:i32 :double]))

(ffi/size point-type)   # => 16  (4 + 4 padding + 8)
(ffi/align point-type)  # => 8

# Nested structs
(def inner (ffi/struct [:i8 :i32]))      # 8 bytes (1 + 3 padding + 4)
(def outer (ffi/struct [:i64 inner]))    # 16 bytes
```

A struct value is an Elle array, mutable or immutable, with one element per
field. Marshalling to C writes each element at its field's offset. Reading
from C builds an array of the fields.

```lisp
# Write a struct to memory
(def buf (ffi/malloc (ffi/size point-type)))
(ffi/write buf point-type [42 1.5])

# Read it back
(ffi/read buf point-type)  # => [42 1.5]
(ffi/free buf)
```

A struct needs at least one field, and `:void` is not a valid field type. The
array must hold exactly one element per field.

### Arrays

`ffi/array` makes a fixed-size array type descriptor.

```lisp
# int32_t[10]
(def arr-type (ffi/array :i32 10))

(ffi/size arr-type)   # => 40
(ffi/align arr-type)  # => 4
```

An array value is also an Elle array. The count must be positive, and a value
must hold exactly that many elements. An array of a one-byte integer type
(`:u8`, `:i8`, `:uchar` or `:char`) reads back as `bytes`, and `ffi/write`
also accepts `bytes` or `@bytes` for it.


## Signatures

A signature describes a C function's calling convention, return type and
argument types. `ffi/signature` makes one as a first-class value.

```lisp
# Non-variadic: (return-type [arg-types...])
(def sig (ffi/signature :int [:int :int]))

# Variadic: (return-type [all-arg-types...] fixed-count)
# For printf(const char *, ...): 1 fixed arg, rest variadic
(def printf-sig (ffi/signature :int [:ptr :int] 1))
```

The third argument is the number of fixed arguments of a variadic function.
It must be in the range `[0, len(arg-types)]`. Without it, the signature is
not variadic.

A signature accepts keywords such as `:i32` and compound type values from
`ffi/struct` or `ffi/array`, as argument types and as the return type.


## Calling C functions

`ffi/call` takes a function pointer, a signature, and the arguments:

```lisp
# (ffi/call fn-ptr sig arg1 arg2 ...)
```

The number of arguments must match the signature's argument count exactly.

```lisp
(def libc (ffi/native nil))
(def abs-ptr (ffi/lookup libc "abs"))
(def abs-sig (ffi/signature :int [:int]))

(ffi/call abs-ptr abs-sig -42)  # => 42
```

### Argument marshalling

`ffi/call` converts each argument to C storage of its declared type.
`ffi/write` converts its value by the same rules, and so does each struct
field and array element:

- **Integers**: Range-checked and narrowed. Passing 256 as `:i8` is an error.
- **Floats**: `:float` and `:double` accept an integer or a float.
- **Booleans**: `:bool` and `:bool32` pass 1 for a truthy value and 0 for a
  falsy one.
- **Pointers**: A pointer, a pointer from `ffi/malloc`, or nil for NULL. A
  freed pointer is an error.
- **Strings**: Copied to a NUL-terminated C string that lives for the call.
  Nil passes NULL, and a string with an interior NUL byte is an error.
- **Structs and arrays**: An Elle array, written into an aligned buffer field
  by field.

### Return value conversion

`ffi/call` converts the C return value to an Elle value:

- An integer type returns an integer. A `:u64`, `:ulong` or `:size` above the
  largest signed integer keeps its bit pattern and reads as negative.
- `:float` and `:double` return a float.
- `:bool` and `:bool32` return `true` or `false`.
- `:void` returns nil.
- `:ptr` returns a pointer, or nil for NULL.
- `:string` returns the pointer, not a copy. Read it with `ffi/string`.
- A struct returns an array of its fields. An array returns an array of its
  elements, or `bytes` for a one-byte integer element type.


## Memory management

C memory is managed by hand:

```lisp
(def ptr (ffi/malloc 100))       # allocate 100 bytes
(ffi/write ptr :i32 42)          # write an i32
(ffi/read ptr :i32)              # => 42
(ffi/free ptr)                   # free the memory
```

| Primitive | Signature | Purpose |
|-----------|-----------|---------|
| `ffi/malloc` | `(size) → ptr` | Allocate C memory with libc `malloc` |
| `ffi/free` | `(ptr) → nil` | Free C memory with libc `free`. Nil is a no-op |
| `ffi/read` | `(ptr type) → value` | Read a typed value from C memory |
| `ffi/write` | `(ptr type value) → nil` | Write a typed value to C memory |
| `ffi/string` | `(ptr [max-len]) → string\|nil` | Read a NUL-terminated C string. Nil gives nil |

A pointer from `ffi/malloc` knows when it is freed. Reading, writing or
freeing it again is an error, and nothing frees it for you.

`ffi/write` checks its value as `ffi/call` checks an argument. A value that
the type cannot hold is an `ffi-error`, and the write does not happen:

```lisp
(def cell (ffi/malloc 4))
(ffi/write cell :u32 7)
(def [fits? err] (protect (ffi/write cell :u8 256)))
(assert (not fits?) "256 does not fit in :u8")
(assert (= (get err :error) :ffi-error) "a value out of range is an ffi-error")
(assert (= (ffi/read cell :u32) 7) "the failed write leaves the memory alone")
(def [int? _] (protect (ffi/write cell :i32 "seven")))
(assert (not int?) "a string is not an :i32")
(ffi/free cell)
```

`ffi/string` reads a NUL-terminated UTF-8 string from a pointer. With a second
argument, it reads at most that many bytes, and stops at the first NUL byte in
that range. Data that is not UTF-8 is an error.

```lisp
(def str-ptr (ffi/malloc 16))
(ffi/write str-ptr :u8 104)   # 'h'
(ffi/write (ptr/add str-ptr 1) :u8 105) # 'i'
(ffi/write (ptr/add str-ptr 2) :u8 0)   # null terminator
(ffi/string str-ptr)           # => "hi"
(ffi/string str-ptr 1)         # => "h"
(ffi/free str-ptr)
```


## Callbacks

`ffi/callback` wraps an Elle closure as a C function pointer, for a C API that
takes one: a `qsort` comparator, an iteration callback, and the like.

```lisp
(def cmp-sig (ffi/signature :int [:ptr :ptr]))
(def cmp-fn (fn (a b)
  (let [va (ffi/read a :int)
        vb (ffi/read b :int)]
    (- va vb))))

(def cb-ptr (ffi/callback cmp-sig cmp-fn))
# cb-ptr is now a C function pointer that can be passed to qsort

# When done:
(ffi/callback-free cb-ptr)
```

The closure's arity must admit the signature's argument count. A callback
converts each C argument as `ffi/call` converts a return value.

A callback has these limits:

- **One VM**: Only the VM that made a callback can run it, so C code must call
  it on the thread that drives that VM.
- **Errors**: When the closure signals an error, C receives a zero return
  value. `ffi/call` raises the error after the C function returns.
- **No variadic signature**: `ffi/callback` rejects a variadic signature,
  because a libffi closure cannot take one.
- **No yield**: A yield or any other signal from the closure becomes an error.
- **Manual lifetime**: Free a callback with `ffi/callback-free` when C no
  longer holds it.


## The `ffi/defbind` macro

`ffi/defbind` is a prelude macro that binds a C function by name. It looks up
the symbol, makes the signature, and defines a wrapper function.

```lisp
# Usage: (ffi/defbind name lib "c-name" return-type [arg-types...])

(ffi/defbind abs libc "abs" :int [:int])
(ffi/defbind my-sqrt libc "sqrt" :double [:double])
(ffi/defbind strlen libc "strlen" :size [:string])

(abs -42)         # => 42
(my-sqrt 2.0)     # => 1.4142135623730951
(strlen "hello")  # => 5
```

`(ffi/defbind abs libc "abs" :int [:int])` expands to this, with a generated
name for each parameter:

```lisp
(def abs
  (let [ptr__ (ffi/lookup libc "abs")
        sig__ (ffi/signature :int [:int])]
    (fn (a0) (ffi/call ptr__ sig__ a0))))
```

The lookup and the signature happen once, when the binding is defined. The
wrapper captures both, so a call pays only for marshalling and the libffi
dispatch.


## Pointers and NULL

A C pointer is an immediate value that holds the raw address. A C NULL is nil:

- Nil passes NULL for a `:ptr` or `:string` argument.
- A NULL `:ptr` return value is nil.
- `ffi/free` on nil is a no-op, as C `free(NULL)` is.
- `ffi/string` on nil returns nil.
- `ffi/read` and `ffi/write` on nil are errors.

`ptr/add`, `ptr/diff`, `ptr/to-int` and `ptr/from-int` do pointer arithmetic
and conversion. `(ptr/from-int 0)` is nil.


## Errors

Each FFI primitive signals its errors as error structs. The kinds:

| Error kind | Cause |
|------------|-------|
| `arity-error` | A primitive got the wrong number of arguments, or a callback closure cannot take the signature's arguments |
| `type-error` | An argument has the wrong type, for example an integer where a pointer belongs, or `ffi/call` got nil as the function |
| `argument-error` | `ffi/read` or `ffi/write` got nil, `ffi/malloc` got a size that is not positive, `ffi/struct` or `ffi/array` got no fields, a void field or a count that is not positive, or the fixed-argument count of `ffi/signature` is out of range |
| `use-after-free` | A primitive got a pointer that `ffi/free` already freed, as the memory to read, write or offset |
| `double-free` | `ffi/free` got a pointer that it already freed |
| `ffi-error` | A type keyword is unknown, a library or symbol does not load, `ffi/malloc` failed, `ffi/call` or `ffi/write` cannot convert a value to its type, `ffi/call` got the wrong argument count, `ffi/read` or `ffi/write` got `:void`, `ffi/write` got `:string`, `ffi/string` read data that is not UTF-8, `ffi/callback` got a variadic signature, or `ffi/callback-free` found no callback |


## Signals and capabilities

`ffi/signature`, `ffi/struct`, `ffi/array`, `ffi/size` and `ffi/align` signal
only `:error`. Every other `ffi/` primitive also carries `:ffi`, so the
analyzer knows that a function calls foreign code or touches C memory.
`ffi/run-teardowns` is one of them, because it calls each declared teardown's
C symbol. A parent fiber withholds the `:ffi` capability with `:deny |:ffi|`,
and a denied primitive does not run
([capabilities](signals/capabilities.md)).

```lisp
(let [f (fiber/new (fn [] (ffi/run-teardowns)) |:ffi :error| :deny |:ffi|)]
  (fiber/resume f)
  (assert (= (get (fiber/value f) :error) :capability-denied)
          "a fiber denied :ffi cannot run the teardowns"))
```
