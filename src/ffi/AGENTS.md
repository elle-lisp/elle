# ffi

Foreign Function Interface. Call C libraries from Elle.

## Responsibility

- Load shared libraries (.so, .dylib, .dll)
- Resolve and cache symbols
- Marshal values between Elle and C
- Manage struct/union layouts
- Handle callbacks (Elle functions callable from C)

Does NOT:
- Parse C headers automatically (use bindgen externally)
- Manage memory automatically (caller responsibility)
- Provide safety guarantees (inherently unsafe)

## Interface

| Type | Purpose |
|------|---------|
| `FFISubsystem` | Main coordinator, held by VM |
| `LibraryHandle` | Loaded library reference |
| `CType` | C type representation |
| `CValue` | Marshalled C value |
| `StructLayout` | C struct field layout |
| `UnionLayout` | C union variant layout |
| `SymbolResolver` | Symbol lookup with caching |
| `HandlerRegistry` | Custom type handlers |

## Data flow

```
Elle code
    │
    ├─► load-library "/path/to/lib.so"
    │       └─► FFISubsystem::load_library()
    │               └─► LibraryHandle (id=1)
    │
    ├─► ffi-call lib-id "function_name" args...
    │       ├─► resolve symbol
    │       ├─► marshal Elle → C
    │       ├─► call native function
    │       └─► marshal C → Elle
    │
    └─► Result
```

## Dependents

- `vm/core.rs` - holds `FFISubsystem`
- `primitives/` - FFI primitives use this module

## Invariants

1. **Library IDs are stable.** Once assigned, a library ID doesn't change
   until unloaded.

2. **Marshaling must be reversible.** `c_to_elle(elle_to_c(v))` should
   round-trip for supported types.

3. **Struct layouts are explicit.** Alignment and padding must be specified;
   we don't infer from C headers.

4. **Callbacks require careful lifetime management.** The Elle closure must
   outlive any C code that might call it.

## C Types

```rust
pub enum CType {
    Void,
    Int, UInt,
    Int8, UInt8, Int16, UInt16,
    Int32, UInt32, Int64, UInt64,
    Float, Double,
    Pointer(Box<CType>),
    Struct(StructId),
    Union(UnionId),
    Array(Box<CType>, usize),
    Function { args: Vec<CType>, ret: Box<CType> },
}
```

## Submodules

| Module | Purpose |
|--------|---------|
| `loader.rs` | Library loading via libloading |
| `symbol.rs` | Symbol resolution and caching |
| `marshal/` | Elle ↔ C value conversion |
| `types.rs` | CType, StructLayout, UnionLayout |
| `call.rs` | Foreign function invocation |
| `callback.rs` | Elle→C callback creation |
| `handlers.rs` | Custom type handler registry |
| `memory.rs` | Memory allocation helpers |
| `safety.rs` | Safety checks and validation |
| `primitives/` | Elle-exposed FFI functions |

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 310 | `FFISubsystem` |
| `types.rs` | ~200 | Type definitions |
| `loader.rs` | ~100 | Library loading |
| `marshal/` | ~300 | Marshaling |
| `call.rs` | ~150 | Function calls |
| `callback.rs` | ~150 | Callback support |
