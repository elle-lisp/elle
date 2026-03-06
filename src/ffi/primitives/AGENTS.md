# ffi/primitives

FFI primitive function registration and context management.

## Responsibility

- Re-export context management functions from `crate::context`
- Provide registration stub for FFI primitives (actual primitives are in `src/primitives/ffi.rs`)

## Submodules

| Module | Purpose |
|--------|---------|
| `context.rs` | Re-exports from `crate::context`, registration stub |

## Interface

| Function | Purpose |
|----------|---------|
| `set_vm_context(vm)` | Set thread-local VM context |
| `get_vm_context()` | Get thread-local VM context |
| `clear_vm_context()` | Clear thread-local VM context |
| `set_symbol_table(symbols)` | Set thread-local symbol table context |
| `get_symbol_table()` | Get thread-local symbol table context |
| `clear_symbol_table()` | Clear thread-local symbol table context |
| `resolve_symbol_name(id)` | Resolve symbol ID to name |
| `register_ffi_primitives(vm)` | No-op (primitives registered via PRIMITIVES table) |

## Note

The actual FFI primitives (15 functions like `ffi/native`, `ffi/lookup`, `ffi/signature`, `ffi/call`, `ffi/callback`) are implemented in `src/primitives/ffi.rs`, not in this module. This module only provides context management and a registration stub.

## Dependents

- `src/primitives/ffi.rs` — FFI primitive implementations
- `src/ffi/` — FFI subsystem (uses context for VM access)
- `src/main.rs` — Sets context before executing code
- `src/repl.rs` — Sets context before executing code

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 7 | Re-exports |
| `context.rs` | 13 | Re-exports from `crate::context`, registration stub |
