# FFI: Architecture Reference

Elle's FFI enables calling C functions from Elle code. The design is inspired
by Janet's FFI: keyword-based type descriptors, reified signatures, and
explicit marshalling. The backend uses the `libffi` crate (middle-level API)
for calling convention correctness across platforms.

This document describes the implemented system.


## Quick Start

```lisp
;# Load a library
(def libc (ffi/native nil))              # current process (dlopen(NULL))
(def libm (ffi/native "libm.so.6"))      # or a specific .so

;# Look up a symbol
(def sqrt-ptr (ffi/lookup libm "sqrt"))

;# Create a signature: return type, [arg types]
(def sqrt-sig (ffi/signature :double [:double]))

;# Call it
(ffi/call sqrt-ptr sqrt-sig 2.0)         # => 1.4142135623730951

;# Or use the convenience macro
(ffi/defbind sqrt libm "sqrt" :double [:double])
(sqrt 2.0)                               # => 1.4142135623730951
```


## Type Descriptors

C types are described by keywords at the Elle level. The `TypeDesc` enum in
`src/ffi/types.rs` maps each keyword to its C equivalent.

### Primitive types

| Keyword | C type | Size (bytes) | Notes |
|---------|--------|-------------|-------|
| `:void` | `void` | — | Return type only# not valid for arguments |
| `:bool` | `_Bool` (as `int`) | 4 | Truthy/falsy conversion |
| `:i8` | `int8_t` | 1 | |
| `:u8` | `uint8_t` | 1 | |
| `:i16` | `int16_t` | 2 | |
| `:u16` | `uint16_t` | 2 | |
| `:i32` | `int32_t` | 4 | |
| `:u32` | `uint32_t` | 4 | |
| `:i64` | `int64_t` | 8 | |
| `:u64` | `uint64_t` | 8 | |
| `:float` | `float` | 4 | Accepts int or float values |
| `:double` | `double` | 8 | Accepts int or float values |
| `:int` | `int` | platform | Typically 4 bytes |
| `:uint` | `unsigned int` | platform | |
| `:long` | `long` | platform | 8 bytes on LP64 |
| `:ulong` | `unsigned long` | platform | |
| `:char` | `char` | 1 | Signed on most platforms |
| `:uchar` | `unsigned char` | 1 | |
| `:short` | `short` | platform | Typically 2 bytes |
| `:ushort` | `unsigned short` | platform | |
| `:size` | `size_t` | platform | 8 bytes on 64-bit |
| `:ssize` | `ptrdiff_t` | platform | 8 bytes on 64-bit |
| `:ptr` | `void *` | platform | Maps to `Value::pointer()` or nil for NULL |
| `:string` | `const char *` | platform | Elle string copied to CString# interior nulls are an error |

### Type introspection

```lisp
(ffi/size :i32)     # => 4
(ffi/size :double)  # => 8
(ffi/size :void)    # => nil
(ffi/align :double) # => 8
(ffi/align :ptr)    # => 8  (on 64-bit)
```


## Compound Types

### Structs

`ffi/struct` creates a struct type descriptor from an array of field types.
Fields are positional (unnamed) and follow C struct layout rules: alignment
padding between fields, tail padding to the struct's alignment.

```lisp
;# struct { int32_t x# double y# }
(def point-type (ffi/struct [:i32 :double]))

(ffi/size point-type)   # => 16  (4 + 4 padding + 8)
(ffi/align point-type)  # => 8

;# Nested structs
(def inner (ffi/struct [:i8 :i32]))      # 8 bytes (1 + 3 padding + 4)
(def outer (ffi/struct [:i64 inner]))    # 16 bytes
```

Struct values are represented as Elle arrays. When marshalling to C, the
array elements are written into a properly aligned buffer at the computed
field offsets. When reading from C, the buffer is read back into an Elle
array.

```lisp
;# Write a struct to memory
(def buf (ffi/malloc (ffi/size point-type)))
(ffi/write buf point-type [42 1.5])

;# Read it back
(ffi/read buf point-type)  # => [42 1.5]
(ffi/free buf)
```

Constraints:
- Structs must have at least one field
- `:void` is not valid as a field type
- Array length must match field count exactly

### Arrays

