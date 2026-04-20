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

Plugins live in a [separate repository](https://github.com/elle-lisp/plugins),
available as a git submodule at `plugins/`.
See [`docs/cookbook/plugins.md`](cookbook/plugins.md) for a step-by-step
guide to writing a plugin.

## Building plugins

Plugins are in the `plugins/` submodule. Initialize it first:

```bash
git submodule update --init plugins
```

Then build from the elle repo root:

```bash
make plugins          # portable plugins (no system deps)
make plugins-all      # all plugins (requires vulkan, wayland, egui libs)
make mcp              # just oxigraph + syn (for the MCP server)
```

Or build individual plugins:

```bash
make -C plugins portable                          # all portable
cargo build --release --manifest-path plugins/Cargo.toml \
    --target-dir target -p elle-crypto             # single plugin
```

The `--target-dir target` flag (or `make` from the root) places `.so` files
in elle's `target/release/`, where the `plugin/` import prefix looks. If
you build from inside `plugins/` directly with plain `cargo build`, the
output lands in `plugins/target/release/` instead — elle won't find it
unless you move the `.so` files or use `--path` (see below).

The plugins submodule's own Makefile handles this automatically when it
detects it's inside the elle repo, so `cd plugins && make` also works.

## Usage pattern

```text
## Plugin (Rust cdylib)
(def crypto (import "plugin/crypto"))
(seq->hex (crypto:sha256 "hello"))

## Standard module (pure Elle or FFI)
(def b64 ((import "std/base64")))
(b64:encode "hello")
```

## Module search path

When `import` resolves a specifier, it searches in order:

**1. Virtual prefixes** (checked first, before the search path):

| Prefix | Resolves to |
|--------|-------------|
| `std/X` | `<root>/lib/X.lisp` |
| `plugin/X` | `<root>/target/<profile>/libelle_X.so` |

The root is `--home` (or `ELLE_HOME`), or auto-detected by walking up
from the elle binary to find `Cargo.toml`. Plugin resolution prefers the
same build profile as the running binary (release or debug) and falls
back to the other.

**2. Search path** (for specifiers that don't match a virtual prefix):

For each directory in the search path, `import` tries:
- `<dir>/<spec>.lisp`
- `<dir>/<spec>` (as-is)
- `<dir>/<spec_dir>/libelle_<leaf>.so` (hierarchical plugin)
- `<dir>/libelle_<leaf>.so` (flat plugin)

Search directories, in order:
1. Current working directory
2. `--path` / `ELLE_PATH` entries (colon-separated)
3. `--home` / `ELLE_HOME` (or directory of the elle binary)

**Example:** if you built plugins somewhere else, point elle at them:

```bash
elle --path=/opt/elle-plugins/target/release my-script.lisp
```

## Rust plugins

| Plugin | Import name | Description |
|--------|-------------|-------------|
| `elle-arrow` | `"plugin/arrow"` | Apache Arrow columnar data |
| `elle-crypto` | `"plugin/crypto"` | SHA-2 hashing and HMAC |
| `elle-csv` | `"plugin/csv"` | CSV reading and writing |
| `elle-egui` | `"plugin/egui"` | Immediate-mode GUI |
| `elle-hash` | `"plugin/hash"` | Universal hashing (SHA-3, BLAKE3, CRC32, etc.) |
| `elle-image` | `"plugin/image"` | Raster image I/O, transforms, drawing, and analysis |
| `elle-jiff` | `"plugin/jiff"` | Date/time operations |
| `elle-mqtt` | `"plugin/mqtt"` | MQTT client |
| `elle-msgpack` | `"plugin/msgpack"` | MessagePack serialization |
| `elle-oxigraph` | `"plugin/oxigraph"` | RDF triple store |
| `elle-polars` | `"plugin/polars"` | DataFrames (Polars) |
| `elle-protobuf` | `"plugin/protobuf"` | Protocol Buffers |
| `elle-random` | `"plugin/random"` | Pseudo-random numbers |
| `elle-regex` | `"plugin/regex"` | Regular expressions |
| `elle-plotters` | `"plugin/plotters"` | Chart and plot generation |
| `elle-selkie` | `"plugin/selkie"` | Mermaid diagram rendering |
| `elle-svg` | `"plugin/svg"` | SVG rasterization (resvg) |
| `elle-syn` | `"plugin/syn"` | Rust source parsing |
| `elle-tls` | `"plugin/tls"` | TLS client/server (rustls) |
| `elle-toml` | `"plugin/toml"` | TOML parsing |
| `elle-tree-sitter` | `"plugin/tree-sitter"` | Multi-language parsing |
| `elle-vulkan` | `"plugin/vulkan"` | Vulkan compute dispatch |
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
