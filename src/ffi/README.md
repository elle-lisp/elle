# Foreign Function Interface (FFI)

The FFI module enables Elle code to call functions in C shared libraries.
This is how Elle interacts with system libraries, databases, GUI toolkits,
and other native code.

## Quick Example

```lisp
; Load the C standard library
(define libc (load-library "libc.so.6"))

; Call strlen
(ffi-call libc "strlen" :string "hello")  ; => 5
```

## Loading Libraries

```lisp
(define lib (load-library "/path/to/library.so"))
```

The returned handle is used for all subsequent calls to that library.

## Calling Functions

```lisp
(ffi-call library "function_name" return-type arg1 arg2 ...)
```

Types are specified as keywords: `:int`, `:double`, `:string`, `:pointer`.

## Type Marshaling

| Elle Type | C Type | Notes |
|-----------|--------|-------|
| `Int` | `int64_t` | Sign-extended for smaller types |
| `Float` | `double` | Converted to/from `float` as needed |
| `String` | `char*` | Null-terminated, copied |
| `Vector` | `T*` | Pointer to contiguous data |
| `Nil` | `NULL` | Null pointer |

## Structs

Define struct layouts for C interop:

```lisp
(define point-layout
  (ffi-struct "Point"
    (x :int 0)      ; field name, type, offset
    (y :int 4)))

(define p (ffi-alloc point-layout))
(ffi-set! p :x 10)
(ffi-set! p :y 20)
```

## Callbacks

Pass Elle functions to C:

```lisp
(define my-callback
  (ffi-callback (:int :int -> :int)
    (fn (a b) (+ a b))))

; Pass to C function expecting int (*)(int, int)
(ffi-call lib "register_callback" :void my-callback)
```

**Warning**: The callback must remain live for as long as C code might
call it. Store it in a global or ensure proper lifetime management.

## Safety

FFI is inherently unsafe. Common pitfalls:

- **Dangling pointers**: Don't pass Elle strings that might be GC'd
- **Type mismatches**: C won't check your types at runtime
- **Buffer overflows**: C doesn't know your array lengths
- **Thread safety**: Most C libraries aren't thread-safe

When in doubt, consult the C library's documentation.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `examples/ffi.lisp` - usage examples
- `src/primitives/` - FFI primitives exposed to Elle
