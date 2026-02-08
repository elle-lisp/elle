pub mod arithmetic;
pub mod comparison;
pub mod concurrency;
pub mod debug;
pub mod exception;
pub mod file_io;
pub mod higher_order;
pub mod json;
pub mod list;
pub mod math;
pub mod meta;
pub mod string;
pub mod structs;
pub mod table;
pub mod type_check;
pub mod utility;
pub mod vector;

use crate::ffi_primitives;
use crate::symbol::SymbolTable;
use crate::value::Value;
use crate::vm::VM;

use self::arithmetic::{
    prim_abs, prim_add, prim_div, prim_even, prim_max, prim_min, prim_mod, prim_mul, prim_odd,
    prim_remainder, prim_sub,
};
use self::comparison::{prim_eq, prim_ge, prim_gt, prim_le, prim_lt};
use self::concurrency::{prim_current_thread_id, prim_join, prim_sleep, prim_spawn};
use self::debug::{prim_debug_print, prim_memory_usage, prim_profile, prim_trace};
use self::exception::{prim_exception, prim_exception_data, prim_exception_message, prim_throw};
use self::file_io::{
    prim_absolute_path, prim_append_file, prim_change_directory, prim_copy_file,
    prim_create_directory, prim_create_directory_all, prim_current_directory,
    prim_delete_directory, prim_delete_file, prim_file_exists, prim_file_extension, prim_file_name,
    prim_file_size, prim_is_directory, prim_is_file, prim_join_path, prim_list_directory,
    prim_parent_directory, prim_read_file, prim_read_lines, prim_rename_file, prim_write_file,
};
// Higher-order functions (map, filter, fold) are now defined in Lisp in init_stdlib
use self::json::{prim_json_parse, prim_json_serialize, prim_json_serialize_pretty};
use self::list::{
    prim_append, prim_cons, prim_drop, prim_first, prim_last, prim_length, prim_list, prim_nth,
    prim_rest, prim_reverse, prim_take,
};
use self::math::{
    prim_ceil, prim_cos, prim_e, prim_exp, prim_floor, prim_log, prim_pi, prim_pow, prim_round,
    prim_sin, prim_sqrt, prim_tan,
};
use self::meta::prim_gensym;
use self::string::{
    prim_char_at, prim_number_to_string, prim_string_append, prim_string_contains,
    prim_string_downcase, prim_string_ends_with, prim_string_index, prim_string_join,
    prim_string_length, prim_string_replace, prim_string_split, prim_string_starts_with,
    prim_string_trim, prim_string_upcase, prim_substring, prim_to_float, prim_to_int,
    prim_to_string,
};
use self::structs::{
    prim_struct, prim_struct_del, prim_struct_get, prim_struct_has, prim_struct_keys,
    prim_struct_length, prim_struct_put, prim_struct_values,
};
use self::table::{
    prim_table, prim_table_del, prim_table_get, prim_table_has, prim_table_keys, prim_table_length,
    prim_table_put, prim_table_values,
};
use self::type_check::{
    prim_is_nil, prim_is_number, prim_is_pair, prim_is_string, prim_is_symbol, prim_type,
};
use self::vector::{prim_vector, prim_vector_length, prim_vector_ref, prim_vector_set};

