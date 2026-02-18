use crate::value::{Condition, Value};

/// Import a module file
pub fn prim_import_file(args: &[Value]) -> Result<Value, Condition> {
    // (import-file "path/to/module.elle")
    // Loads and compiles a .elle file as a module
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "import-file: expected 1 argument, got {}",
            args.len()
        )));
    }

    if let Some(path) = args[0].as_string() {
        // Get VM context for file loading
        let vm_ptr = crate::ffi_primitives::get_vm_context().ok_or_else(|| {
            Condition::error("import-file: VM context not initialized".to_string())
        })?;

        unsafe {
            let vm = &mut *vm_ptr;

            // Check for circular dependencies
            if vm.is_module_loaded(path) {
                return Ok(Value::bool(true));
            }

            // Mark as loaded to prevent circular dependency
            vm.mark_module_loaded(path.to_string());

            // Get the caller's symbol table context
            let symbols_ptr =
                crate::ffi_primitives::context::get_symbol_table().ok_or_else(|| {
                    Condition::error(
                        "import-file: symbol table context not initialized".to_string(),
                    )
                })?;

            // Read and compile the file
            let path_str = path;
            let contents = std::fs::read_to_string(path_str).map_err(|e| {
                Condition::error(format!("import-file: failed to read '{}': {}", path_str, e))
            })?;

            // Compile all forms using the new pipeline
            let symbols = &mut *symbols_ptr;
            let results = crate::pipeline::compile_all_new(&contents, symbols).map_err(|e| {
                Condition::error(format!(
                    "import-file: compilation error in {}: {}",
                    path_str, e
                ))
            })?;

            // Execute each compiled form sequentially
            for result in &results {
                vm.execute(&result.bytecode).map_err(|e| {
                    Condition::error(format!("import-file: runtime error in {}: {}", path_str, e))
                })?;
            }

            Ok(Value::bool(true))
        }
    } else {
        Err(Condition::type_error(format!(
            "import-file: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Add a directory to the module search path
pub fn prim_add_module_path(args: &[Value]) -> Result<Value, Condition> {
    // (add-module-path "path")
    // Adds a directory to the module search path
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "add-module-path: expected 1 argument, got {}",
            args.len()
        )));
    }

    if let Some(path) = args[0].as_string() {
        // Get VM context
        let vm_ptr = crate::ffi_primitives::get_vm_context().ok_or_else(|| {
            Condition::error("add-module-path: VM context not initialized".to_string())
        })?;

        unsafe {
            let vm = &mut *vm_ptr;
            vm.add_module_search_path(std::path::PathBuf::from(path));
            Ok(Value::NIL)
        }
    } else {
        Err(Condition::type_error(format!(
            "add-module-path: expected string, got {}",
            args[0].type_name()
        )))
    }
}
