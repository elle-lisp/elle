use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Import a module file
pub fn prim_import_file(args: &[Value]) -> (SignalBits, Value) {
    // (import-file "path/to/module.elle")
    // Loads and compiles a .elle file as a module
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

    // Get VM context for file loading
    let vm_ptr = match crate::context::get_vm_context() {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    "import-file: VM context not initialized".to_string(),
                ),
            );
        }
    };

    unsafe {
        let vm = &mut *vm_ptr;

        // Check for circular dependencies
        if vm.is_module_loaded(&path) {
            return (SIG_OK, Value::bool(true));
        }

        // Mark as loaded to prevent circular dependency
        vm.mark_module_loaded(path.clone());

        // Get the caller's symbol table context
        let symbols_ptr = match crate::context::get_symbol_table() {
            Some(ptr) => ptr,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        "import-file: symbol table context not initialized".to_string(),
                    ),
                );
            }
        };

        let symbols = &mut *symbols_ptr;

        // Plugin loading for .so files
        if path.ends_with(".so") {
            return match crate::plugin::load_plugin(&path, vm, symbols) {
                Ok(()) => (SIG_OK, Value::bool(true)),
                Err(e) => (SIG_ERROR, error_val("error", format!("import-file: {}", e))),
            };
        }

        // Elle source file loading
        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!("import-file: failed to read '{}': {}", path, e),
                    ),
                );
            }
        };

        let results = match crate::pipeline::compile_all(&contents, symbols) {
            Ok(r) => r,
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!("import-file: compilation error in {}: {}", path, e),
                    ),
                );
            }
        };

        let mut last_value = Value::NIL;
        for result in &results {
            match vm.execute(&result.bytecode) {
                Ok(v) => last_value = v,
                Err(e) => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "error",
                            format!("import-file: runtime error in {}: {}", path, e),
                        ),
                    );
                }
            }
        }

        (SIG_OK, last_value)
    }
}

/// Add a directory to the module search path
pub fn prim_add_module_path(args: &[Value]) -> (SignalBits, Value) {
    // (add-module-path "path")
    // Adds a directory to the module search path
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("add-module-path: expected 1 argument, got {}", args.len()),
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
                format!(
                    "add-module-path: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        );
    };

    // Get VM context
    let vm_ptr = match crate::context::get_vm_context() {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    "add-module-path: VM context not initialized".to_string(),
                ),
            );
        }
    };

    unsafe {
        let vm = &mut *vm_ptr;
        vm.add_module_search_path(std::path::PathBuf::from(path));
        (SIG_OK, Value::NIL)
    }
}

/// Declarative primitive definitions for module loading operations
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "module/import",
        func: prim_import_file,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Import a module file and execute it in the current context",
        params: &["path"],
        category: "module",
        example: "(module/import \"lib/utils.elle\")",
        aliases: &["import-file", "import"],
    },
    PrimitiveDef {
        name: "module/add-path",
        func: prim_add_module_path,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Add a directory to the module search path",
        params: &["path"],
        category: "module",
        example: "(module/add-path \"./lib\")",
        aliases: &["add-module-path"],
    },
];
