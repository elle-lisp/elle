# Adding a New Plugin


A plugin is a Rust cdylib crate that exports `elle_plugin_init` and
returns a struct of native functions. Plugins are loaded at runtime via
`(import-file "path/to/plugin.so")`.

### Files to create / modify (in order)

1. **`plugins/myplugin/Cargo.toml`** — New crate with `crate-type = ["cdylib"]`.

2. **`plugins/myplugin/src/lib.rs`** — Plugin implementation.

3. **`Cargo.toml`** (root) — Add `"plugins/myplugin"` to `[workspace] members`.

4. **`Makefile`** — Add `myplugin` to the `PLUGINS` variable (one name
   per line, alphabetical).

5. **`tests/elle/plugins/myplugin.lisp`** — Integration tests.

6. **`plugins/myplugin/README.md`** — Documentation.

### Step by step

**Step 1: Create the crate.**

```toml
# plugins/myplugin/Cargo.toml
[package]
name = "elle-myplugin"
version = "1.0.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
elle = { path = "../.." }
```

**Step 2: Implement the plugin.** Every plugin follows the same
structure — an `elle_plugin_init` entry point that registers primitives
and returns a struct:

```rust
use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};
use std::collections::BTreeMap;

#[no_mangle]
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
    let mut fields = BTreeMap::new();
    for def in PRIMITIVES {
        ctx.register(def);
        let short = def.name.strip_prefix("myplugin/").unwrap_or(def.name);
        fields.insert(TableKey::Keyword(short.into()), Value::native_fn(def.func));
    }
    Value::struct_from(fields)
}

fn prim_hello(args: &[Value]) -> (SignalBits, Value) {
    // ... implementation ...
    (SIG_OK, Value::string("hello"))
}

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "myplugin/hello",
        func: prim_hello,
        signal: Signal::errors(),
        arity: Arity::Exact(0),
        doc: "Say hello.",
        params: &[],
        category: "myplugin",
        example: r#"(myplugin/hello)"#,
        aliases: &[],
    },
];
```

Important: the `Arity` in `PrimitiveDef` only applies when the function
is called by its registered name. When called through the struct (the
common case), arity is not enforced automatically — validate `args.len()`
inside each function.

**Step 3: Register in the workspace.** Add to the root `Cargo.toml`:

```toml
[workspace]
members = [
    # ...
    "plugins/myplugin",
]
```

**Step 4: Add to CI.** Add `myplugin` to the `PLUGINS` variable in the
`Makefile`, keeping alphabetical order. The CI workflows derive their
plugin matrix from the same list. Run `make check-plugin-list` to verify
the Makefile and Cargo.toml stay in sync — CI runs this check on every
PR and merge.

**Step 5: Write tests** in `tests/elle/plugins/myplugin.lisp`:

```lisp
(elle/epoch 1)

(def [ok? plugin] (protect (import-file "target/release/libelle_myplugin.so")))
(when (not ok?)
  (println "SKIP: myplugin plugin not built")
  (exit 0))

(def hello-fn (get plugin :hello))

(assert (= (hello-fn) "hello") "myplugin/hello works")
```

### Conventions

- Plugin functions should validate arity and types themselves (see note
  above about struct-based calls).
- Use helper functions (`require_arity`, `require_string`, etc.) to
  reduce boilerplate — see the regex or jiff plugins for examples.
- Return `(SIG_OK, value)` for success, `(SIG_ERROR, error_val(...))` for
  errors.
- Wrap external Rust types via `Value::external("typename", value)`.
- The entry point **must** return `Value::struct_from(fields)`, not a
  bare boolean or nil.

---

## See also

- [Cookbook index](index.md)
