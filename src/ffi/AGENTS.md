# ffi

<!-- audited: 2026-10-13 -->

The foreign function interface: load C libraries, describe C types, marshal values, and call through libffi.

[docs/ffi.md](../../docs/ffi.md) is the user's reference: the type keywords,
the conversions, the errors. This file holds the internals.

## Responsibility

Enable Elle code to call C functions in shared libraries:
- Load shared libraries (or the current process)
- Look up symbols by name
- Describe C types and function signatures
- Marshal Elle values to/from C-typed data
- Dispatch calls through libffi
- Create callback trampolines (Elle closures as C function pointers)

Does NOT:
- Execute bytecode (that's `vm`)
- Define the `ffi/defbind` macro (that's [prelude.lisp](../prelude.lisp))
- Register primitives into the VM (that's
  [loading.rs](../primitives/loading.rs) and
  [memory.rs](../primitives/memory.rs), over the helpers in
  [primitives/ffi.rs](../primitives/ffi.rs))
- Own the `Value::pointer()` representation (that's `value/repr`)

The `ffi` cargo feature gates libffi: the call, callback and marshalling
modules. Type descriptors, library loading and the scalar writer build without
it, so `ffi/write` checks a scalar the same way in every build.

## Interface

| Type/Function | Module | Purpose |
|---------------|--------|---------|
| `FFISubsystem` | [mod.rs](mod.rs) | Manages loaded libraries and active callbacks |
| `TypeDesc` | [types.rs](types.rs) | Enum describing a C type for marshalling |
| `CBool` | [types.rs](types.rs) | The storage of a C `bool`, which `TypeDesc::Bool` reads and writes |
| `Signature` | [types.rs](types.rs) | Return type + arg types + variadic info |
| `StructDesc` | [types.rs](types.rs) | Positional struct descriptor with field layout |
| `MarshalledArg` | [to_c.rs](to_c.rs) | Holds C-typed storage for one FFI argument |
| `write_value_to_buffer` | [to_c/buffer.rs](to_c/buffer.rs) | Write a value into caller-provided C memory |
| `write_scalar` | [scalar.rs](scalar.rs) | Write a number, boolean or pointer into C memory, checked against its type |
| `read_value_from_buffer` | [from_c.rs](from_c.rs) | Read C memory into a value |
| `AlignedBuffer` | [marshal.rs](marshal.rs) | Heap buffer with guaranteed alignment for struct data |
| `to_libffi_type` | [marshal.rs](marshal.rs) | The libffi type of a `TypeDesc` |
| `prepare_cif` | [call.rs](call.rs) | Build a `libffi::middle::Cif` from a `Signature` |
| `ffi_call` | [call.rs](call.rs) | Call a C function through libffi |
| `create_callback` | [callback.rs](callback.rs) | Wrap an Elle closure as a C function pointer |
| `free_callback` | [callback.rs](callback.rs) | Recover leaked callback data |
| `CallbackStore` | [callback.rs](callback.rs) | Storage for active callbacks, keyed by code pointer |
| `CallbackData` | [callback.rs](callback.rs) | Closure + signature captured by a trampoline |
| `ActiveCallback` | [callback.rs](callback.rs) | Keeps a libffi closure alive; holds code pointer |
| `load` / `load_self` | [registry.rs](registry.rs) | Load a `.so` (or self) into the process-global registry; never `dlclose`d |
| `symbol` | [registry.rs](registry.rs) | Resolve a symbol in a registered library |
| `register_teardown` / `run_teardowns` | [registry.rs](registry.rs) | Attach / run a library's explicit ordered teardown |

## Data flow

```
Elle code
  │
  ├─ ffi/native "libm.so.6"  →  registry::load()  →  process-global mapping; FFISubsystem maps id→key
  ├─ ffi/lookup lib "sqrt"    →  FFISubsystem::get_symbol(id,"sqrt")  →  registry::symbol()  →  Value::pointer(addr)
  ├─ ffi/signature :double [:double]  →  Signature  →  ctx.ffi_signature()
  │                                                      └─ HeapObject::FFISignature(sig, RefCell<Option<Cif>>)
  └─ ffi/call ptr sig 2.0     →  prim_ffi_call
       │
       ├─ Value.get_or_prepare_cif()  →  lazily prepares and caches Cif
       ├─ ffi_call(fn_ptr, args, sig, cif)
       │    ├─ MarshalledArg::new(value, type_desc) per arg  →  C-typed storage
       │    ├─ build libffi::middle::Arg refs
       │    ├─ cif.call(code_ptr, args)  →  C function executes
       │    └─ convert return value  →  Elle Value
       └─ check take_callback_error()  →  propagate callback errors
```

### Callback flow

```
Elle code
  │
  ├─ ffi/callback sig closure  →  create_callback()
  │    ├─ prepare_cif(sig)
  │    ├─ Box::into_raw(CallbackData { closure, closure_value, signature, vm })
  │    ├─ libffi::middle::Closure::new(cif, trampoline_callback, userdata)
  │    └─ ActiveCallback { code_ptr }  →  stored in CallbackStore
  │
  └─ C code calls the code_ptr  →  trampoline_callback()
       ├─ read C args via read_value_from_buffer(), each in a region of its own
       ├─ use the VM captured at create_callback (userdata.vm)
       ├─ build closure env, execute bytecode
       ├─ write return value to libffi result buffer
       └─ on error: zero result + vm.ffi_mut().set_callback_error()
```

## Key types

### TypeDesc

Enum describing a C type. Variants: `Void`, `Bool`, `Bool32`, `I8`..`U64`,
`Float`, `Double`, `Int`, `UInt`, `Long`, `ULong`, `Char`, `UChar`, `Short`,
`UShort`, `Size`, `SSize`, `Ptr`, `Str`, `Struct(StructDesc)`,
`Array(Box<TypeDesc>, usize)`.

- `from_keyword(name)` — parse from Elle keyword (`:i32` → `TypeDesc::I32`,
  `:string` → `TypeDesc::Str`)
- `size()` → `Option<usize>` — byte size on current platform (`None` for Void)
- `align()` → `Option<usize>` — alignment on current platform
- `short_name()` → display string

`Bool` is the C99 `bool`, held as a `CBool` (`u8`). A read is never a Rust
`bool`: a byte that is neither 0 nor 1 would make that read undefined
behavior, so every read compares the byte with zero. `Bool32` is a 32-bit
integer used as a boolean, stored as `i32`.

### StructDesc

Positional struct descriptor. Fields are unnamed and ordered.

- `field_offsets()` → `Option<(Vec<usize>, usize)>` — byte offset of each
  field plus total size with tail padding. Each field starts at the next
  offset aligned for it, and the total rounds up to the largest field
  alignment. A nested struct aligns to its own largest field.

`ffi_call` marshals a struct argument into an `AlignedBuffer` with
`write_value_to_buffer` at each offset, and reads a struct return value back
with `read_value_from_buffer`.

### Signature

Reified function signature: `convention`, `ret: TypeDesc`, `args: Vec<TypeDesc>`,
`fixed_args: Option<usize>`. The `fixed_args` field supports variadic functions
(e.g., `printf`): `Some(n)` means the first `n` args are fixed, the rest are
variadic. `None` means non-variadic.

### MarshalledArg

Holds C-typed storage for one FFI argument. Created from `(Value, TypeDesc)`.
The internal `ArgStorage` enum covers all primitive types plus `Str` (owns a
`CString`) and `Struct` (owns an `AlignedBuffer` + nested `MarshalledArg`s).
`as_arg()` returns a `libffi::middle::Arg` referencing the storage.

### AlignedBuffer

Heap-allocated zero-initialized buffer with guaranteed alignment. Used for
struct/array data that libffi reads from (arguments) or writes into (returns).
Allocated via `std::alloc::alloc_zeroed`, freed on drop.

### CallbackData / ActiveCallback / CallbackStore

`CallbackData` captures the Elle closure, the closure value, the signature and
the creating VM. It is leaked with `Box::into_raw`, so the libffi closure can
reference it with `'static` lifetime. `ActiveCallback` holds the libffi closure
(owns the trampoline code page), the leaked userdata pointer, and the callable
code pointer address. `CallbackStore` is a `HashMap<usize, ActiveCallback>`
keyed by code pointer, stored in `FFISubsystem`.

### FFISubsystem

Per-VM FFI state. Holds `libraries: HashMap<u32, PathBuf>` (per-VM id → the
process-global registry key) and `callbacks: CallbackStore`. Owns **no**
`libloading::Library` — the mappings live in [registry.rs](registry.rs) for the process lifetime —
so dropping a worker's `FFISubsystem` `dlclose`s nothing. Methods: `load_library`,
`load_self`, `get_symbol`, `register_teardown`, `callbacks_mut`,
`set_callback_error`, `take_callback_error`.

## Invariants

1. **MarshalledArg must outlive its Arg.** The `as_arg()` method returns a
   reference into the `MarshalledArg`'s storage. If the `MarshalledArg` is
   dropped, the `Arg` becomes a dangling pointer. In `ffi_call`, the
   `marshalled` vec lives for the duration of the call.

2. **CStrings must outlive their pointers.** For `:string` arguments, the
   `CString` is owned by `ArgStorage::Str`. For struct fields containing
   strings, the `CString` is owned by a `MarshalledArg` in the struct's
   `owned` vec. Dropping these before the C call is UB.

3. **Callbacks run under the VM that created them.** The trampoline uses the
   `*mut VM` captured in `CallbackData` at `create_callback`. The callback is
   owned by that VM's FFI subsystem, so it can only be invoked while that VM is
   the active driver (the C call that re-enters the trampoline runs under it).

4. **Callback errors stash on the VM's FFI subsystem.** When an Elle closure
   signals an error during a callback, the trampoline writes zeros to the result
   buffer and calls `vm.ffi_mut().set_callback_error(err)`. `prim_ffi_call`
   drains it via `take_callback_error()` after the C function returns and
   propagates it.

5. **Variadic callbacks are rejected.** `create_callback` returns an error
   if `signature.fixed_args.is_some()`. libffi closures don't support
   variadic calling conventions.

6. **CIF caching is per signature value.** `HeapObject::FFISignature` holds a
   `RefCell<Option<Cif>>` that `Value::get_or_prepare_cif()` fills on first
   use, and every later `ffi/call` with that value reuses it. Two
   `ffi/signature` calls with the same types make two values with two caches.

7. **NULL is nil.** `Value::pointer(0)` is `Value::NIL`, so a NULL return
   value reads as nil, and nil marshals to NULL for `:ptr` and `:string`.

8. **Struct/array values are Elle arrays.** When marshalling compound types,
   Elle arrays map to C structs (positional fields) and C arrays (uniform
   elements). Field count must match exactly.

9. **Platform-guarded loading.** [registry.rs](registry.rs) uses `#[cfg(unix)]`
   guards. Non-unix platforms get stub implementations that return errors.

10. **Library mappings are process-global and never `dlclose`d**
    ([registry.rs](registry.rs), matching [plugin.rs](../plugin.rs)). A worker
    that loads an FFI library and exits never unmaps
    it, so a thread-local destructor the library registered (e.g. libgit2 via OpenSSL)
    always runs against still-mapped code — the worker-teardown SIGSEGV in
    `__nptl_deallocate_tsd` is impossible by construction. Teardown (`ffi/on-unload` +
    `ffi/run-teardowns`) is explicit-only, optional graceful cleanup; the runtime
    never runs it and never unmaps.

## Dependents

- [loading.rs](../primitives/loading.rs), [memory.rs](../primitives/memory.rs)
  and [ffiops.rs](../primitives/memory/ffiops.rs) — the FFI primitives call
  into this module
- [accessors.rs](../value/repr/accessors.rs) — `get_or_prepare_cif()`
- [conversions.rs](../value/repr/accessors/conversions.rs) —
  `as_ffi_signature()`, `as_ffi_type()`, `as_lib_handle()`
- [build.rs](../primitives/ctx/build.rs) — `ctx.ffi_signature()`,
  `ctx.ffi_type()`, `ctx.lib_handle()`
- [heap.rs](../value/heap.rs) — `HeapObject::FFISignature`,
  `HeapObject::FFIType`, `HeapObject::LibHandle`
- [display.rs](../value/display.rs) — prints an `ffi-type` by
  `TypeDesc::short_name`
- [send/mod.rs](../value/send/mod.rs) — carries a `TypeDesc` across threads
- [core.rs](../vm/core.rs) — each VM owns one `FFISubsystem`
- [prelude.lisp](../prelude.lisp) — `ffi/defbind` macro generates calls to `ffi/lookup`,
  `ffi/signature`, `ffi/call`
- [constructors.rs](../signals/constructors.rs) — `Signal::ffi()`,
  `Signal::ffi_errors()` use `SIG_FFI`
