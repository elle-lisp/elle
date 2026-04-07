# Plugins

Dynamically-loaded Rust libraries that extend Elle with additional primitives. Plugins are compiled as `.so` files and loaded at runtime via `(import-file "path/to/plugin.so")`.

## Available Plugins

| Plugin | Purpose | Key Primitives |
|--------|---------|-----------------|
| [`arrow/`](arrow/) | Apache Arrow columnar data | `arrow/batch`, `arrow/schema`, `arrow/column`, `arrow/to-rows`, `arrow/display`, `arrow/slice`, `arrow/write-ipc`, `arrow/read-ipc`, `arrow/write-parquet`, `arrow/read-parquet` |
| [`crypto/`](crypto/) | Cryptographic hashing | `sha256`, `hmac-sha256`, `sha512`, etc. |
| [`csv/`](csv/) | CSV parsing and serialization | `csv/parse`, `csv/parse-rows`, `csv/write`, `csv/write-rows` |
| [`egui/`](egui/) | Immediate-mode GUI | egui/eframe windowing and rendering |
| [`hash/`](hash/) | Universal hashing | `hash/md5`, `hash/sha256`, `hash/blake3`, `hash/crc32`, `hash/new`, `hash/update`, `hash/finalize` |
| [`jiff/`](jiff/) | Date, time, and duration arithmetic | `date/year`, `date/month`, `date/day`, `date/weekday` |
| [`mqtt/`](mqtt/) | MQTT packet codec | `mqtt/state`, `mqtt/encode-connect`, `mqtt/feed`, `mqtt/poll` |
| [`msgpack/`](msgpack/) | MessagePack serialization | `msgpack/encode`, `msgpack/decode`, `msgpack/valid?`, `msgpack/encode-tagged`, `msgpack/decode-tagged` |
| [`oxigraph/`](oxigraph/) | RDF quad store + SPARQL | `oxigraph/store-new`, `oxigraph/store-open`, `oxigraph/query`, `oxigraph/update`, `oxigraph/load`, `oxigraph/dump` |
| [`polars/`](polars/) | Polars DataFrames | `polars/df`, `polars/read-csv`, `polars/write-csv`, `polars/select`, `polars/sort`, `polars/lazy`, `polars/collect` |
| [`protobuf/`](protobuf/) | Protocol Buffers serialization | `protobuf/schema`, `protobuf/encode`, `protobuf/decode`, `protobuf/messages`, `protobuf/fields` |
| [`random/`](random/) | Random number generation | `random/int`, `random/float`, `random/normal`, `random/exponential`, `random/weighted`, `random/csprng-bytes`, `random/sample` |
| [`regex/`](regex/) | Regular expressions | `regex/match`, `regex/split`, `regex/replace` |
| [`selkie/`](selkie/) | Mermaid diagram renderer | `selkie/render`, `selkie/render-to-file`, `selkie/render-ascii` |
| [`syn/`](syn/) | Rust syntax parsing via syn | `syn/parse-file`, `syn/parse-expr`, `syn/items`, `syn/fn-info` |
| [`tls/`](tls/) | TLS 1.2/1.3 via rustls | `tls/client-state`, `tls/process`, `tls/get-plaintext` |
| [`toml/`](toml/) | TOML parsing and serialization | `toml/parse`, `toml/encode` |
| [`tree-sitter/`](tree-sitter/) | Multi-language parsing and structural queries | `ts/language`, `ts/parse`, `ts/root`, `ts/node-type`, `ts/node-text` |
| [`xml/`](xml/) | XML parsing and serialization | `xml/parse`, `xml/emit`, `xml/reader-new`, `xml/next-event`, `xml/reader-close` |
| [`yaml/`](yaml/) | YAML parsing and serialization | `yaml/parse`, `yaml/parse-all`, `yaml/encode` |

For migrated modules (base64, cli, compress, git, glob, semver, sqlite, uuid), see `lib/` and `docs/libraries.md`.

## Building Plugins

Build all plugins as part of the workspace:

```bash
cargo build --workspace
```

Individual plugins:

```bash
cd plugins/crypto
cargo build --release
# Output: target/release/libelle_crypto.so
```

## Loading Plugins

From Elle code:

```janet
(import-file "target/release/libelle_crypto.so")
(bytes->hex (crypto/sha256 "hello"))
```

From Rust:

```rust
use elle::plugin::load_plugin;
load_plugin(&mut vm, &mut symbols, "path/to/plugin.so")?;
```

## Plugin System

Plugins are Rust cdylib crates that export an `elle_plugin_init` function. The plugin loader:

1. Loads the `.so` file via `libloading`
2. Calls `elle_plugin_init` with a `PluginContext`
3. The plugin registers its primitives via `context.register(def)`
4. The loader installs all registered primitives into the VM

**Important**: Plugins must be compiled against the same version of Elle. There is no stable ABI — version skew will crash.

## Writing a New Plugin

1. Create a new directory: `plugins/myplugin/`
2. Create `Cargo.toml` with `crate-type = ["cdylib"]`
3. Create `src/lib.rs` exporting `elle_plugin_init`
4. Implement primitives and register them
5. Build: `cargo build --release`
6. Load: `(import-file "target/release/libelle_myplugin.so")`
7. Add `"plugins/myplugin"` to the workspace `members` list in
   the root `Cargo.toml`
8. Add `myplugin` to the `PLUGINS` variable in the `Makefile`
   (the CI plugin matrix and `check-plugin-list` target derive from this)
9. Write tests in `tests/elle/plugins/myplugin.lisp`

Run `make check-plugin-list` to verify the `Makefile` and `Cargo.toml`
stay in sync. CI runs this check on every PR and merge.

See [`crypto/`](crypto/) for a complete example.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/plugin.rs`](../src/plugin.rs) - plugin loading infrastructure
- [`crypto/README.md`](crypto/README.md) - crypto plugin documentation
- [`glob/README.md`](glob/README.md) - glob plugin documentation