pub fn register_primitives(vm: &mut VM, symbols: &mut SymbolTable) {
    // Arithmetic
    register_fn(vm, symbols, "+", prim_add);
    register_fn(vm, symbols, "-", prim_sub);
    register_fn(vm, symbols, "*", prim_mul);
    register_fn(vm, symbols, "/", prim_div);

    // Comparisons
    register_fn(vm, symbols, "=", prim_eq);
    register_fn(vm, symbols, "<", prim_lt);
    register_fn(vm, symbols, ">", prim_gt);
    register_fn(vm, symbols, "<=", prim_le);
    register_fn(vm, symbols, ">=", prim_ge);

    // List operations
    register_fn(vm, symbols, "cons", prim_cons);
    register_fn(vm, symbols, "first", prim_first);
    register_fn(vm, symbols, "rest", prim_rest);
    register_fn(vm, symbols, "list", prim_list);

    // Type predicates
    register_fn(vm, symbols, "nil?", prim_is_nil);
    register_fn(vm, symbols, "pair?", prim_is_pair);
    register_fn(vm, symbols, "number?", prim_is_number);
    register_fn(vm, symbols, "symbol?", prim_is_symbol);
    register_fn(vm, symbols, "string?", prim_is_string);

    // Logic
    register_fn(vm, symbols, "not", prim_not);
    register_fn(vm, symbols, "and", prim_and);
    register_fn(vm, symbols, "or", prim_or);
    register_fn(vm, symbols, "xor", prim_xor);

    // Display
    register_fn(vm, symbols, "display", prim_display);
    register_fn(vm, symbols, "newline", prim_newline);

    // Additional list operations
    register_fn(vm, symbols, "length", prim_length);
    register_fn(vm, symbols, "append", prim_append);
    register_fn(vm, symbols, "reverse", prim_reverse);
    // map, filter, fold are now defined as Lisp functions in init_stdlib to support closures

    // Type conversions
    register_fn(vm, symbols, "int", prim_to_int);
    register_fn(vm, symbols, "float", prim_to_float);
    register_fn(vm, symbols, "string", prim_to_string);

    // Min/Max
    register_fn(vm, symbols, "min", prim_min);
    register_fn(vm, symbols, "max", prim_max);

    // Absolute value
    register_fn(vm, symbols, "abs", prim_abs);

    // String operations
    register_fn(vm, symbols, "string-length", prim_string_length);
    register_fn(vm, symbols, "string-append", prim_string_append);
    register_fn(vm, symbols, "string-upcase", prim_string_upcase);
    register_fn(vm, symbols, "string-downcase", prim_string_downcase);
    register_fn(vm, symbols, "substring", prim_substring);
    register_fn(vm, symbols, "string-index", prim_string_index);
    register_fn(vm, symbols, "char-at", prim_char_at);
    register_fn(vm, symbols, "string-split", prim_string_split);
    register_fn(vm, symbols, "string-replace", prim_string_replace);
    register_fn(vm, symbols, "string-trim", prim_string_trim);
    register_fn(vm, symbols, "string-contains?", prim_string_contains);
    register_fn(vm, symbols, "string-starts-with?", prim_string_starts_with);
    register_fn(vm, symbols, "string-ends-with?", prim_string_ends_with);
    register_fn(vm, symbols, "string-join", prim_string_join);
    register_fn(vm, symbols, "number->string", prim_number_to_string);

    // List utilities
    register_fn(vm, symbols, "nth", prim_nth);
    register_fn(vm, symbols, "last", prim_last);
    register_fn(vm, symbols, "take", prim_take);
    register_fn(vm, symbols, "drop", prim_drop);

    // Vector operations
    register_fn(vm, symbols, "vector", prim_vector);
    register_fn(vm, symbols, "vector-length", prim_vector_length);
    register_fn(vm, symbols, "vector-ref", prim_vector_ref);
    register_fn(vm, symbols, "vector-set!", prim_vector_set);

    // Table operations (mutable)
    register_fn(vm, symbols, "table", prim_table);
    register_fn(vm, symbols, "get", prim_table_get);
    register_fn(vm, symbols, "put", prim_table_put);
    register_fn(vm, symbols, "del", prim_table_del);
    register_fn(vm, symbols, "keys", prim_table_keys);
    register_fn(vm, symbols, "values", prim_table_values);
    register_fn(vm, symbols, "has?", prim_table_has);
    register_fn(vm, symbols, "table-length", prim_table_length);

    // Struct operations (immutable)
    register_fn(vm, symbols, "struct", prim_struct);
    register_fn(vm, symbols, "struct-get", prim_struct_get);
    register_fn(vm, symbols, "struct-put", prim_struct_put);
    register_fn(vm, symbols, "struct-del", prim_struct_del);
    register_fn(vm, symbols, "struct-keys", prim_struct_keys);
    register_fn(vm, symbols, "struct-values", prim_struct_values);
    register_fn(vm, symbols, "struct-has?", prim_struct_has);
    register_fn(vm, symbols, "struct-length", prim_struct_length);

    // Type info
    register_fn(vm, symbols, "type", prim_type);

    // Math functions
    register_fn(vm, symbols, "sqrt", prim_sqrt);
    register_fn(vm, symbols, "sin", prim_sin);
    register_fn(vm, symbols, "cos", prim_cos);
    register_fn(vm, symbols, "tan", prim_tan);
    register_fn(vm, symbols, "log", prim_log);
    register_fn(vm, symbols, "exp", prim_exp);
    register_fn(vm, symbols, "pow", prim_pow);
    register_fn(vm, symbols, "floor", prim_floor);
    register_fn(vm, symbols, "ceil", prim_ceil);
    register_fn(vm, symbols, "round", prim_round);

    // Math constants
    register_fn(vm, symbols, "pi", prim_pi);
    register_fn(vm, symbols, "e", prim_e);

    // Additional utilities
    register_fn(vm, symbols, "mod", prim_mod);
    register_fn(vm, symbols, "%", prim_mod); // % as alias for mod
    register_fn(vm, symbols, "remainder", prim_remainder);
    register_fn(vm, symbols, "even?", prim_even);
    register_fn(vm, symbols, "odd?", prim_odd);

    // FFI primitives
    register_fn(
        vm,
        symbols,
        "load-library",
        ffi_primitives::prim_load_library_wrapper,
    );
    register_fn(
        vm,
        symbols,
        "list-libraries",
        ffi_primitives::prim_list_libraries_wrapper,
    );
    register_fn(
        vm,
        symbols,
        "call-c-function",
        ffi_primitives::prim_call_c_function_wrapper,
    );
    register_fn(
        vm,
        symbols,
        "load-header-with-lib",
        ffi_primitives::prim_load_header_with_lib_wrapper,
    );
    register_fn(
        vm,
        symbols,
        "define-enum",
        ffi_primitives::prim_define_enum_wrapper,
    );
    register_fn(
        vm,
        symbols,
        "make-c-callback",
        ffi_primitives::prim_make_c_callback_wrapper,
    );
    register_fn(
        vm,
        symbols,
        "free-callback",
        ffi_primitives::prim_free_callback_wrapper,
    );
    register_fn(
        vm,
        symbols,
        "register-allocation",
        ffi_primitives::prim_register_allocation_wrapper,
    );
    register_fn(
        vm,
        symbols,
        "memory-stats",
        ffi_primitives::prim_memory_stats_wrapper,
    );
    register_fn(
        vm,
        symbols,
        "type-check",
        ffi_primitives::prim_type_check_wrapper,
    );
    register_fn(
        vm,
        symbols,
        "null-pointer?",
        ffi_primitives::prim_null_pointer_wrapper,
    );
    register_fn(
        vm,
        symbols,
        "ffi-last-error",
        ffi_primitives::prim_ffi_last_error_wrapper,
    );
    register_fn(
        vm,
        symbols,
        "with-ffi-safety-checks",
        ffi_primitives::prim_with_ffi_safety_checks_wrapper,
    );

    // Exception handling
    register_fn(vm, symbols, "throw", prim_throw);
    register_fn(vm, symbols, "exception", prim_exception);
    register_fn(vm, symbols, "exception-message", prim_exception_message);
    register_fn(vm, symbols, "exception-data", prim_exception_data);

    // Quoting and meta-programming
    register_fn(vm, symbols, "gensym", prim_gensym);

    // Package manager
    register_fn(vm, symbols, "package-version", prim_package_version);
    register_fn(vm, symbols, "package-info", prim_package_info);

    // Module loading
    register_fn(vm, symbols, "import-file", prim_import_file);
    register_fn(vm, symbols, "add-module-path", prim_add_module_path);

    // Macro expansion
    register_fn(vm, symbols, "expand-macro", prim_expand_macro);
    register_fn(vm, symbols, "macro?", prim_is_macro);

    // Concurrency primitives
    register_fn(vm, symbols, "spawn", prim_spawn);
    register_fn(vm, symbols, "join", prim_join);
    register_fn(vm, symbols, "sleep", prim_sleep);
    register_fn(vm, symbols, "current-thread-id", prim_current_thread_id);

    // Debugging and profiling primitives
    register_fn(vm, symbols, "debug-print", prim_debug_print);
    register_fn(vm, symbols, "trace", prim_trace);
    register_fn(vm, symbols, "profile", prim_profile);
    register_fn(vm, symbols, "memory-usage", prim_memory_usage);

    // File I/O primitives
    register_fn(vm, symbols, "read-file", prim_read_file);
    register_fn(vm, symbols, "write-file", prim_write_file);
    register_fn(vm, symbols, "append-file", prim_append_file);
    register_fn(vm, symbols, "file-exists?", prim_file_exists);
    register_fn(vm, symbols, "directory?", prim_is_directory);
    register_fn(vm, symbols, "file?", prim_is_file);
    register_fn(vm, symbols, "delete-file", prim_delete_file);
    register_fn(vm, symbols, "delete-directory", prim_delete_directory);
    register_fn(vm, symbols, "create-directory", prim_create_directory);
    register_fn(
        vm,
        symbols,
        "create-directory-all",
        prim_create_directory_all,
    );
    register_fn(vm, symbols, "rename-file", prim_rename_file);
    register_fn(vm, symbols, "copy-file", prim_copy_file);
    register_fn(vm, symbols, "file-size", prim_file_size);
    register_fn(vm, symbols, "list-directory", prim_list_directory);
    register_fn(vm, symbols, "absolute-path", prim_absolute_path);
    register_fn(vm, symbols, "current-directory", prim_current_directory);
    register_fn(vm, symbols, "change-directory", prim_change_directory);
    register_fn(vm, symbols, "join-path", prim_join_path);
    register_fn(vm, symbols, "file-extension", prim_file_extension);
    register_fn(vm, symbols, "file-name", prim_file_name);
    register_fn(vm, symbols, "parent-directory", prim_parent_directory);
    register_fn(vm, symbols, "read-lines", prim_read_lines);

    // JSON operations
    register_fn(vm, symbols, "json-parse", prim_json_parse);
    register_fn(vm, symbols, "json-serialize", prim_json_serialize);
    register_fn(
        vm,
        symbols,
        "json-serialize-pretty",
        prim_json_serialize_pretty,
    );
}

