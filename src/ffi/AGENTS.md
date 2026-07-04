# ffi

Foreign function interface. Calls C functions from Elle via libffi.

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
- Define the `ffi/defbind` macro (that's `prelude.lisp`)
- Register primitives into the VM (that's `primitives/ffi.rs`)
- Own the `Value::pointer()` representation (that's `value/repr`)

## Interface

| Type/Function | Module | Purpose |
|---------------|--------|---------|
| `FFISubsystem` | `mod.rs` | Manages loaded libraries and active callbacks |
| `TypeDesc` | `types.rs` | Enum describing a C type for marshalling |
| `Signature` | `types.rs` | Return type + arg types + variadic info |
| `StructDesc` | `types.rs` | Positional struct descriptor with field layout |
| `MarshalledArg` | `marshal.rs` | Holds C-typed storage for one FFI argument |
| `AlignedBuffer` | `marshal.rs` | Heap buffer with guaranteed alignment for struct data |
| `prepare_cif` | `call.rs` | Build a `libffi::middle::Cif` from a `Signature` |
| `ffi_call` | `call.rs` | Call a C function through libffi |
| `create_callback` | `callback.rs` | Wrap an Elle closure as a C function pointer |
| `free_callback` | `callback.rs` | Recover leaked callback data |
| `CallbackStore` | `callback.rs` | Storage for active callbacks, keyed by code pointer |
| `CallbackData` | `callback.rs` | Closure + signature captured by a trampoline |
| `ActiveCallback` | `callback.rs` | Keeps a libffi closure alive; holds code pointer |
| `load` / `load_self` | `registry.rs` | Load a `.so` (or self) into the process-global registry; never `dlclose`d |
| `symbol` | `registry.rs` | Resolve a symbol in a registered library |
| `register_teardown` / `run_teardowns` | `registry.rs` | Attach / run a library's explicit ordered teardown |

## Data flow

```
Elle code
  │
  ├─ ffi/native "libm.so.6"  →  registry::load()  →  process-global mapping; FFISubsystem maps id→key
  ├─ ffi/lookup lib "sqrt"    →  FFISubsystem::get_symbol(id,"sqrt")  →  registry::symbol()  →  Value::pointer(addr)
  ├─ ffi/signature :double [:double]  →  Signature  →  Value::ffi_signature()
  │                                                      └─ HeapObject::FFISignature(sig, RefCell<Option<Cif>>)
  └─ ffi/call ptr sig 2.0     →  prim_ffi_call
       │
       ├─ Value.get_or_prepare_cif()  →  lazily prepares and caches Cif
       ├─ MarshalledArg::new(value, type_desc)  →  C-typed storage
       ├─ ffi_call(fn_ptr, args, sig, cif)
       │    ├─ marshal each arg  →  Vec<MarshalledArg>
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
  │    ├─ Box::leak(CallbackData { closure, sig })
  │    ├─ libffi::middle::Closure::new(cif, trampoline_callback, userdata)
  │    └─ ActiveCallback { code_ptr }  →  stored in CallbackStore
  │
  └─ C code calls the code_ptr  →  trampoline_callback()
       ├─ read C args via read_value_from_buffer()
       ├─ use the VM captured at create_callback (userdata.vm)
       ├─ build closure env, execute bytecode
       ├─ write return value to libffi result buffer
       └─ on error: zero result + vm.ffi_mut().set_callback_error()
```

## Submodules

The Elle-facing primitives live in `src/primitives/ffi.rs`, not
in this module. This module provides the Rust implementation that those
primitives call.

## Key types

### TypeDesc

Enum describing a C type. Variants: `Void`, `Bool`, `I8`..`U64`, `Float`,
`Double`, `Int`, `UInt`, `Long`, `ULong`, `Char`, `UChar`, `Short`, `UShort`,
`Size`, `SSize`, `Ptr`, `Str`, `Struct(StructDesc)`, `Array(Box<TypeDesc>, usize)`.

- `from_keyword(name)` — parse from Elle keyword (`:i32` → `TypeDesc::I32`,
  `:string` → `TypeDesc::Str`)
- `size()` → `Option<usize>` — byte size on current platform (`None` for Void)
- `align()` → `Option<usize>` — alignment on current platform
- `short_name()` → display string

### StructDesc

Positional struct descriptor. Fields are unnamed and ordered.

- `field_offsets()` → `Option<(Vec<usize>, usize)>` — byte offset of each
  field plus total size with tail padding. Computes C-compatible layout
  (alignment padding between fields, tail padding to struct alignment).

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

`CallbackData` captures the Elle closure and signature for a callback
trampoline. It is `Box::leak`'d so the libffi closure can reference it with
`'static` lifetime. `ActiveCallback` holds the libffi closure (owns the
trampoline code page), the leaked userdata pointer, and the callable code
pointer address. `CallbackStore` is a `HashMap<usize, ActiveCallback>` keyed
by code pointer, stored in `FFISubsystem`.

### FFISubsystem

Per-VM FFI state. Holds `libraries: HashMap<u32, PathBuf>` (per-VM id → the
process-global registry key) and `callbacks: CallbackStore`. Owns **no**
`libloading::Library` — the mappings live in `registry.rs` for the process lifetime —
so dropping a worker's `FFISubsystem` `dlclose`s nothing. Methods: `load_library`,
`load_self`, `get_symbol`, `register_teardown`, `callbacks_mut`.

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

6. **CIF caching is on HeapObject::FFISignature.** The `RefCell<Option<Cif>>`
   is lazily populated by `Value::get_or_prepare_cif()`. Once prepared, the
   CIF is reused for all calls with that signature. This avoids re-preparing
   the CIF on every `ffi/call`.

7. **NULL pointers map to nil.** `Value::pointer(0)` is a valid C null
   pointer. In the `:ptr` marshalling path, `Value::NIL` marshals to
   `NULL` and `NULL` return values become `Value::pointer(0)`.

8. **Struct/array values are Elle arrays.** When marshalling compound types,
   Elle arrays map to C structs (positional fields) and C arrays (uniform
   elements). Field count must match exactly.

9. **Platform-guarded loading.** `registry.rs` uses `#[cfg(unix)]` guards. Non-unix
   platforms get stub implementations that return errors.

10. **Library mappings are process-global and never `dlclose`d** (`registry.rs`,
    matching `plugin.rs`). A worker that loads an FFI library and exits never unmaps
    it, so a thread-local destructor the library registered (e.g. libgit2 via OpenSSL)
    always runs against still-mapped code — the worker-teardown SIGSEGV in
    `__nptl_deallocate_tsd` is impossible by construction. Teardown (`ffi/on-unload` +
    `ffi/run-teardowns`) is explicit-only, optional graceful cleanup; the runtime
    never runs it and never unmaps.

## Dependents

- `primitives/ffi.rs` / `primitives/loading.rs` — the FFI primitives call into this module
- `value/repr/accessors.rs` — `get_or_prepare_cif()`, `as_ffi_signature()`,
  `as_ffi_type()`, `as_lib_handle()`
- `value/repr/constructors.rs` — `Value::ffi_signature()`, `Value::ffi_type()`,
  `Value::lib_handle()`
- `value/heap.rs` — `HeapObject::FFISignature`, `HeapObject::FFIType`,
  `HeapObject::LibHandle`
- `prelude.lisp` — `ffi/defbind` macro generates calls to `ffi/lookup`,
  `ffi/signature`, `ffi/call`
- `signals/mod.rs` — `Signal::ffi()`, `Signal::ffi_errors()` use `SIG_FFI`