`ffi/array` creates a fixed-size array type descriptor.

```lisp
;# int32_t[10]
(def arr-type (ffi/array :i32 10))

(ffi/size arr-type)   # => 40
(ffi/align arr-type)  # => 4
```

Array values are also represented as Elle arrays. Count must be positive
and must match exactly when marshalling.


## Signatures

A signature describes a C function's calling convention, return type, and
argument types. Created by `ffi/signature` and stored as a first-class
Elle value (`HeapObject::FFISignature`).

```lisp
;# Non-variadic: (return-type [arg-types...])
(def sig (ffi/signature :int [:int :int]))

;# Variadic: (return-type [all-arg-types...] fixed-count)
;# For printf(const char *, ...): 1 fixed arg, rest variadic
(def printf-sig (ffi/signature :int [:ptr :int] 1))
```

The third argument to `ffi/signature` is the number of fixed arguments for
variadic functions. It must be in the range `[0, len(arg-types)]`. When
omitted, the signature is non-variadic.

Signatures accept both keywords (`:i32`) and compound type values (from
`ffi/struct` or `ffi/array`) for argument and return types.


## Calling C Functions

`ffi/call` takes a function pointer, a signature, and the arguments:

```lisp
(ffi/call fn-ptr sig arg1 arg2 ...)
```

The number of arguments must match the signature's argument count exactly.

```lisp
(def libc (ffi/native nil))
(def abs-ptr (ffi/lookup libc "abs"))
(def abs-sig (ffi/signature :int [:int]))

(ffi/call abs-ptr abs-sig -42)  # => 42
```

### Argument marshalling

Each Elle value is converted to C-typed storage based on the corresponding
`TypeDesc`:

- **Integers**: Range-checked and narrowed (e.g., `Value::int(256)` as `:i8`
  is an error)
- **Floats**: `:float` and `:double` accept both int and float values
- **Booleans**: Truthy → 1, falsy → 0 (as `c_int`)
- **Pointers**: `Value::pointer(addr)` or `nil` (→ NULL)
- **Strings**: Copied to a `CString`# interior null bytes are an error
- **Structs/arrays**: Elle array → aligned buffer with field-by-field marshalling

### Return value conversion

C return values are converted back to Elle values:

- Integer types → `Value::int()`
- Float types → `Value::float()`
- `:bool` → `Value::bool()`
- `:void` → `Value::NIL`
- `:ptr` / `:string` → `Value::pointer(addr)`
- Struct/array → Elle array (read from aligned buffer)


## Memory Management

Manual memory management for C interop:

```lisp
(def ptr (ffi/malloc 100))       # allocate 100 bytes
(ffi/write ptr :i32 42)          # write an i32
(ffi/read ptr :i32)              # => 42
(ffi/free ptr)                   # free the memory
```

| Primitive | Signature | Purpose |
|-----------|-----------|---------|
| `ffi/malloc` | `(size) → ptr` | Allocate C memory (via libc `malloc`) |
| `ffi/free` | `(ptr) → nil` | Free C memory (via libc `free`)# nil is a no-op |
| `ffi/read` | `(ptr type) → value` | Read a typed value from C memory |
| `ffi/write` | `(ptr type value) → nil` | Write a typed value to C memory |
| `ffi/string` | `(ptr [max-len]) → string\|nil` | Read a null-terminated C string# nil ptr → nil |

`ffi/string` reads a null-terminated UTF-8 string from a pointer. With an
optional second argument, it reads at most that many bytes (stopping at the
first null byte within that range). Returns nil for null pointers. Signals
an error for non-UTF-8 data.

```lisp
(def ptr (ffi/malloc 16))
;# ... write "hello\0" to ptr ...
(ffi/string ptr)      # => "hello"
(ffi/string ptr 3)    # => "hel"
(ffi/free ptr)
```


## Callbacks

`ffi/callback` wraps an Elle closure as a C function pointer, enabling Elle
functions to be passed to C APIs that expect function pointer arguments
(e.g., `qsort` comparators, iteration callbacks).

