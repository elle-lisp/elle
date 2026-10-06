# FFI: Foreign Function Interface

<!-- audited: 2026-10-06 -->

Calls C functions in shared libraries from Elle code: libloading opens a library, and libffi makes the call.

## How a call works

1. **Library loading**: `(ffi/native "libm.so.6")` opens a shared library, and
   `(ffi/native nil)` opens the running process.
2. **Symbol lookup**: `ffi/lookup` finds a C function in a library by name.
3. **Signature**: `ffi/signature` states the return type and the argument
   types.
4. **Call**: `ffi/call` converts each Elle value to its C type, calls the
   function, and converts the result back.

`ffi/native` opens a plain C library. An Elle plugin is a library that exports
`elle_plugin_init`, and it loads through `import-file` or
`import/load-plugin` instead ([modules](../../docs/modules.md)).

## See also

- [docs/ffi.md](../../docs/ffi.md): the architecture reference, with examples
  that run
- [AGENTS.md](AGENTS.md): the module's types and invariants
- [loading.rs](../primitives/loading.rs) and
  [memory.rs](../primitives/memory.rs): the `ffi/*` primitives
- [plugin.rs](../plugin.rs): plugin loading
