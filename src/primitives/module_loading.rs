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

                // Get the caller's symbol table context
                let symbols_ptr = crate::ffi_primitives::context::get_symbol_table()
                    .ok_or("Symbol table context not initialized for module loading")?;

                // Read and compile the file
                let path_str = path.as_ref();
                std::fs::read_to_string(path_str)
                    .map_err(|e| format!("Failed to read module file '{}': {}", path_str, e))
                    .and_then(|contents| {
                        // Parse all forms in the module using the caller's symbol table
                        let symbols = &mut *symbols_ptr;

                        // Tokenize the entire file
                        let mut lexer = crate::reader::Lexer::new(&contents);
                        let mut tokens = Vec::new();
                        let mut locations = Vec::new();

                        loop {
                            match lexer.next_token_with_loc() {
                                Ok(Some(token_with_loc)) => {
                                    tokens.push(crate::reader::OwnedToken::from(
                                        token_with_loc.token,
                                    ));
                                    locations.push(token_with_loc.loc);
                                }
                                Ok(None) => break,
                                Err(e) => {
                                    return Err(format!(
                                        "Failed to tokenize module '{}': {}",
                                        path_str, e
                                    ))
                                }
                            }
                        }

                        if tokens.is_empty() {
                            return Ok(Value::Bool(true));
                        }

                        // Read and execute all forms
                        let mut reader = crate::reader::Reader::with_locations(tokens, locations);
                        while let Some(result) = reader.try_read(symbols) {
                            match result {
                                Ok(value) => {
                                    // Compile and execute using the caller's symbol table
                                    let expr = crate::compiler::value_to_expr(&value, symbols)
                                        .map_err(|e| {
                                            format!(
                                                "Failed to compile module '{}': {}",
                                                path_str, e
                                            )
                                        })?;
                                    let bytecode = crate::compile(&expr);
                                    vm.execute(&bytecode).map_err(|e| {
                                        format!("Failed to execute module '{}': {}", path_str, e)
                                    })?;
                                }
                                Err(e) => {
                                    return Err(format!(
                                        "Failed to parse module '{}': {}",
                                        path_str, e
                                    ));
                                }
                            }
                        }

                        Ok(Value::Bool(true))
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
