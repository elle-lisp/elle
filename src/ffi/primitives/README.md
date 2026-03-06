# FFI Primitives

Primitives that expose the FFI subsystem to Elle code. These functions handle dynamic library loading, symbol lookup, and type marshalling between Elle and C.

## Core Primitives

| Primitive | Purpose |
|-----------|---------|
| `ffi/library` | Load or get a shared library |
| `ffi/symbol` | Look up a symbol in a library |
| `ffi/call` | Call a C function with arguments |
| `ffi/type` | Get or create a C type descriptor |

## Type Marshalling

FFI primitives handle conversion between Elle `Value` and C types:

- **Integers**: `i32`, `i64`, `u32`, `u64` ↔ Elle integers
- **Floats**: `f32`, `f64` ↔ Elle floats
- **Strings**: UTF-8 strings ↔ C `char*` (null-terminated)
- **Pointers**: Opaque pointers ↔ Elle `External` values
- **Structs**: Plugin-provided types via `External` wrapper

## Error Handling

FFI primitives return errors for:

- **Library not found**: `(ffi/library "nonexistent.so")` → error
- **Symbol not found**: `(ffi/symbol lib "missing_func")` → error
- **Type mismatch**: Passing wrong type to C function → error
- **Null pointer dereference**: Dereferencing null → error

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/ffi/`](../) - FFI subsystem overview
- [`src/plugin.rs`](../../plugin.rs) - plugin loading (uses similar mechanisms)
