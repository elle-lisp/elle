# Plugins

Elle ships with 29 plugins — Rust cdylib crates that extend the language
with new primitives. `import` loads a plugin and returns a struct of its
functions.

## Usage pattern

```text
(def crypto (import "crypto"))
(seq->hex (crypto:sha256 "hello"))
```

Build plugins before use: `cargo build --release -p elle-crypto`.

## Shipped plugins

| Plugin | Import name | Description |
|--------|-------------|-------------|
| `elle-arrow` | `"arrow"` | Apache Arrow columnar data |
| `elle-base64` | `"base64"` | Base64 encoding/decoding |
| `elle-clap` | `"clap"` | CLI argument parsing |
| `elle-compress` | `"compress"` | Compression (gzip, zstd, etc.) |
| `elle-crypto` | `"crypto"` | SHA-2 hashing and HMAC |
| `elle-csv` | `"csv"` | CSV reading and writing |
| `elle-git` | `"git"` | Git repository operations |
| `elle-glob` | `"glob"` | Filesystem glob patterns |
| `elle-hash` | `"hash"` | Universal hashing (SHA-3, BLAKE3, CRC32, etc.) |
| `elle-jiff` | `"jiff"` | Date/time operations |
| `elle-mqtt` | `"mqtt"` | MQTT client |
| `elle-msgpack` | `"msgpack"` | MessagePack serialization |
| `elle-oxigraph` | `"oxigraph"` | RDF triple store |
| `elle-polars` | `"polars"` | DataFrames (Polars) |
| `elle-protobuf` | `"protobuf"` | Protocol Buffers |
| `elle-random` | `"random"` | Pseudo-random numbers |
| `elle-regex` | `"regex"` | Regular expressions |
| `elle-selkie` | `"selkie"` | Mermaid diagram rendering |
| `elle-semver` | `"semver"` | Semantic versioning |
| `elle-sqlite` | `"sqlite"` | SQLite database |
| `elle-syn` | `"syn"` | Rust source parsing |
| `elle-tls` | `"tls"` | TLS client/server (rustls) |
| `elle-toml` | `"toml"` | TOML parsing |
| `elle-tree-sitter` | `"tree-sitter"` | Multi-language parsing |
| `elle-uuid` | `"uuid"` | UUID generation |
| `elle-watch` | `"watch"` | Filesystem watching |
| `elle-xml` | `"xml"` | XML parsing |
| `elle-yaml` | `"yaml"` | YAML parsing |
| `elle-egui` | `"egui"` | Immediate-mode GUI |

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