fn register_fn(
    vm: &mut VM,
    symbols: &mut SymbolTable,
    name: &str,
    func: fn(&[Value]) -> Result<Value, String>,
) {
    let sym_id = symbols.intern(name);
    vm.set_global(sym_id.0, Value::NativeFn(func));
}

// Logic primitives
fn prim_not(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("not requires exactly 1 argument".to_string());
    }
    Ok(Value::Bool(!args[0].is_truthy()))
}

fn prim_and(args: &[Value]) -> Result<Value, String> {
    // (and) => true
    // (and x) => x
    // (and x y z) => z if all truthy, else first falsy
    if args.is_empty() {
        return Ok(Value::Bool(true));
    }

    for arg in &args[..args.len() - 1] {
        if !arg.is_truthy() {
            return Ok(arg.clone());
        }
    }

    Ok(args[args.len() - 1].clone())
}

fn prim_or(args: &[Value]) -> Result<Value, String> {
    // (or) => false
    // (or x) => x
    // (or x y z) => x if truthy, else next truthy or z
    if args.is_empty() {
        return Ok(Value::Bool(false));
    }

    for arg in &args[..args.len() - 1] {
        if arg.is_truthy() {
            return Ok(arg.clone());
        }
    }

    Ok(args[args.len() - 1].clone())
}

