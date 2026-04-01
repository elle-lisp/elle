use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, error_val_extra, Value};
use std::path::{Path, PathBuf};

/// Resolve the Elle project root.
/// Checks `ELLE_HOME` env var first, then walks up from the binary to find `Cargo.toml`.
fn elle_root() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("ELLE_HOME") {
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

    // Virtual prefix: plugin/X → <repo-root>/target/<profile>/libelle_X.so
    // Prefer the same profile as the running binary, fallback to the other.
    if let Some(rest) = spec.strip_prefix("plugin/") {
        if let Some(root) = elle_root() {
            let profiles: &[&str] = if cfg!(debug_assertions) {
                &["debug", "release"]
            } else {
                &["release", "debug"]
            };
            for profile in profiles {
                let path = root
                    .join("target")
                    .join(profile)
                    .join(format!("libelle_{}.so", rest));
                if path.is_file() {
                    return Some(path.to_string_lossy().into_owned());
                }
            }
        }
    }

    // Fast path: already exists with the given name (full path or relative)
    if as_path.exists() {
        return Some(spec.to_string());
    }

    // Build list of directories to search
    let mut search_dirs: Vec<PathBuf> = Vec::new();

    // CWD
    if let Ok(cwd) = std::env::current_dir() {
        search_dirs.push(cwd);
    }

    // ELLE_PATH (colon-separated)
    if let Ok(elle_path) = std::env::var("ELLE_PATH") {
        for entry in elle_path.split(':') {
            let p = PathBuf::from(entry);
            if p.is_dir() {
                search_dirs.push(p);
            }
        }
    }

    // ELLE_HOME (default: directory of the elle binary)
    let elle_home = std::env::var("ELLE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
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

        // Try <dir>/<spec_dir>/libelle_<leaf>.so  (plugin convention)
        let so_name = format!("libelle_{}.so", leaf);
        let plugin_in_dir = dir
            .join(as_path.parent().unwrap_or(Path::new("")))
            .join(&so_name);
        if plugin_in_dir.is_file() {
            return Some(plugin_in_dir.to_string_lossy().into_owned());
        }

        // Try <dir>/libelle_<leaf>.so  (flat layout)
        let plugin_flat = dir.join(&so_name);
        if plugin_flat.is_file() {
            return Some(plugin_flat.to_string_lossy().into_owned());
        }
    }

    None
}

/// Import a module file
pub(crate) fn prim_import_file(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("import: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let spec = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("import: expected string, got {}", args[0].type_name()),
            ),
        );
    };

    let path = match resolve_import(&spec) {
        Some(p) => p,
        None => {
            return (
                SIG_ERROR,
                error_val_extra(
                    "io-error",
                    format!("import: module '{}' not found", spec),
                    &[("spec", Value::string(spec.as_str()))],
                ),
            );
        }
    };

    // Get VM context for file loading
    let vm_ptr = match crate::context::get_vm_context() {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "internal-error",
                    "import: VM context not initialized".to_string(),
                ),
            );
        }
    };

    unsafe {
        let vm = &mut *vm_ptr;

        // Detect circular imports (module currently being loaded)
        if vm.is_module_loading(&path) {
            return (
                SIG_ERROR,
                error_val_extra(
                    "io-error",
                    format!("import: circular dependency detected for '{}'", path),
                    &[("path", Value::string(path.as_str()))],
                ),
            );
        }

        // Mark as loading for circular-import detection
        vm.mark_module_loading(path.clone());

        // Get the caller's symbol table context
        let symbols_ptr = match crate::context::get_symbol_table() {
            Some(ptr) => ptr,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "internal-error",
                        "import: symbol table context not initialized".to_string(),
                    ),
                );
            }
        };

        let symbols = &mut *symbols_ptr;

        // Plugin loading for .so files
        if path.ends_with(".so") {
            // Return cached value if already loaded (avoids re-registering primitives)
            if let Some(&cached) = vm.loaded_plugins.get(&path) {
                vm.unmark_module_loading(&path);
                return (SIG_OK, cached);
            }
            let result = match crate::plugin::load_plugin(&path, vm, symbols) {
                Ok(value) => {
                    vm.loaded_plugins.insert(path.clone(), value);
                    (SIG_OK, value)
                }
                Err(e) => (
                    SIG_ERROR,
                    error_val_extra(
                        "io-error",
                        format!("import: {}", e),
                        &[("path", Value::string(path.as_str()))],
                    ),
                ),
            };
            vm.unmark_module_loading(&path);
            return result;
        }

        // Elle source file loading
        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val_extra(
                        "io-error",
                        format!("import: failed to read '{}': {}", path, e),
                        &[("path", Value::string(path.as_str()))],
                    ),
                );
            }
        };

        let result = match crate::pipeline::compile_file(&contents, symbols, &path) {
            Ok(r) => r,
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val_extra(
                        "eval-error",
                        format!("import: compilation error in {}: {}", path, e),
                        &[("path", Value::string(path.as_str()))],
                    ),
                );
            }
        };

        // Save/restore the caller's stack. import executes the
        // module's bytecode on the same VM, which would overwrite the
        // caller's local variable slots without this protection.
        vm.location_map = result.bytecode.location_map.clone();
        let bc_rc = std::rc::Rc::new(result.bytecode.instructions);
        let consts_rc = std::rc::Rc::new(result.bytecode.constants);
        let location_map_rc = std::rc::Rc::new(vm.location_map.clone());
        let empty_env = std::rc::Rc::new(vec![]);

        let exec_result =
            vm.execute_bytecode_saving_stack(&bc_rc, &consts_rc, &empty_env, &location_map_rc);

        // Unmark loading regardless of outcome
        vm.unmark_module_loading(&path);

        match exec_result.bits {
            SIG_OK => {
                let (_, value) = vm
                    .fiber
                    .signal
                    .take()
                    .unwrap_or((SIG_OK, crate::value::Value::NIL));
                (SIG_OK, value)
            }
            SIG_ERROR => {
                let (_, err_value) = vm
                    .fiber
                    .signal
                    .take()
                    .unwrap_or((SIG_ERROR, crate::value::Value::NIL));
                let msg = vm.format_error_with_location(err_value);
                (
                    SIG_ERROR,
                    error_val_extra(
                        "eval-error",
                        format!("import: runtime error in {}: {}", path, msg),
                        &[("path", Value::string(path.as_str()))],
                    ),
                )
            }
            bits => (
                SIG_ERROR,
                error_val_extra(
                    "eval-error",
                    format!("import: unexpected signal {} in {}", bits, path),
                    &[("path", Value::string(path.as_str()))],
                ),
            ),
        }
    }
}

/// Declarative primitive definitions for module loading operations
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[PrimitiveDef {
    name: "import",
    func: prim_import_file,
    signal: Signal::errors(),
    arity: Arity::Exact(1),
    doc: "Import a module by specifier. Resolves via search paths (CWD, ELLE_PATH, ELLE_HOME) with extension probing (.lisp, libelle_<name>.so).",
    params: &["spec"],
    category: "",
    example: "(import \"lib/http\")",
    aliases: &["import-file", "module/import"],
}];
