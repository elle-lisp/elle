use crate::symbol::SymbolTable;
use crate::value::Value;

/// Import a module file
pub fn prim_import_file(args: &[Value]) -> Result<Value, String> {
    // (import-file "path/to/module.elle")
    // Loads and compiles a .elle file as a module
    if args.len() != 1 {
        return Err(format!(
            "import-file: expected 1 argument, got {}",
            args.len()
        ));
    }

    match &args[0] {
        Value::String(path) => {
            // Get VM context for file loading
            let vm_ptr = crate::ffi_primitives::get_vm_context()
                .ok_or("VM context not initialized for module loading")?;

            unsafe {
                let vm = &mut *vm_ptr;

                // Check for circular dependencies
                if vm.is_module_loaded(path) {
                    return Ok(Value::Bool(true));
                }

                // Mark as loaded to prevent circular dependency
                vm.mark_module_loaded(path.to_string());

                // Read and compile the file
                let path_str = path.as_ref();
                std::fs::read_to_string(path_str)
                    .map_err(|e| format!("Failed to read module file '{}': {}", path_str, e))
                    .and_then(|contents| {
                        // Parse the module
                        let mut symbols = SymbolTable::new();
                        crate::read_str(&contents, &mut symbols)
                            .map_err(|e| format!("Failed to parse module '{}': {}", path_str, e))
                            .and_then(|value| {
                                // Compile and execute
                                let expr = crate::compiler::value_to_expr(&value, &mut symbols)
                                    .map_err(|e| {
                                        format!("Failed to compile module '{}': {}", path_str, e)
                                    })?;
                                let bytecode = crate::compile(&expr);
                                vm.execute(&bytecode).map_err(|e| {
                                    format!("Failed to execute module '{}': {}", path_str, e)
                                })
                            })
                    })
                    .map(|_| Value::Bool(true))
            }
        }
        _ => Err("import-file: argument must be a string".to_string()),
    }
}

/// Add a directory to the module search path
pub fn prim_add_module_path(args: &[Value]) -> Result<Value, String> {
    // (add-module-path "path")
    // Adds a directory to the module search path
    if args.len() != 1 {
        return Err(format!(
            "add-module-path: expected 1 argument, got {}",
            args.len()
        ));
    }

    match &args[0] {
        Value::String(path) => {
            // Get VM context
            let vm_ptr = crate::ffi_primitives::get_vm_context()
                .ok_or("VM context not initialized for module loading")?;

            unsafe {
                let vm = &mut *vm_ptr;
                vm.add_module_search_path(std::path::PathBuf::from(path.as_ref()));
                Ok(Value::Nil)
            }
        }
        _ => Err("add-module-path: argument must be a string".to_string()),
    }
}
