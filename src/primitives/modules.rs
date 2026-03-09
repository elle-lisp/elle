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

        // Detect circular imports (module currently being loaded)
        if vm.is_module_loading(&path) {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("import-file: circular dependency detected for '{}'", path),
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
                Ok(value) => (SIG_OK, value),
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

        let result = match crate::pipeline::compile_file(&contents, symbols) {
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

        // Save/restore the caller's stack. import-file executes the
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
                    error_val(
                        "error",
                        format!("import-file: runtime error in {}: {}", path, msg),
                    ),
                )
            }
            bits => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("import-file: unexpected signal {} in {}", bits, path),
                ),
            ),
        }
    }
}

/// Declarative primitive definitions for module loading operations
pub const PRIMITIVES: &[PrimitiveDef] = &[PrimitiveDef {
    name: "module/import",
    func: prim_import_file,
    effect: Effect::errors(),
    arity: Arity::Exact(1),
    doc: "Import a module file and execute it in the current context",
    params: &["path"],
    category: "module",
    example: "(module/import \"lib/utils.elle\")",
    aliases: &["import-file", "import"],
}];
