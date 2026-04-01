# Plugins

Elle ships with 29 plugins — Rust cdylib crates that extend the language
with new primitives. `import` loads a plugin and returns a struct of its
functions.

## Usage pattern

```lisp
# (def crypto (import "plugin/crypto"))
# (seq->hex (crypto:sha256 "hello"))
```

Build plugins before use: `cargo build --release -p elle-crypto`.

## Shipped plugins

| Plugin | Import name | Description |
|--------|-------------|-------------|
| `elle-arrow` | `"plugin/arrow"` | Apache Arrow columnar data |
| `elle-base64` | `"plugin/base64"` | Base64 encoding/decoding |
| `elle-clap` | `"plugin/clap"` | CLI argument parsing |
| `elle-compress` | `"plugin/compress"` | Compression (gzip, zstd, etc.) |
| `elle-crypto` | `"plugin/crypto"` | SHA-2 hashing and HMAC |
| `elle-csv` | `"plugin/csv"` | CSV reading and writing |
| `elle-git` | `"plugin/git"` | Git repository operations |
| `elle-glob` | `"plugin/glob"` | Filesystem glob patterns |
| `elle-hash` | `"plugin/hash"` | Universal hashing (SHA-3, BLAKE3, CRC32, etc.) |
| `elle-jiff` | `"plugin/jiff"` | Date/time operations |
| `elle-mqtt` | `"plugin/mqtt"` | MQTT client |
| `elle-msgpack` | `"plugin/msgpack"` | MessagePack serialization |
| `elle-oxigraph` | `"plugin/oxigraph"` | RDF triple store |
| `elle-polars` | `"plugin/polars"` | DataFrames (Polars) |
| `elle-protobuf` | `"plugin/protobuf"` | Protocol Buffers |
| `elle-random` | `"plugin/random"` | Pseudo-random numbers |
| `elle-regex` | `"plugin/regex"` | Regular expressions |
| `elle-selkie` | `"plugin/selkie"` | Mermaid diagram rendering |
| `elle-semver` | `"plugin/semver"` | Semantic versioning |
| `elle-sqlite` | `"plugin/sqlite"` | SQLite database |
| `elle-syn` | `"plugin/syn"` | Rust source parsing |
| `elle-tls` | `"plugin/tls"` | TLS client/server (rustls) |
| `elle-toml` | `"plugin/toml"` | TOML parsing |
| `elle-tree-sitter` | `"plugin/tree-sitter"` | Multi-language parsing |
| `elle-uuid` | `"plugin/uuid"` | UUID generation |
| `elle-watch` | `"plugin/watch"` | Filesystem watching |
| `elle-xml` | `"plugin/xml"` | XML parsing |
| `elle-yaml` | `"plugin/yaml"` | YAML parsing |
| `elle-egui` | `"plugin/egui"` | Immediate-mode GUI |

## Gotchas

- `import` returns a **struct** — access functions via `get` or
  accessor syntax (`crypto:sha256`)
- Plugins are **never unloaded** — the library handle is leaked
- No stable ABI — recompile when upgrading Elle
- The analyzer has no static knowledge of plugin functions
- Bind once at top level to avoid redundant loads

## Writing plugins

See `plugins/AGENTS.md` and `docs/cookbook.md` for the recipe.

---

## See also

- [modules.md](modules.md) — import system
- [stdlib.md](stdlib.md) — standard library modules
- [cookbook.md](cookbook.md) — adding a new plugin
