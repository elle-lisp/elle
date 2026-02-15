use crate::error::{LError, LResult};
use crate::value::Value;

/// Import a module file
pub fn prim_import_file(args: &[Value]) -> LResult<Value> {
    // (import-file "path/to/module.elle")
    // Loads and compiles a .elle file as a module
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }

    match &args[0] {
        Value::String(path) => {
            // Get VM context for file loading
            let vm_ptr = crate::ffi_primitives::get_vm_context().ok_or_else(|| {
                LError::runtime_error("VM context not initialized for module loading")
            })?;

            unsafe {
                let vm = &mut *vm_ptr;

                // Check for circular dependencies
                if vm.is_module_loaded(path) {
                    return Ok(Value::Bool(true));
                }

                // Mark as loaded to prevent circular dependency
                vm.mark_module_loaded(path.to_string());

                // Get the caller's symbol table context
                let symbols_ptr =
                    crate::ffi_primitives::context::get_symbol_table().ok_or_else(|| {
                        LError::runtime_error(
                            "Symbol table context not initialized for module loading",
                        )
                    })?;

                // Read and compile the file
                let path_str = path.as_ref();
                std::fs::read_to_string(path_str)
                    .map_err(|e| LError::file_read_error(path_str, e.to_string()))
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
                                    return Err(LError::runtime_error(format!(
                                        "Failed to tokenize module '{}': {}",
                                        path_str, e
                                    )))
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
                                            LError::compile_error(format!(
                                                "Failed to compile module '{}': {}",
                                                path_str, e
                                            ))
                                        })?;
                                    let bytecode = crate::compile(&expr);
                                    vm.execute(&bytecode).map_err(|e| {
                                        LError::runtime_error(format!(
                                            "Failed to execute module '{}': {}",
                                            path_str, e
                                        ))
                                    })?;
                                }
                                Err(e) => {
                                    return Err(LError::runtime_error(format!(
                                        "Failed to parse module '{}': {}",
                                        path_str, e
                                    )));
                                }
                            }
                        }

                        Ok(Value::Bool(true))
                    })
                    .map(|_| Value::Bool(true))
            }
        }
        _ => Err(LError::type_mismatch("string", args[0].type_name())),
    }
}

/// Add a directory to the module search path
pub fn prim_add_module_path(args: &[Value]) -> LResult<Value> {
    // (add-module-path "path")
    // Adds a directory to the module search path
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }

    match &args[0] {
        Value::String(path) => {
            // Get VM context
            let vm_ptr = crate::ffi_primitives::get_vm_context().ok_or_else(|| {
                LError::runtime_error("VM context not initialized for module loading")
            })?;

            unsafe {
                let vm = &mut *vm_ptr;
                vm.add_module_search_path(std::path::PathBuf::from(path.as_ref()));
                Ok(Value::Nil)
            }
        }
        _ => Err(LError::type_mismatch("string", args[0].type_name())),
    }
}
