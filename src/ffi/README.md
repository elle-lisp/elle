# FFI: Foreign Function Interface

The FFI subsystem enables Elle code to call C functions from shared libraries. It uses `libloading` for dynamic library loading and `bindgen`-generated bindings for type-safe C interop.

## How FFI Works

1. **Library loading**: `(import-file "path/to/lib.so")` loads a shared library
2. **Symbol lookup**: FFI primitives find C functions by name
3. **Type marshalling**: Elle `Value` is converted to C types and back
4. **Execution**: The C function is called with marshalled arguments
5. **Result conversion**: C return values become Elle `Value`

## FFI Primitives

FFI primitives are defined in [`src/ffi/primitives/`](primitives/) and provide:

- `ffi/call` — Call a C function by name with arguments
- `ffi/library` — Get or load a library
- `ffi/symbol` — Look up a symbol in a library
- Type conversion functions for marshalling

## Type Support

FFI supports marshalling between Elle and C for:

- **Integers**: `i32`, `i64`, `u32`, `u64`
- **Floats**: `f32`, `f64`
- **Strings**: UTF-8 strings (null-terminated for C)
- **Pointers**: Opaque pointers to C objects
- **Structs**: Via `External` wrapper (plugin-provided types)

## Example

```janet
(import-file "libm.so")
(ffi/call "sin" 1.57)  ; Call C's sin() function
```

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`primitives/`](primitives/) - FFI primitive implementations
- [`src/plugin.rs`](../plugin.rs) - plugin loading (similar mechanism)
