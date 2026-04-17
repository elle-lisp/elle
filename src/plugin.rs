//! Plugin loading for dynamically-linked Rust libraries.
//!
//! Plugins are `.so` files (cdylib crates) that depend on `elle-plugin`
//! (not on `elle`). They export an `elle_plugin_init` function that
//! receives an `ElleApiLoader` + `EllePluginCtx`, resolves API functions
//! by name, and registers primitives.
//!
//! The ABI is stable: plugins can be compiled separately from elle and
//! loaded at runtime without version matching.

use crate::error::{LError, LResult};
use crate::plugin_api::{self, ApiLoader, PrimDefRaw};
use crate::primitives::def::{Doc, PrimitiveDef};
use crate::symbol::SymbolTable;
use crate::value::Value;
use crate::vm::VM;

/// Plugin init function signature.
///
/// The plugin receives the API loader (for resolving functions by name)
/// and a registration context (for declaring primitives). Returns 0 on
/// success, nonzero on failure.
type PluginInitFn = extern "C" fn(loader: &ApiLoader, ctx: &mut PluginCtx) -> i32;

// ── Registration context ──────────────────────────────────────────────

/// Plugin registration context. Layout-compatible with `EllePluginCtx`
/// in elle-plugin.
#[repr(C)]
struct PluginCtx {
    register: extern "C" fn(ctx: *mut PluginCtx, def: *const PrimDefRaw),
    collected: *mut Vec<&'static PrimitiveDef>,
}

// Compile-time layout verification: PluginCtx must be exactly two pointers.
const _: () = assert!(std::mem::size_of::<PluginCtx>() == 2 * std::mem::size_of::<usize>());

extern "C" fn register_prim(ctx_ptr: *mut PluginCtx, def_ptr: *const PrimDefRaw) {
    let ctx = unsafe { &mut *ctx_ptr };
    let collected = unsafe { &mut *ctx.collected };
    let raw = unsafe { &*def_ptr };
    let def = unsafe { plugin_api::raw_def_to_primitive(raw) };
    collected.push(def);
}

// ── Plugin loading ────────────────────────────────────────────────────

/// Load a plugin `.so` and register its primitives.
///
/// The library handle is intentionally leaked — plugins are never unloaded.
/// This avoids use-after-free if Elle code holds values created by the plugin.
///
/// The caller is responsible for deduplication (e.g., via `is_module_loading`).
pub fn load_plugin(path: &str, vm: &mut VM, symbols: &mut SymbolTable) -> LResult<Value> {
    use crate::value::types::TableKey;
    use std::collections::BTreeMap;

    // Load the shared library.
    //
    // On Linux we use RTLD_GLOBAL so that the plugin's symbols are visible to
    // subsequently loaded DSOs.  This is required for C++ runtime state —
    // vtables, `std::type_info`, and global constructors — to be shared
    // correctly across plugin boundaries (e.g. when a plugin links libstdc++
    // via oxrocksdb-sys).  Without it, each DSO gets its own copy of C++
    // runtime globals, breaking dynamic_cast and RTTI.
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
    let init_fn: libloading::Symbol<PluginInitFn> = unsafe { lib.get(b"elle_plugin_init\0") }
        .map_err(|e| {
            LError::generic(format!("plugin '{}' missing elle_plugin_init: {}", path, e))
        })?;

    // Build API loader and registration context
    let loader = plugin_api::build_api_loader();
    let mut collected: Vec<&'static PrimitiveDef> = Vec::new();
    let mut ctx = PluginCtx {
        register: register_prim,
        collected: &mut collected,
    };

    // Call plugin init
    let rc = init_fn(&loader, &mut ctx);
    if rc != 0 {
        return Err(LError::generic(format!(
            "plugin '{}' init failed with code {}",
            path, rc
        )));
    }

    // Build the module struct from collected definitions.
    let prefix = if let Some(first) = collected.first() {
        if let Some(pos) = first.name.rfind('/') {
            &first.name[..=pos]
        } else {
            ""
        }
    } else {
        ""
    };

    let mut fields = BTreeMap::new();
    for def in &collected {
        let short_name = def.name.strip_prefix(prefix).unwrap_or(def.name);
        fields.insert(TableKey::Keyword(short_name.into()), Value::native_fn(def));
    }

    // Register docs
    for def in &collected {
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

    Ok(Value::struct_from(fields))
}
