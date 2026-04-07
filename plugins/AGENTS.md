# plugins

Dynamically-loaded Rust libraries that extend Elle with additional primitives.

## Responsibility

Provide optional functionality via `.so` cdylib crates that:
- Register primitives using the same `PrimitiveDef` mechanism as built-in primitives
- Work directly with `Value` — no C FFI marshalling
- Are loaded at runtime via `(import "path/to/plugin.so")`

## Plugin system

Plugins are compiled as Rust cdylib crates that export an `elle_plugin_init` function. The plugin loader:
1. Loads the `.so` file via `libloading`
2. Calls `elle_plugin_init` with a `PluginContext`
3. The plugin registers its primitives via `context.register(def)`
4. The loader installs all registered primitives into the VM

Most plugins use the `elle_plugin_init!` macro:
```rust
elle::elle_plugin_init!(PRIMITIVES, "mymod/");
```
This generates the `#[no_mangle] pub unsafe extern "C" fn elle_plugin_init`,
calls `init_keywords()`, registers all primitives, strips the prefix to
build the module struct, and returns it.

For plugins that need custom init logic, call `elle::plugin::register_and_build()`
directly (see `tls` and `jiff` for examples).

**Important:** Plugins must be compiled against the same version of Elle. There is no stable ABI — version skew will crash.

## Available plugins

| Plugin | Purpose | Primitives |
|--------|---------|-----------|
| `arrow/` | Apache Arrow columnar data | `arrow/batch`, `arrow/schema`, `arrow/column`, `arrow/to-rows`, `arrow/display`, `arrow/slice`, `arrow/write-ipc`, `arrow/read-ipc`, `arrow/write-parquet`, `arrow/read-parquet` |
| `csv/` | CSV parsing and serialization | `csv/parse`, `csv/parse-rows`, `csv/write`, `csv/write-rows` |
| `crypto/` | Cryptographic hashing | `sha256`, `hmac-sha256` |
| `hash/` | Universal hashing | `hash/md5`, `hash/sha256`, `hash/blake3`, `hash/crc32`, `hash/xxh64`, `hash/new`, `hash/update`, `hash/finalize` |
| `oxigraph/` | RDF quad store + SPARQL | `oxigraph/store-new`, `oxigraph/store-open`, `oxigraph/insert`, `oxigraph/remove`, `oxigraph/contains`, `oxigraph/quads`, `oxigraph/query`, `oxigraph/update`, `oxigraph/load`, `oxigraph/dump`, `oxigraph/iri`, `oxigraph/literal`, `oxigraph/blank-node` |
| `random/` | Random number generation | `random/int`, `random/float`, `random/bool`, `random/bytes`, `random/shuffle`, `random/choice`, `random/seed`, `random/normal`, `random/exponential`, `random/weighted`, `random/csprng-bytes`, `random/csprng-seed`, `random/sample` |
| `regex/` | Regular expressions | `regex/match`, `regex/split`, `regex/replace` |
| `selkie/` | Mermaid diagram renderer | `selkie/render`, `selkie/render-to-file`, `selkie/render-ascii` |
| `toml/` | TOML parsing and serialization | `toml/parse`, `toml/encode` |
| `tls/` | TLS client and server | `tls/client-state`, `tls/server-config`, `tls/server-state`, `tls/process`, `tls/encrypt`, `tls/get-outgoing`, `tls/get-plaintext`, `tls/read-plaintext`, `tls/plaintext-indexof`, `tls/handshake-complete?`, `tls/close-notify` |
| `xml/` | XML parsing and serialization | `xml/parse`, `xml/emit`, `xml/reader-new`, `xml/next-event`, `xml/reader-close` |
| `yaml/` | YAML parsing and serialization | `yaml/parse`, `yaml/parse-all`, `yaml/encode` |
| `mqtt/` | MQTT packet codec (state-machine pattern) | `mqtt/state`, `mqtt/encode-connect`, `mqtt/encode-publish`, `mqtt/encode-subscribe`, `mqtt/encode-unsubscribe`, `mqtt/encode-ping`, `mqtt/encode-disconnect`, `mqtt/encode-puback`, `mqtt/feed`, `mqtt/poll`, `mqtt/poll-all`, `mqtt/next-packet-id`, `mqtt/connected?`, `mqtt/keep-alive` |
| `polars/` | Polars DataFrames (eager + lazy) | `polars/df`, `polars/read-csv`, `polars/write-csv`, `polars/select`, `polars/sort`, `polars/lazy`, `polars/lfilter`, `polars/lgroupby`, `polars/collect` |
| `protobuf/` | Protocol Buffers encode/decode/introspect | `protobuf/schema`, `protobuf/schema-bytes`, `protobuf/encode`, `protobuf/decode`, `protobuf/messages`, `protobuf/fields`, `protobuf/enums` |