```lisp
(def cmp-sig (ffi/signature :int [:ptr :ptr]))
(def cmp-fn (fn (a b)
  (let ((va (ffi/read a :int))
        (vb (ffi/read b :int)))
    (- va vb))))

(def cb-ptr (ffi/callback cmp-sig cmp-fn))
;# cb-ptr is now a C function pointer that can be passed to qsort

;# When done:
(ffi/callback-free cb-ptr)
```

### How it works

1. `create_callback` builds a libffi closure with a trampoline function
2. The Elle closure and signature are `Box::leak`'d as `CallbackData` so
   the trampoline can reference them with `'static` lifetime
3. When C code calls the function pointer, `trampoline_callback` fires:
   - Reads C arguments into Elle values via `read_value_from_buffer`
   - Gets the VM from thread-local storage (`get_vm_context`)
   - Builds a closure environment and executes the bytecode
   - Writes the return value back to the libffi result buffer
4. The `ActiveCallback` is stored in `FFISubsystem::callbacks` (keyed by
   code pointer address) to keep the libffi closure alive

### Arity validation

`ffi/callback` validates that the closure's arity matches the signature's
argument count. Exact arity must match# `AtLeast(n)` requires
`sig.args.len() >= n`# `Range(min, max)` requires the count to be in range.

### Limitations

- **Single-threaded**: Callbacks can only be invoked on the thread that
  created them (same VM context). The trampoline reads the VM from
  thread-local storage.
- **Error handling**: If the Elle closure signals an error, the trampoline
  writes zeros to the result buffer and sets a thread-local error flag.
  `ffi/call` checks `take_callback_error()` after the C function returns
  and propagates the error to the Elle caller.
- **No variadic callbacks**: `create_callback` rejects signatures with
  `fixed_args` set. libffi closures don't support variadic calling
  conventions.
- **No yield/suspend**: Yielding or signaling inside a callback is not
  supported. The trampoline treats unexpected signals as errors.
- **Manual lifetime**: Callbacks must be explicitly freed with
  `ffi/callback-free` when no longer needed. The leaked `CallbackData`
  is recovered and dropped at that point.


## The `ffi/defbind` Macro

`ffi/defbind` is a prelude macro that provides convenient FFI function
binding. It looks up the symbol, creates a signature, and defines a
wrapper function — all at definition time.

```lisp
;# Usage: (ffi/defbind name lib "c-name" return-type [arg-types...])

(def libc (ffi/native nil))
(ffi/defbind abs libc "abs" :int [:int])
(ffi/defbind sqrt libm "sqrt" :double [:double])
(ffi/defbind strlen libc "strlen" :size [:string])

(abs -42)       # => 42
(sqrt 2.0)      # => 1.4142135623730951
(strlen "hello") # => 5
```

### Expansion

`(ffi/defbind abs libc "abs" :int [:int])` expands to:

```lisp
(def abs
  (let ((ptr__ (ffi/lookup libc "abs"))
        (sig__ (ffi/signature :int [:int])))
    (fn (a0) (ffi/call ptr__ sig__ a0))))
```

The pointer lookup and signature creation happen once at definition time.
The generated function captures them in its closure environment, so each
call only pays for marshalling and the libffi dispatch.


## Value::pointer()

C pointers are represented as NaN-boxed values using tag `0x7FFE`:

```
CPointer: 0x7FFE_XXXX_XXXX_XXXX where X = 48-bit raw C pointer address
```

- `Value::pointer(addr)` — create from a `usize` address
- `value.as_pointer()` — extract as `Option<usize>`
- `value.is_nil()` — nil is accepted as NULL in pointer contexts

### NULL semantics

- Elle `nil` marshals to C `NULL` for `:ptr` arguments
- C `NULL` return values become `Value::pointer(0)` (not nil)
- `ffi/free` on nil is a no-op (matches C `free(NULL)` semantics)
- `ffi/string` on nil returns nil
- `ffi/read` on nil signals an error


## CIF Caching

A CIF (Call Interface) describes the calling convention for a specific
function signature. Preparing a CIF involves libffi setup work. To avoid
repeating this on every call, CIFs are cached on the signature value itself.

`HeapObject::FFISignature(Signature, RefCell<Option<Cif>>)` stores the
signature and an optional cached CIF. `Value::get_or_prepare_cif()` lazily
prepares the CIF on first access and returns a `Ref` to the cached value
on subsequent accesses.

