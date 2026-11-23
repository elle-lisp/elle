# Embedding

<!-- audited: 2026-11-23 -->

Two host programs, one in Rust and one in C, that run Elle as a scripting
engine and walk the lifecycle a host owes.

[The embedding guide](../../docs/embedding.md) numbers the steps both hosts
walk. `make embedding` builds both hosts and runs them.

## The Rust host

[main.rs](src/main.rs) uses the `elle` crate directly. It builds a `Runtime`,
registers the primitive `host/add-ten` through `register_host_binding`, and
compiles and runs [hello.lisp](hello.lisp). Then it gives the result's owning
reference back with `release_program_value` and tears the runtime down.

The host runs the teardown sweep itself and asserts that no region survives
it. A host that keeps a reference therefore fails the demo, rather than
printing a number nobody checks.

## The C host

[host.c](host.c) links against `libelle_embed.so`, the cdylib that
[lib.rs](src/lib.rs) builds, through the header [elle.h](include/elle.h). It
calls `elle_init`, `elle_eval`, `elle_result_int` and `elle_destroy`.

The cdylib wraps a `Runtime` in an opaque `ElleCtx`. It holds the owning
reference of the last result for the C host, so a result stays readable until
the next `elle_eval` or `elle_destroy`, and each of those gives the displaced
result back.

The cdylib also exports `elle_register_prim`, which the C host does not call.
It registers a C function as a primitive through the plugin dispatch table,
under `PLUGIN_SENTINEL`, so the function takes the plugin calling convention.