## Building plugins

Each plugin is a Rust crate with:
- `Cargo.toml` with `crate-type = ["cdylib"]`
- `src/lib.rs` exporting `elle_plugin_init`
- Dependencies on `elle` (path to the main crate)

```bash
# Build a plugin
cd plugins/crypto
cargo build --release

# The .so is at target/release/libelle_crypto.so
```

## Loading plugins

From Elle code:
```janet
(import "target/release/libelle_crypto.so")
(sha256 "hello")
```

From Rust:
```rust
use elle::plugin::load_plugin;
load_plugin(&mut vm, &mut symbols, "path/to/plugin.so")?;
```

## Writing a new plugin

1. Create a new directory: `plugins/myplugin/`
2. Create `Cargo.toml`:
   ```toml
   [package]
   name = "elle-myplugin"
   version = "1.0.0"
   edition = "2021"

   [lib]
   crate-type = ["cdylib"]

   [dependencies]
   elle = { path = "../.." }
   # Add your dependencies here
   ```
3. Create `src/lib.rs`:
   ```rust
   use elle::primitives::def::PrimitiveDef;
   use elle::signals::Signal;
   use elle::value::fiber::{SignalBits, SIG_OK};
   use elle::value::types::Arity;
   use elle::value::Value;

   fn prim_my_function(args: &[Value]) -> (SignalBits, Value) {
       // Implementation
   }

   static PRIMITIVES: &[PrimitiveDef] = &[
       PrimitiveDef {
           name: "myplugin/function",
           func: prim_my_function,
           signal: Signal::silent(),
           arity: Arity::Exact(1),
           doc: "Does something.",
           params: &["x"],
           category: "myplugin",
           example: "(myplugin/function 42)",
           aliases: &[],
       },
   ];

   elle::elle_plugin_init!(PRIMITIVES, "myplugin/");
   ```
4. Build: `cargo build --release`
5. Load: `(import "target/release/libelle_myplugin.so")`

## Invariants

1. **Plugins are never unloaded.** The library handle is intentionally leaked to avoid use-after-free if Elle code holds values created by the plugin.

2. **Plugins must match Elle's version.** There is no stable ABI. Recompile plugins when upgrading Elle.

3. **Plugins use the same `PrimitiveDef` mechanism.** No special registration code needed — just call `context.register()`.

4. **Plugins work with `Value` directly.** No C FFI marshalling — plugins are Rust code.

## Dependents

- `src/plugin.rs` — plugin loading infrastructure
- `src/main.rs` — loads plugins via `import` primitive
- Elle code — via `(import "path/to/plugin.so")`

## Files

| File | Purpose |
|------|---------|
| `arrow/` | Apache Arrow columnar data and Parquet serialization |
| `crypto/` | SHA256 and HMAC-SHA256 hashing |
| `csv/` | CSV parsing and serialization |
| `egui/` | Immediate-mode GUI via egui |
| `hash/` | Universal hashing (MD5, SHA-1/2/3, BLAKE2/3, CRC32, xxHash) |
| `jiff/` | Date/time operations |
| `mqtt/` | MQTT packet codec (state-machine, no I/O) |
| `msgpack/` | MessagePack serialization |
| `oxigraph/` | RDF quad store with SPARQL query and update |
| `polars/` | Polars DataFrame operations (eager and lazy APIs) |
| `protobuf/` | Protocol Buffers encoding, decoding, and introspection |
| `random/` | Random number generation (distributions and CSPRNG) |
| `regex/` | Regular expression matching and replacement |
| `selkie/` | Mermaid diagram rendering |
| `syn/` | Rust syntax parsing via the `syn` crate |
| `tls/` | TLS client and server via rustls |
| `toml/` | TOML parsing and serialization |
| `tree-sitter/` | Multi-language parsing |
| `xml/` | XML parsing and serialization (DOM and streaming APIs) |
| `yaml/` | YAML parsing and serialization |
