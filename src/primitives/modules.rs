use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, error_val_extra, Value};

/// Load a module by resolved filesystem path.
///
/// Shared by both `import` (after resolution) and `import-file` (direct).
/// Handles circular-import detection, plugin loading, source compilation,
/// and source_file_stack management.
fn load_module(path: &str) -> (SignalBits, Value) {
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

        // Canonicalize for circular-import detection
        let canonical = crate::path::canonicalize(path).unwrap_or_else(|_| path.to_string());

        if vm.is_module_loading(&canonical) {
            return (
                SIG_ERROR,
                error_val_extra(
                    "io-error",
                    format!("import: circular dependency detected for '{}'", path),
                    &[("path", Value::string(path))],
                ),
            );
        }

        vm.mark_module_loading(canonical.clone());

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

        // Plugin loading for shared libraries
        if path.ends_with(".so") || path.ends_with(".dylib") {
            let result = match crate::plugin::load_plugin(path, vm, symbols) {
                Ok(value) => (SIG_OK, value),
                Err(e) => (
                    SIG_ERROR,
                    error_val_extra(
                        "io-error",
                        format!("import: {}", e),
                        &[("path", Value::string(path))],
                    ),
                ),
            };
            vm.unmark_module_loading(&canonical);
            return result;
        }

        // Elle source file loading
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                vm.unmark_module_loading(&canonical);
                return (
                    SIG_ERROR,
                    error_val_extra(
                        "io-error",
                        format!("import: failed to read '{}': {}", path, e),
                        &[("path", Value::string(path))],
                    ),
                );
            }
        };

        let result = match crate::pipeline::compile_file(&contents, symbols, path) {
            Ok(r) => r,
            Err(e) => {
                vm.unmark_module_loading(&canonical);
                return (
                    SIG_ERROR,
                    error_val_extra(
                        "eval-error",
                        format!("import: compilation error in {}: {}", path, e),
                        &[("path", Value::string(path))],
                    ),
                );
            }
        };

        // Push source file for relative import resolution
        vm.source_file_stack.push(path.to_string());

        vm.location_map = result.bytecode.location_map.clone();
        let bc_rc = std::rc::Rc::new(result.bytecode.instructions);
        let consts_rc = std::rc::Rc::new(result.bytecode.constants);
        let location_map_rc = std::rc::Rc::new(vm.location_map.clone());
        let empty_env = std::rc::Rc::new(vec![]);

        let exec_result =
            vm.execute_bytecode_saving_stack(&bc_rc, &consts_rc, &empty_env, &location_map_rc);

        vm.source_file_stack.pop();
        vm.unmark_module_loading(&canonical);

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
                        &[("path", Value::string(path))],
                    ),
                )
            }
            bits => (
                SIG_ERROR,
                error_val_extra(
                    "eval-error",
                    format!("import: unexpected signal {} in {}", bits, path),
                    &[("path", Value::string(path))],
                ),
            ),
        }
    }
}

/// Smart import with module resolution.
///
/// Resolves bare names via ELLE_PATH, relative paths via caller file,
/// then delegates to `load_module`.
pub(crate) fn prim_import(args: &[Value]) -> (SignalBits, Value) {
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

    // Get caller directory from source_file_stack
    let caller_dir = unsafe {
        crate::context::get_vm_context().and_then(|ptr| {
            let vm = &*ptr;
            vm.source_file_stack
                .last()
                .and_then(|f| crate::path::parent(f))
                .map(|s| s.to_string())
        })
    };

    let search_dirs = crate::resolve::search_path();

    match crate::resolve::resolve(&spec, caller_dir.as_deref(), &search_dirs) {
        Some(resolved) => load_module(&resolved),
        None => {
            let dirs_display = if search_dirs.is_empty() {
                "(no search directories configured)".to_string()
            } else {
                search_dirs.join(", ")
            };
            (
                SIG_ERROR,
                error_val_extra(
                    "io-error",
                    format!(
                        "import: module '{}' not found (searched: {})",
                        spec, dirs_display
                    ),
                    &[("name", Value::string(spec.as_str()))],
                ),
            )
        }
    }
}

/// Import a native plugin by name.
///
/// Only searches for `libelle_NAME.{so,dylib}` in ELLE_PATH directories.
pub(crate) fn prim_import_native(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("import-native: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let name = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "import-native: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        );
    };

    let search_dirs = crate::resolve::search_path();

    match crate::resolve::resolve_native(&name, &search_dirs) {
        Some(resolved) => load_module(&resolved),
        None => {
            let dirs_display = if search_dirs.is_empty() {
                "(no search directories configured)".to_string()
            } else {
                search_dirs.join(", ")
            };
            (
                SIG_ERROR,
                error_val_extra(
                    "io-error",
                    format!(
                        "import-native: plugin '{}' not found (searched: {})",
                        name, dirs_display
                    ),
                    &[("name", Value::string(name.as_str()))],
                ),
            )
        }
    }
}

/// Import a module by exact filesystem path (no resolution).
pub(crate) fn prim_import_file(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("import-file: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let path = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("import-file: expected string, got {}", args[0].type_name()),
            ),
        );
    };

    load_module(&path)
}

/// Declarative primitive definitions for module loading operations
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "import",
        func: prim_import,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Import a module by name, resolving via ELLE_PATH",
        params: &["name"],
        category: "",
        example: "(import \"regex\")",
        aliases: &["module/import"],
    },
    PrimitiveDef {
        name: "import-native",
        func: prim_import_native,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Import a native plugin by name, searching ELLE_PATH for libelle_NAME.{so,dylib}",
        params: &["name"],
        category: "",
        example: "(import-native \"regex\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "import-file",
        func: prim_import_file,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Import a module by exact filesystem path",
        params: &["path"],
        category: "",
        example: "(import-file \"lib/utils.elle\")",
        aliases: &[],
    },
];