fn prim_xor(args: &[Value]) -> Result<Value, String> {
    // (xor) => false
    // (xor x) => x (as bool)
    // (xor x y z) => true if odd number of truthy values, else false
    if args.is_empty() {
        return Ok(Value::Bool(false));
    }

    let truthy_count = args.iter().filter(|v| v.is_truthy()).count();
    Ok(Value::Bool(truthy_count % 2 == 1))
}

// Display primitives
fn prim_display(args: &[Value]) -> Result<Value, String> {
    for arg in args {
        print!("{}", arg);
    }
    Ok(Value::Nil)
}

fn prim_newline(_args: &[Value]) -> Result<Value, String> {
    println!();
    Ok(Value::Nil)
}

// Standard library initialization
pub fn init_stdlib(vm: &mut VM, symbols: &mut SymbolTable) {
    // Define Lisp implementations of higher-order functions that support closures
    // These override the Rust primitives to enable closure support
    define_higher_order_functions(vm, symbols);

    init_list_module(vm, symbols);
    init_string_module(vm, symbols);
    init_math_module(vm, symbols);
    init_json_module(vm, symbols);
}

/// Define map, filter, and fold as Lisp functions that support closures
fn define_higher_order_functions(vm: &mut VM, symbols: &mut SymbolTable) {
    use crate::read_str;

    // Define map: (lambda (f lst) (if (nil? lst) nil (cons (f (first lst)) (map f (rest lst)))))
    let map_code = r#"
        (define map (lambda (f lst)
          (if (nil? lst)
            nil
            (cons (f (first lst)) (map f (rest lst))))))
    "#;

    // Define filter: (lambda (p lst) (if (nil? lst) nil (if (p (first lst)) (cons (first lst) (filter p (rest lst))) (filter p (rest lst)))))
    let filter_code = r#"
        (define filter (lambda (p lst)
          (if (nil? lst)
            nil
            (if (p (first lst))
              (cons (first lst) (filter p (rest lst)))
              (filter p (rest lst))))))
    "#;

    // Define fold: (lambda (f init lst) (if (nil? lst) init (fold f (f init (first lst)) (rest lst))))
    let fold_code = r#"
        (define fold (lambda (f init lst)
          (if (nil? lst)
            init
            (fold f (f init (first lst)) (rest lst)))))
    "#;

    // Execute each definition
    for code in &[map_code, filter_code, fold_code] {
        match read_str(code, symbols) {
            Ok(value) => {
                match crate::compiler::value_to_expr(&value, symbols) {
                    Ok(expr) => {
                        // Compile and evaluate
                        let bytecode = crate::compile(&expr);
                        if let Err(e) = vm.execute(&bytecode) {
                            eprintln!(
                                "Warning: Failed to execute higher-order function definition: {}",
                                e
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to compile higher-order function definition: {}",
                            e
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to parse higher-order function definition: {}",
                    e
                );
            }
        }
    }
}

fn init_list_module(vm: &mut VM, symbols: &mut SymbolTable) {
    // List module exports
    let mut list_exports = std::collections::HashMap::new();

    // These functions are already registered globally
    // The module just provides a namespace for them
    let functions = vec![
        "length", "append", "reverse", "map", "filter", "fold", "nth", "last", "take", "drop",
        "list", "cons", "first", "rest",
    ];

    let mut exports = Vec::new();
    for func_name in &functions {
        if let Some(func) = vm.get_global(symbols.intern(func_name).0) {
            list_exports.insert(symbols.intern(func_name).0, func.clone());
        }
        exports.push(symbols.intern(func_name));
    }

    use crate::symbol::ModuleDef;
    let list_module = ModuleDef {
        name: symbols.intern("list"),
        exports,
    };
    symbols.define_module(list_module);
    vm.define_module("list".to_string(), list_exports);
}

fn init_string_module(vm: &mut VM, symbols: &mut SymbolTable) {
    // String module exports
    let mut string_exports = std::collections::HashMap::new();

    let functions = vec![
        "string-length",
        "string-append",
        "string-upcase",
        "string-downcase",
        "substring",
        "string-index",
        "char-at",
        "string",
        "string-split",
        "string-replace",
        "string-trim",
        "string-contains?",
        "string-starts-with?",
        "string-ends-with?",
        "string-join",
        "number->string",
    ];

    let mut exports = Vec::new();
    for func_name in &functions {
        if let Some(func) = vm.get_global(symbols.intern(func_name).0) {
            string_exports.insert(symbols.intern(func_name).0, func.clone());
        }
        exports.push(symbols.intern(func_name));
    }

    use crate::symbol::ModuleDef;
    let string_module = ModuleDef {
        name: symbols.intern("string"),
        exports,
    };
    symbols.define_module(string_module);
    vm.define_module("string".to_string(), string_exports);
}

fn init_math_module(vm: &mut VM, symbols: &mut SymbolTable) {
    // Math module exports
    let mut math_exports = std::collections::HashMap::new();

    let functions = vec![
        "+",
        "-",
        "*",
        "/",
        "mod",
        "remainder",
        "abs",
        "min",
        "max",
        "sqrt",
        "sin",
        "cos",
        "tan",
        "log",
        "exp",
        "pow",
        "floor",
        "ceil",
        "round",
        "even?",
        "odd?",
        "pi",
        "e",
    ];

    let mut exports = Vec::new();
    for func_name in &functions {
        if let Some(func) = vm.get_global(symbols.intern(func_name).0) {
            math_exports.insert(symbols.intern(func_name).0, func.clone());
        }
        exports.push(symbols.intern(func_name));
    }

    use crate::symbol::ModuleDef;
    let math_module = ModuleDef {
        name: symbols.intern("math"),
        exports,
    };
    symbols.define_module(math_module);
    vm.define_module("math".to_string(), math_exports);
}

fn init_json_module(vm: &mut VM, symbols: &mut SymbolTable) {
    // JSON module exports
    let mut json_exports = std::collections::HashMap::new();

    let functions = vec!["json-parse", "json-serialize", "json-serialize-pretty"];

    let mut exports = Vec::new();
    for func_name in &functions {
        if let Some(func) = vm.get_global(symbols.intern(func_name).0) {
            json_exports.insert(symbols.intern(func_name).0, func.clone());
        }
        exports.push(symbols.intern(func_name));
    }

    use crate::symbol::ModuleDef;
    let json_module = ModuleDef {
        name: symbols.intern("json"),
        exports,
    };
    symbols.define_module(json_module);
    vm.define_module("json".to_string(), json_exports);
}

// Package manager primitives
fn prim_package_version(_args: &[Value]) -> Result<Value, String> {
    // Return current version of Elle
    Ok(Value::String("0.3.0".into()))
}

fn prim_package_info(_args: &[Value]) -> Result<Value, String> {
    // Return package information
    use crate::value::list;
    Ok(list(vec![
        Value::String("Elle".into()),
        Value::String("0.3.0".into()),
        Value::String("A Lisp interpreter with bytecode compilation".into()),
    ]))
}

// Module loading primitives
fn prim_import_file(args: &[Value]) -> Result<Value, String> {
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
                    // Module already loaded, return true (idempotent)
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

fn prim_add_module_path(args: &[Value]) -> Result<Value, String> {
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

// Macro expansion primitives
fn prim_expand_macro(args: &[Value]) -> Result<Value, String> {
    // (expand-macro macro-expr)
    // Expands a macro call and returns the expanded form
    if args.len() != 1 {
        return Err(format!(
            "expand-macro: expected 1 argument, got {}",
            args.len()
        ));
    }

    // In production, this would:
    // 1. Check if the value is a macro call (list starting with macro name)
    // 2. Look up the macro definition
    // 3. Apply the macro with arguments
    // 4. Return the expanded form
    // For Phase 5, just return the argument (placeholder)
    Ok(args[0].clone())
}

fn prim_is_macro(args: &[Value]) -> Result<Value, String> {
    // (macro? value)
    // Returns true if value is a macro
    if args.len() != 1 {
        return Err(format!("macro?: expected 1 argument, got {}", args.len()));
    }

    // In production, would check symbol table for macro definitions
    // For now, always return false
    Ok(Value::Bool(false))
}