This means:
- First `ffi/call` with a signature: prepare CIF + call
- Subsequent calls with the same signature value: reuse cached CIF
- Different signature values (even with identical types) have independent caches


## Error Handling

All FFI primitives return `(SignalBits, Value)`. Errors are signaled via
`SIG_ERROR` with an error tuple.

| Error kind | Raised by | Cause |
|------------|-----------|-------|
| `arity-error` | All primitives | Wrong number of arguments |
| `type-error` | All primitives | Wrong argument type (e.g., int where pointer expected) |
| `ffi-error` | `ffi/native` | Library not found or load failure |
| `ffi-error` | `ffi/lookup` | Symbol not found in library |
| `ffi-error` | `ffi/call` | Argument count mismatch, marshalling failure |
| `ffi-error` | `ffi/read` | Cannot read void# null pointer |
| `ffi-error` | `ffi/write` | Cannot write void# null pointer |
| `ffi-error` | `ffi/string` | Not valid UTF-8 |
| `ffi-error` | `ffi/callback` | Variadic signature# no VM context |
| `ffi-error` | `ffi/callback-free` | No callback at address |
| `argument-error` | `ffi/malloc` | Size not positive |
| `argument-error` | `ffi/struct` | Empty struct# void field |
| `argument-error` | `ffi/array` | Non-positive count# void element |
| `argument-error` | `ffi/signature` | `fixed_args` out of range |

Integer arguments are range-checked: passing 256 as `:i8` signals an
`ffi-type-error` from the marshalling layer.


## Effect System

FFI primitives carry the `Effect::ffi_raises()` effect, which is
`SIG_FFI | SIG_ERROR`. This means:

- The effect system knows these functions call foreign code (`SIG_FFI`)
- They may also raise errors (`SIG_ERROR`)
- Pure primitives like `ffi/signature`, `ffi/struct`, `ffi/array`,
  `ffi/size`, `ffi/align` carry `Effect::raises()` (just `SIG_ERROR`)

`SIG_FFI` is bit 4 (value 16) in the signal bitmask. It is used by the
effect system for compile-time tracking but is not a runtime signal — FFI
calls don't emit `SIG_FFI` at runtime.


## Struct Marshalling

When passing structs to C or reading them back, the marshalling layer
computes C-compatible field layout:

1. **Field offsets**: Each field is placed at the next address aligned to
   its alignment requirement. `StructDesc::field_offsets()` computes this.

2. **Tail padding**: The total struct size is rounded up to the struct's
   alignment (max alignment of any field).

3. **Nested structs**: Alignment of a struct is the max alignment of its
   fields. Nested structs are laid out recursively.

Example: `struct { int8_t a# int32_t b# }`
- Field `a` at offset 0 (size 1, align 1)
- 3 bytes padding
- Field `b` at offset 4 (size 4, align 4)
- Total size: 8 (no tail padding needed, already aligned to 4)

The `AlignedBuffer` type provides heap-allocated storage with the correct
alignment for the struct. For arguments, `write_value_to_buffer` writes
each field at its computed offset. For return values, `read_value_from_buffer`
reads each field back. String fields within structs require special handling:
the `CString` must outlive the buffer, so it's stored in a `MarshalledArg`
that's kept alive alongside the buffer.


## Invariants

1. **MarshalledArg outlives its Arg.** The libffi `Arg` references storage
   inside `MarshalledArg`. Dropping the `MarshalledArg` before the call
   completes is undefined behavior.

2. **Callbacks are single-threaded.** The trampoline accesses the VM via
   thread-local storage. Cross-thread callback invocation will fail.

3. **CIF caching is per-value, not per-type.** Two `ffi/signature` calls
   with identical types produce two independent signature values with
   independent CIF caches.

4. **Struct/array values are Elle arrays.** The marshalling layer expects
   Elle arrays with exactly the right number of elements. Mismatches are
   errors, not silent truncation or padding.

5. **Platform-guarded loading.** Library loading is Linux-only
   (`#[cfg(target_os = "linux")]`). Other platforms get error stubs.

6. **No automatic memory management.** `ffi/malloc` memory must be
   explicitly freed with `ffi/free`. Callbacks must be freed with
   `ffi/callback-free`. There is no GC integration for C memory.
