use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;
use std::path::{Path, PathBuf};

/// Check whether a file path has a native shared library extension.
fn is_native_library(path: &str) -> bool {
    path.ends_with(".so") || path.ends_with(".dylib") || path.ends_with(".dll")
}

/// Resolve the Elle project root.
/// Checks `--home` config first, then walks up from the binary to find `Cargo.toml`.
fn elle_root() -> Option<PathBuf> {
    if let Some(home) = &crate::config::get().home {
        let p = PathBuf::from(home);
        if p.is_dir() {
            return Some(p);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?;
    // Walk up until we find Cargo.toml
    loop {
        if dir.join("Cargo.toml").is_file() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

/// Resolve a module specifier to a concrete file path.
pub(crate) fn resolve_import(spec: &str) -> Option<String> {
    let as_path = Path::new(spec);

    // Virtual prefix: std/X → <repo-root>/lib/X.lisp
    if let Some(rest) = spec.strip_prefix("std/") {
        if let Some(root) = elle_root() {
            let path = root.join("lib").join(format!("{}.lisp", rest));
            if path.is_file() {
                return Some(path.to_string_lossy().into_owned());
            }
        }
    }

    // Virtual prefix: plugin/X → <repo-root>/target/<profile>/libelle_X.{so,dylib,dll}
    // Prefer the same profile as the running binary, fallback to the other.
    if let Some(rest) = spec.strip_prefix("plugin/") {
        if let Some(root) = elle_root() {
            let profiles: &[&str] = if cfg!(debug_assertions) {
                &["debug", "release"]
            } else {
                &["release", "debug"]
            };
            let ext = std::env::consts::DLL_EXTENSION;
            for profile in profiles {
                let path = root
                    .join("target")
                    .join(profile)
                    .join(format!("libelle_{}.{}", rest, ext));
                if path.is_file() {
                    return Some(path.to_string_lossy().into_owned());
                }
            }
        }
    }

    // Fast path: already exists as a file (skip directories — no semantics for them)
    if as_path.is_file() {
        return Some(spec.to_string());
    }

    // Build list of directories to search
    let mut search_dirs: Vec<PathBuf> = Vec::new();

    // CWD
    if let Ok(cwd) = std::env::current_dir() {
        search_dirs.push(cwd);
    }

    // --path (colon-separated)
    if let Some(elle_path) = &crate::config::get().path {
        for entry in elle_path.split(':') {
            let p = PathBuf::from(entry);
            if p.is_dir() {
                search_dirs.push(p);
            }
        }
    }

    // --home (default: directory of the elle binary)
    let elle_home = crate::config::get()
        .home
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .unwrap_or_default()
        });
    if elle_home.is_dir() {
        search_dirs.push(elle_home);
    }

    // Derive the leaf name for plugin probing: "plugin/glob" → "glob"
    let leaf = as_path.file_name().and_then(|n| n.to_str()).unwrap_or(spec);
    let ext = std::env::consts::DLL_EXTENSION;

    for dir in &search_dirs {
        // Try <dir>/<spec>.lisp
        let lisp = dir.join(format!("{}.lisp", spec));
        if lisp.is_file() {
            return Some(lisp.to_string_lossy().into_owned());
        }

        // Try <dir>/<spec> as-is (without extension, in case it exists in a search dir)
        let bare = dir.join(spec);
        if bare.is_file() {
            return Some(bare.to_string_lossy().into_owned());
        }

        // Try <dir>/<spec_dir>/libelle_<leaf>.{so,dylib,dll}  (plugin convention)
        let lib_name = format!("libelle_{}.{}", leaf, ext);
        let plugin_in_dir = dir
            .join(as_path.parent().unwrap_or(Path::new("")))
            .join(&lib_name);
        if plugin_in_dir.is_file() {
            return Some(plugin_in_dir.to_string_lossy().into_owned());
        }

        // Try <dir>/libelle_<leaf>.{so,dylib,dll}  (flat layout)
        let plugin_flat = dir.join(&lib_name);
        if plugin_flat.is_file() {
            return Some(plugin_flat.to_string_lossy().into_owned());
        }
    }

    None
}

/// Mint the caller's owning reference for a plugin value `import` hands along.
///
/// `import` declares [`result_minted`](crate::primitives::def::PrimitiveDef::result_minted),
/// so `dispatch_native_call` takes no pass-through retain for it; on the
/// `.lisp` path the module body's return mint is that reference, and on the
/// plugin paths — which run no thunk — this retain is. It balances the
/// caller's `DecrefValueRegion` exactly as the dispatch retain would have,
/// leaving the plugin cache's own reference untouched.
fn retain_plugin_result(vm: &mut crate::vm::VM, value: Value) {
    let heap = unsafe { &mut *vm.heap_ptr };
    let region = crate::value::arena::region_of(heap, value);
    crate::value::arena::incref_for_escape(
        heap,
        region,
        crate::value::arena::EscapeSite::NativeCallResult,
    );
}

/// Import a module file
pub(crate) fn prim_import_file(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let spec = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return type_error!(ctx, args[0], "import", "string");
    };

    let path = match resolve_import(&spec) {
        Some(p) => p,
        None => {
            return crate::rich_error!(
                ctx,
                "io-error",
                format!("import: module '{}' not found", spec),
                spec = ctx.string(spec.as_str()),
            );
        }
    };

    // The driving VM loads the module: `execute_bytecode_saving_stack` runs the
    // module's bytecode on it (preserving the caller's stack). Reached as a raw
    // pointer so the `vm.*` calls and the `ctx.*` allocations below — which use the
    // disjoint heap — coexist; `ctx.vm()` is total (a native always runs under a VM).
    let vm_ptr: *mut crate::vm::VM = ctx.vm();

    unsafe {
        let vm = &mut *vm_ptr;

        // Detect circular imports (module currently being loaded)
        if vm.is_module_loading(&path) {
            return crate::rich_error!(
                ctx,
                "io-error",
                format!("import: circular dependency detected for '{}'", path),
                path = ctx.string(path.as_str()),
            );
        }

        // Mark as loading for circular-import detection
        vm.mark_module_loading(path.clone());

        // The caller's symbol table, reached through the driving VM (this
        // instance's own table).
        let symbols_ptr = vm.symbols_ptr;
        if symbols_ptr.is_null() {
            return (
                SIG_ERROR,
                ctx.error(
                    "internal-error",
                    "import: symbol table context not initialized".to_string(),
                ),
            );
        }

        let symbols = &mut *symbols_ptr;

        // Plugin loading for native shared libraries (.so, .dylib, .dll)
        if is_native_library(&path) {
            // Return cached value if already loaded (avoids re-registering primitives)
            if let Some(&cached) = vm.loaded_plugins.get(&path) {
                vm.unmark_module_loading(&path);
                // `import` declares `result_minted`, so the dispatch retain is
                // skipped; this call did not run a thunk to produce the cached
                // value, so mint the caller's reference here (the retain the
                // dispatch would have taken for a pass-through result).
                retain_plugin_result(vm, cached);
                return (SIG_OK, cached);
            }
            let result = match crate::plugin::load_plugin(&path, vm, symbols) {
                Ok(value) => {
                    vm.loaded_plugins.insert(path.clone(), value);
                    retain_plugin_result(vm, value);
                    (SIG_OK, value)
                }
                Err(e) => crate::rich_error!(
                    ctx,
                    "io-error",
                    format!("import: {}", e),
                    path = ctx.string(path.as_str()),
                ),
            };
            vm.unmark_module_loading(&path);
            return result;
        }

        // Elle source file loading — fall back to plugin loading on UTF-8 failure
        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                // File exists but isn't valid UTF-8 — try loading as a plugin
                let result = match crate::plugin::load_plugin(&path, vm, symbols) {
                    Ok(value) => {
                        vm.loaded_plugins.insert(path.clone(), value);
                        retain_plugin_result(vm, value);
                        (SIG_OK, value)
                    }
                    Err(plugin_err) => crate::rich_error!(
                        ctx,
                        "io-error",
                        format!(
                            "import: '{}' is not valid Elle source ({}), \
                             and plugin loading also failed: {}",
                            path, e, plugin_err
                        ),
                        path = ctx.string(path.as_str()),
                    ),
                };
                vm.unmark_module_loading(&path);
                return result;
            }
            Err(e) => {
                vm.unmark_module_loading(&path);
                return crate::rich_error!(
                    ctx,
                    "io-error",
                    format!("import: failed to read '{}': {}", path, e),
                    path = ctx.string(path.as_str()),
                );
            }
        };

        // Compile the module in this instance's compile context, reached through
        // the executing VM. The borrow ends with the match.
        let compiled = match vm.compile_ctx() {
            Some(cctx) => crate::pipeline::compile_file(&contents, symbols, cctx, &path),
            None => Err("import: compile context unavailable".to_string()),
        };
        let result = match compiled {
            Ok(r) => r,
            Err(e) => {
                return crate::rich_error!(
                    ctx,
                    "eval-error",
                    format!("import: compilation error in {}: {}", path, e),
                    path = ctx.string(path.as_str()),
                );
            }
        };

        // Save/restore the caller's stack. import executes the
        // module's bytecode on the same VM, which would overwrite the
        // caller's local variable slots without this protection.
        let code = crate::value::Code::new(
            std::rc::Rc::new(result.bytecode.instructions),
            std::rc::Rc::new(result.bytecode.constants),
            std::rc::Rc::new(result.bytecode.location_map),
            std::rc::Rc::new(result.bytecode.child_protos),
        );
        let empty_env = std::rc::Rc::new(vec![]);

        // Drive the module's top-level forms to completion, draining any
        // nested fiber/resume SIG_SWITCH trampoline — a module's forms run as
        // part of the CURRENT fiber's execution (like `eval`'s thunk), so a
        // top-level `protect`/`fiber/resume` returns SIG_SWITCH that must be
        // drained here rather than leaked out of the import boundary. Using the
        // raw executor reported that internal signal as "unexpected".
        let bits = vm.run_thunk_to_completion(&code, &empty_env);

        // Unmark loading regardless of outcome
        vm.unmark_module_loading(&path);

        match bits {
            SIG_OK => {
                let (_, value) = vm
                    .fiber
                    .signal
                    .take()
                    .unwrap_or((SIG_OK, crate::value::Value::NIL));
                // The module value left its compiled top level through the
                // return convention, so it already carries the one owed
                // reference the caller's release consumes — the
                // `result_minted` declaration's claim on this path. The plugin
                // paths above return a value no thunk minted, and take the
                // reference explicitly (`retain_plugin_result`).
                (SIG_OK, value)
            }
            SIG_ERROR => {
                let (_, err_value) = vm
                    .fiber
                    .signal
                    .take()
                    .unwrap_or((SIG_ERROR, crate::value::Value::NIL));
                let msg = vm.format_error_with_location(err_value);
                crate::rich_error!(
                    ctx,
                    "eval-error",
                    format!("import: runtime error in {}: {}", path, msg),
                    path = ctx.string(path.as_str()),
                )
            }
            bits => crate::rich_error!(
                ctx,
                "eval-error",
                format!("import: unexpected signal {} in {}", bits, path),
                path = ctx.string(path.as_str()),
            ),
        }
    }
}

// Declarative primitive definitions for module loading operations
primitive! {
    "import" => prim_import_file {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Import a module by specifier. Resolves via search paths (CWD, --path, --home) with extension probing (.lisp, native plugins). Binary files that fail UTF-8 reading are automatically tried as plugins.",
        params: &["spec"],
        example: "(import \"std/http\")",
        aliases: &["import-file", "module/import"],
        effect: RegionEffect::Mixed,
        result_minted: true,
    }
}
