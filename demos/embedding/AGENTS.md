# embedding

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

Two embedding paths:

1. **Rust (idiomatic)** — `src/main.rs` uses `elle` crate directly:
   VM::new → register_primitives → init_stdlib → compile_file → execute_scheduled

2. **C (via cdylib)** — `host.c` links against `libelle_embed.so`:
   elle_init → elle_eval → elle_result_int → elle_destroy

The cdylib (`src/lib.rs`) wraps the Rust API in `extern "C"` functions with
an opaque `ElleCtx` pointer.

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
