# Plugins

Elle ships with Rust plugins and pure Elle standard library modules.
Plugins are cdylib crates loaded at runtime via `import`. Standard modules
use `import` with the `std/` prefix and require no compilation.

## Stable ABI

Plugins depend on the `elle-plugin` crate — not on `elle` itself, so a
plugin compiles independently and loads at runtime. The ABI uses a named
function lookup pattern (like `vkGetInstanceProcAddr`): the plugin asks
the host for each API function by name at init time. Adding API functions
to elle never breaks an existing plugin, because a plugin only asks for
the names it was written against.

Plugins live in a [separate repository](https://github.com/elle-lisp/plugins),
available as a git submodule at `plugins/`.
See [`docs/cookbook/plugins.md`](cookbook/plugins.md) for a step-by-step
guide to writing a plugin.

### The ABI version

Name lookup carries the name, not the calling convention. A plugin that
resolves `struct_key` gets whatever the host has under that name. It then
calls that pointer through the argument list it was compiled with. A
changed signature is therefore a corrupt call, not a failed lookup.
`ABI_VERSION` names the current calling convention, and it is the only
thing that tells those two cases apart.

The host advertises its version in `ElleApiLoader::version`, and
`define_plugin!`'s init returns `-2` when that differs from the
`ABI_VERSION` the plugin was built against. The load fails cleanly and the
`import` reports it.

| Version | What it named |
|---------|---------------|
| 2 | Primitives took `(args, nargs)`. |
| 3 | Primitives take a leading opaque `*mut ElleCtx`, and thread it into the allocating constructors. |
| 4 | The keyword and struct-key name readers take the `ctx` too: a spelling comes from the calling instance's memo. |

Bump `ABI_VERSION` whenever an existing declaration in `elle_api!` changes
its arguments or return type, whenever a declaration is removed, and
whenever the primitive calling convention itself changes. Adding a new
declaration needs no bump.

`elle-plugin`'s tests pin the signature of every function the current
version names. Changing one fails the build until the pin and the version
move together, because an unbumped signature change is the one breakage
the load guard cannot see.

## Building plugins

Plugins are in the `plugins/` submodule. Initialize it first:

```bash
git submodule update --init plugins
```

Then build from the elle repo root:

```bash
make plugins          # portable plugins
make plugins-all      # every plugin in the workspace
make mcp              # just oxigraph + syn (for the MCP server)
```

`make plugins` builds the `PORTABLE` list in `plugins/Makefile`.
`make plugins-all` adds `elle-arrow`, `elle-polars`, `elle-vulkan`,
`elle-egui` and `elle-wayland`. All five compile on a stock toolchain — each
opens its system library with dlopen — but the last three need a GPU or a
display before they can do anything. That is what keeps them out of the
portable list; docs/analysis/ci.md § "The plugins job" owns the split.

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

## Testing plugins

Each plugin has an integration test under `plugins/tests/`. Run the whole set
against the elle binary from the repo root:

```bash
make plugins          # build the portable plugins first
make smoke-plugins    # assert the artifacts, then run plugins/tests/*.lisp
```

`smoke-plugins` runs `plugins-verify` first, which fails when a portable plugin
produced no `.so`. That assertion is not decoration. Every test file imports its
`.so` under `protect` and exits 0 when the import fails, so a plugin that did
not build makes its own test report success. The reasoning, and the CI job that
runs these targets, are in
[`docs/analysis/ci.md`](analysis/ci.md) § "The plugins job".

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
