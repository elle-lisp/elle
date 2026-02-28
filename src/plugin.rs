//! Plugin loading for dynamically-linked Rust libraries.
//!
//! Plugins are `.so` files (cdylib crates) that export an `elle_plugin_init`
//! function. They register primitives using the same `PrimitiveDef` mechanism
//! as built-in primitives, and work directly with `Value` — no C FFI
//! marshalling.
//!
//! Plugins must be compiled against the same version of Elle. There is no
//! stable ABI — version skew will crash.

use crate::primitives::def::{Doc, PrimitiveDef};
use crate::symbol::SymbolTable;
use crate::value::Value;
use crate::vm::VM;

/// Context passed to a plugin's init function.
///
/// The plugin calls `register` to declare its primitives. After init
/// returns, the collected definitions are registered into the VM.
pub struct PluginContext {
    primitives: Vec<&'static PrimitiveDef>,
}

impl PluginContext {
    fn new() -> Self {
        PluginContext {
            primitives: Vec::new(),
        }
    }

    /// Register a primitive definition. The `PrimitiveDef` must be `'static`
    /// (typically a const or static in the plugin).
    pub fn register(&mut self, def: &'static PrimitiveDef) {
        self.primitives.push(def);
    }
}

/// The function signature that plugins must export.
///
/// Declared `extern "C"` for symbol visibility (no name mangling), not
/// ABI safety. The `PluginContext` argument is a Rust type — the plugin
/// must be compiled with the same Rust compiler version and the same
/// `elle` crate version. Version skew will crash.
pub type PluginInitFn = unsafe extern "C" fn(ctx: &mut PluginContext) -> Value;

/// Load a plugin `.so` and register its primitives.
///
/// The library handle is intentionally leaked — plugins are never unloaded.
/// This avoids use-after-free if Elle code holds values created by the plugin.
///
/// The caller is responsible for deduplication (e.g., via `is_module_loaded`).
/// Calling this twice with the same path will register primitives twice and
/// leak a second library handle.
pub fn load_plugin(path: &str, vm: &mut VM, symbols: &mut SymbolTable) -> Result<Value, String> {
    // Load the shared library
    let lib = unsafe { libloading::Library::new(path) }
        .map_err(|e| format!("failed to load plugin '{}': {}", path, e))?;

    // Look up the init function
    let init_fn: libloading::Symbol<PluginInitFn> = unsafe { lib.get(b"elle_plugin_init") }
        .map_err(|e| format!("plugin '{}' missing elle_plugin_init: {}", path, e))?;

    // Call init to collect primitive definitions
    let mut ctx = PluginContext::new();
    let return_value = unsafe { init_fn(&mut ctx) };

    // Register collected primitives into the VM's globals and docs.
    //
    // Note: plugin effects and arities are NOT registered in PrimitiveMeta
    // because plugins are loaded at runtime (via `import-file`), after the
    // static analyzer has already processed the calling code. The analyzer
    // will see plugin primitives as unknown globals, not as primitives with
    // known effects or arities. This is the same limitation as any runtime
    // import — a pre-existing constraint, not a plugin-specific gap.
    for def in &ctx.primitives {
        let sym_id = symbols.intern(def.name);
        vm.set_global(sym_id.0, Value::native_fn(def.func));

        let doc = Doc {
            name: def.name,
            doc: def.doc,
            params: def.params,
            arity: def.arity,
            effect: def.effect,
            category: def.category,
            example: def.example,
            aliases: def.aliases,
        };
        vm.docs.insert(def.name.to_string(), doc.clone());

        for alias in def.aliases {
            let alias_id = symbols.intern(alias);
            vm.set_global(alias_id.0, Value::native_fn(def.func));
            vm.docs.insert((*alias).to_string(), doc.clone());
        }
    }

    // Leak the library handle — never unload plugins
    std::mem::forget(lib);

    Ok(return_value)
}
