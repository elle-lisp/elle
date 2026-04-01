//! Plugin loading for dynamically-linked Rust libraries.
//!
//! Plugins are `.so` files (cdylib crates) that export an `elle_plugin_init`
//! function. They register primitives using the same `PrimitiveDef` mechanism
//! as built-in primitives, and work directly with `Value` — no C FFI
//! marshalling.
//!
//! Plugins must be compiled against the same version of Elle. There is no
//! stable ABI — version skew will crash.

use crate::error::{LError, LResult};
use crate::primitives::def::{Doc, PrimitiveDef};
use crate::symbol::SymbolTable;
use crate::value::Value;
use crate::vm::VM;

/// Context passed to a plugin's init function.
///
/// The plugin calls `register` to declare its primitives. After init
/// returns, the collected definitions are registered into the VM.
///
/// The plugin must call `init_keywords()` at the start of `elle_plugin_init`
/// to route keyword operations to the host's global name table. Without this,
/// keywords created in the host are invisible to `as_keyword_name()` in the
/// plugin (each DSO has its own copy of the `elle` statics).
pub struct PluginContext {
    primitives: Vec<&'static PrimitiveDef>,
    /// Host's `intern_keyword` function. Passed to `set_keyword_fns` during
    /// `init_keywords()` so the plugin routes keyword interning to the host.
    intern_keyword_fn: fn(&str) -> u64,
    /// Host's `keyword_name` function. Passed to `set_keyword_fns` during
    /// `init_keywords()` so the plugin routes name lookup to the host.
    keyword_name_fn: fn(u64) -> Option<String>,
}

impl PluginContext {
    fn new() -> Self {
        PluginContext {
            primitives: Vec::new(),
            intern_keyword_fn: crate::value::keyword::intern_keyword,
            keyword_name_fn: crate::value::keyword::keyword_name,
        }
    }

    /// Register a primitive definition. The `PrimitiveDef` must be `'static`
    /// (typically a const or static in the plugin).
    pub fn register(&mut self, def: &'static PrimitiveDef) {
        self.primitives.push(def);
    }

    /// Route this DSO's keyword operations to the host's global name table.
    ///
    /// Must be called at the start of `elle_plugin_init`, before any keyword
    /// is created or looked up. Calling it after keyword creation will leave
    /// some hashes unregistered in the host's table.
    ///
    /// Each cdylib plugin has its own copy of the `elle` statics (including
    /// `KEYWORD_NAMES`). Without this call, keywords created in the host are
    /// invisible to `as_keyword_name()` in the plugin, and vice versa.
    pub fn init_keywords(&self) {
        crate::value::keyword::set_keyword_fns(self.intern_keyword_fn, self.keyword_name_fn);
    }
}

