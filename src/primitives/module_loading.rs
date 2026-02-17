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
            std::fs::read_to_string(path_str)
                .map_err(|e| {
                    Condition::error(format!("import-file: failed to read '{}': {}", path_str, e))
                })
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
                                tokens.push(crate::reader::OwnedToken::from(token_with_loc.token));
                                locations.push(token_with_loc.loc);
                            }
                            Ok(None) => break,
                            Err(e) => {
                                return Err(Condition::error(format!(
                                    "import-file: failed to tokenize '{}': {}",
                                    path_str, e
                                )))
                            }
                        }
                    }

                    if tokens.is_empty() {
                        return Ok(Value::bool(true));
                    }

                    // Read and execute all forms
                    let mut reader = crate::reader::Reader::with_locations(tokens, locations);
                    while let Some(result) = reader.try_read(symbols) {
                        match result {
                            Ok(value) => {
                                // Compile and execute using the caller's symbol table
                                let expr = crate::compiler::value_to_expr(&value, symbols)
                                    .map_err(|e| {
                                        Condition::error(format!(
                                            "import-file: failed to compile '{}': {}",
                                            path_str, e
                                        ))
                                    })?;
                                let bytecode = crate::compile(&expr);
                                vm.execute(&bytecode).map_err(|e| {
                                    Condition::error(format!(
                                        "import-file: failed to execute '{}': {}",
                                        path_str, e
                                    ))
                                })?;
                            }
                            Err(e) => {
                                return Err(Condition::error(format!(
                                    "import-file: failed to parse '{}': {}",
                                    path_str, e
                                )));
                            }
                        }
                    }

                    Ok(Value::bool(true))
                })
                .map(|_| Value::bool(true))
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
