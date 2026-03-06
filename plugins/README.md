# Plugins

Dynamically-loaded Rust libraries that extend Elle with additional primitives. Plugins are compiled as `.so` files and loaded at runtime via `(import-file "path/to/plugin.so")`.

## Available Plugins

| Plugin | Purpose | Key Primitives |
|--------|---------|-----------------|
| [`crypto/`](crypto/) | Cryptographic hashing | `sha256`, `hmac-sha256`, `sha512`, etc. |
| [`glob/`](glob/) | Filesystem pattern matching | `glob/match`, `glob/glob` |
| [`random/`](random/) | Random number generation | `random/int`, `random/float`, `random/shuffle` |
| [`regex/`](regex/) | Regular expressions | `regex/match`, `regex/split`, `regex/replace` |
| [`selkie/`](selkie/) | HTTP client | `http/get`, `http/post`, `http/request` |
| [`sqlite/`](sqlite/) | SQLite database | `db/open`, `db/query`, `db/exec` |

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

```lisp
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

See [`crypto/`](crypto/) for a complete example.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/plugin.rs`](../src/plugin.rs) - plugin loading infrastructure
- [`crypto/README.md`](crypto/README.md) - crypto plugin documentation
- [`glob/README.md`](glob/README.md) - glob plugin documentation
