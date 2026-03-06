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

**Important:** Plugins must be compiled against the same version of Elle. There is no stable ABI — version skew will crash.

## Available plugins

| Plugin | Purpose | Primitives |
|--------|---------|-----------|
| `crypto/` | Cryptographic hashing | `sha256`, `hmac-sha256` |
| `glob/` | Filesystem globbing | `glob/match`, `glob/glob` |
| `random/` | Random number generation | `random/int`, `random/float`, `random/shuffle` |
| `regex/` | Regular expressions | `regex/match`, `regex/split`, `regex/replace` |
| `selkie/` | HTTP client | `http/get`, `http/post`, `http/request` |
| `sqlite/` | SQLite database | `db/open`, `db/query`, `db/exec` |

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
   use elle::plugin::PluginContext;
   use elle::primitives::def::PrimitiveDef;
   use elle::value::Value;

   pub fn prim_my_function(args: &[Value]) -> (SignalBits, Value) {
       // Implementation
   }

   const MY_PRIMITIVE: PrimitiveDef = PrimitiveDef {
       name: "my/function",
       func: prim_my_function,
       // ... other fields
   };

   #[no_mangle]
   pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
       ctx.register(&MY_PRIMITIVE);
       Value::true_()
   }
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
| `crypto/` | SHA256 and HMAC-SHA256 hashing |
| `glob/` | Filesystem pattern matching |
| `random/` | Random number generation |
| `regex/` | Regular expression matching and replacement |
| `selkie/` | HTTP client library |
| `sqlite/` | SQLite database access |