/// Register primitives from a slice and build the module struct.
///
/// This is the runtime helper behind `elle_plugin_init!`. Plugins that
/// need custom init logic before registration can call this directly.
pub fn register_and_build(
    ctx: &mut PluginContext,
    primitives: &'static [PrimitiveDef],
    prefix: &str,
) -> Value {
    use crate::value::types::TableKey;
    use std::collections::BTreeMap;

    ctx.init_keywords();
    let mut fields = BTreeMap::new();
    for def in primitives {
        ctx.register(def);
        let short_name = def.name.strip_prefix(prefix).unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}

/// Like `register_and_build` but accepts a borrowed slice of references.
///
/// Used by plugins that collect primitives from multiple sub-modules
/// into a `Vec<&'static PrimitiveDef>` at init time.
pub fn register_and_build_refs(
    ctx: &mut PluginContext,
    primitives: &[&'static PrimitiveDef],
    prefix: &str,
) -> Value {
    use crate::value::types::TableKey;
    use std::collections::BTreeMap;

    ctx.init_keywords();
    let mut fields = BTreeMap::new();
    for def in primitives {
        ctx.register(def);
        let short_name = def.name.strip_prefix(prefix).unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}

/// Generate the boilerplate `elle_plugin_init` entry point.
///
/// # Usage
///
/// ```ignore
/// elle::elle_plugin_init!(PRIMITIVES, "mymod/");
/// ```
///
/// Expands to a `#[no_mangle] pub unsafe extern "C" fn elle_plugin_init`
/// that calls `init_keywords()`, registers every `PrimitiveDef` in the
/// given static slice, strips the prefix from names to build the module
/// struct, and returns it.
#[macro_export]
macro_rules! elle_plugin_init {
    ($prims:expr, $prefix:expr) => {
        // #[used] ensures clippy's dead-code analysis sees PRIMITIVES as
        // reachable even in --all-targets (test) builds, where #[no_mangle]
        // from a macro expansion isn't traced through.
        #[used]
        static _ELLE_PLUGIN_PRIMS: &[$crate::primitives::def::PrimitiveDef] = $prims;

        #[no_mangle]
        pub unsafe extern "C" fn elle_plugin_init(
            ctx: &mut $crate::plugin::PluginContext,
        ) -> $crate::value::Value {
            $crate::plugin::register_and_build(ctx, $prims, $prefix)
        }
    };
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
/// The caller is responsible for deduplication (e.g., via `is_module_loading`).
/// Calling this twice with the same path will register primitives twice and
/// leak a second library handle.
pub fn load_plugin(path: &str, vm: &mut VM, symbols: &mut SymbolTable) -> LResult<Value> {
    // Load the shared library.
    //
    // On Linux we use RTLD_GLOBAL so that the plugin's symbols are visible to
    // subsequently loaded DSOs.  This is required for C++ runtime state —
    // vtables, `std::type_info`, and global constructors — to be shared
    // correctly across plugin boundaries (e.g. when a plugin links libstdc++
    // via oxrocksdb-sys).  Without it, each DSO gets its own copy of C++
    // runtime globals, breaking dynamic_cast and RTTI.
    //
    // Note: if dlopen fails with "cannot allocate memory in static TLS block",
    // that is a separate glibc static-TLS-reservation issue and is NOT fixed by
    // RTLD_GLOBAL.  main.rs handles this by setting GLIBC_TUNABLES and
    // re-execing before we get here.
    #[cfg(unix)]
    let lib = {
        use libloading::os::unix::Library as UnixLibrary;
        unsafe { UnixLibrary::open(Some(path), libc::RTLD_NOW | libc::RTLD_GLOBAL) }
            .map(libloading::Library::from)
            .map_err(|e| LError::generic(format!("failed to load plugin '{}': {}", path, e)))?
    };
    #[cfg(not(unix))]
    let lib = unsafe { libloading::Library::new(path) }
        .map_err(|e| LError::generic(format!("failed to load plugin '{}': {}", path, e)))?;

    // Look up the init function
    let init_fn: libloading::Symbol<PluginInitFn> = unsafe { lib.get(b"elle_plugin_init") }
        .map_err(|e| {
            LError::generic(format!("plugin '{}' missing elle_plugin_init: {}", path, e))
        })?;

    // Call init to collect primitive definitions
    let mut ctx = PluginContext::new();
    let return_value = unsafe { init_fn(&mut ctx) };

    // Register collected primitives into the VM's docs.
    //
    // Note: plugin signals and arities are NOT registered in PrimitiveMeta
    // because plugins are loaded at runtime (via `import-file`), after the
    // static analyzer has already processed the calling code. The analyzer
    // will see plugin primitives as unknown locals, not as primitives with
    // known signals or arities. This is the same limitation as any runtime
    // import — a pre-existing constraint, not a plugin-specific gap.
    for def in &ctx.primitives {
        let _sym_id = symbols.intern(def.name);

        let doc = Doc {
            name: def.name,
            doc: def.doc,
            params: def.params,
            arity: def.arity,
            signal: def.signal,
            category: def.category,
            example: def.example,
            aliases: def.aliases,
        };
        vm.docs.insert(def.name.to_string(), doc.clone());

        for alias in def.aliases {
            let _alias_id = symbols.intern(alias);
            vm.docs.insert((*alias).to_string(), doc.clone());
        }
    }

    // Leak the library handle — never unload plugins
    std::mem::forget(lib);

    Ok(return_value)
}
