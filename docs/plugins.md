# Plugins

Elle ships with Rust plugins and pure Elle standard library modules.
Plugins are cdylib crates loaded at runtime via `import`. Standard modules
use `import` with the `std/` prefix and require no compilation.

## Stable ABI

Plugins depend on the `elle-plugin` crate — not on `elle` itself. This
provides a stable ABI: plugins can be compiled independently from elle
and loaded at runtime without version matching. The ABI uses a named
function lookup pattern (like `vkGetInstanceProcAddr`). Adding API
functions to elle never breaks existing plugins.

Plugins live in a [separate repository](https://github.com/elle-lisp/plugins).
See [`docs/cookbook/plugins.md`](cookbook/plugins.md) for a step-by-step
guide to writing a plugin.

## Usage pattern

```text
## Plugin (Rust cdylib)
(def crypto (import "plugin/crypto"))
(seq->hex (crypto:sha256 "hello"))

## Standard module (pure Elle or FFI)
(def b64 ((import "std/base64")))
(b64:encode "hello")
```

Build plugins before use: `cargo build --release -p elle-crypto`.

## Rust plugins

| Plugin | Import name | Description |
|--------|-------------|-------------|
| `elle-arrow` | `"plugin/arrow"` | Apache Arrow columnar data |
| `elle-crypto` | `"plugin/crypto"` | SHA-2 hashing and HMAC |
| `elle-csv` | `"plugin/csv"` | CSV reading and writing |
| `elle-egui` | `"plugin/egui"` | Immediate-mode GUI |
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
| `elle-syn` | `"plugin/syn"` | Rust source parsing |
| `elle-tls` | `"plugin/tls"` | TLS client/server (rustls) |
| `elle-toml` | `"plugin/toml"` | TOML parsing |
| `elle-tree-sitter` | `"plugin/tree-sitter"` | Multi-language parsing |
| `elle-wayland` | `"plugin/wayland"` | Wayland compositor interaction |
| `elle-xml` | `"plugin/xml"` | XML parsing |
| `elle-yaml` | `"plugin/yaml"` | YAML parsing |

## Standard library modules (pure Elle / FFI)

| Module | Import | Description |
|--------|--------|-------------|
| `base64` | `(def b64 ((import "std/base64")))` | Base64 encoding/decoding |
| `cli` | `(def cli ((import "std/cli")))` | CLI argument parsing |
| `compress` | `(def z ((import "std/compress")))` | Gzip, zlib, deflate, zstd (FFI to libz + libzstd) |
| `git` | `(def git ((import "std/git")))` | Git repository operations (FFI to libgit2) |
| `glob` | `(def glob ((import "std/glob")))` | Filesystem glob patterns |
| `semver` | `(def sv ((import "std/semver")))` | Semantic versioning |
| `sqlite` | `(def db ((import "std/sqlite")))` | SQLite database (FFI to libsqlite3) |
| `uuid` | `(def uuid ((import "std/uuid")))` | UUID generation and parsing |
| `wayland` | `(def wl ((import "std/wayland") plugin))` | Wayland Elle wrapper |
| `watch` | `(def w ((import "std/watch")))` | Filesystem watching |

## Gotchas

- `import` returns a **struct** — access functions via `get` or
  accessor syntax (`crypto:sha256`)
- Plugins are **never unloaded** — the library handle is leaked
- The analyzer has no static knowledge of plugin functions
- Bind once at top level to avoid redundant loads

## Writing plugins

See [`docs/cookbook/plugins.md`](cookbook/plugins.md) for the recipe and
[`plugins/AGENTS.md`](../plugins/AGENTS.md) for technical reference.

---

## See also

- [modules.md](modules.md) — import system
- [stdlib.md](stdlib.md) — standard library modules
- [cookbook.md](cookbook.md) — adding a new plugin
