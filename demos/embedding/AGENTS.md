# embedding

<!-- audited: 2026-09-20 -->

Demonstrates embedding Elle as a scripting engine in host programs.

## Files

| File | Purpose |
|------|---------|
| `src/lib.rs` | C-ABI embedding surface (cdylib) |
| `src/main.rs` | Rust host demo — idiomatic Rust embedding |
| `include/elle.h` | C header for the cdylib |
| `host.c` | C host demo — same lifecycle via C ABI |
| `hello.lisp` | Elle script evaluated by the Rust host |
| `Makefile` | Builds the C host against libelle_embed.so |

## Architecture

Two embedding paths, both walking the sequence
[the embedding guide](../../docs/embedding.md) numbers:

1. **Rust (idiomatic)** — `src/main.rs` uses the `elle` crate directly:
   Runtime::new → register_repl_binding → compile_file → execute_scheduled →
   release_program_value → teardown

2. **C (via cdylib)** — `host.c` links against `libelle_embed.so`:
   elle_init → elle_eval → elle_result_int → elle_destroy

The cdylib (`src/lib.rs`) wraps the Rust API in `extern "C"` functions with
an opaque `ElleCtx` pointer. It holds the program value's owning reference for
the C host, whose result stays readable until the next `elle_eval` or
`elle_destroy`; each of those gives the displaced result back.

The Rust host runs the teardown sweep itself and reads the census it answers,
so a host that keeps a reference fails the demo rather than printing a number
nobody checks.

## Custom primitives

The Rust host registers `host/add-ten` using `PrimitiveDef` +
`register_repl_binding`. The C host uses `elle_register_prim` which
routes through the plugin dispatch table (PLUGIN_SENTINEL mechanism).

## Building

```bash
cargo build -p elle-embed          # builds cdylib + Rust host
cargo run -p elle-embed --bin host  # runs Rust host
make -C demos/embedding chost       # builds C host
demos/embedding/chost               # runs C host
```
