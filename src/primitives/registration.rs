use crate::ffi_primitives;
use crate::symbol::SymbolTable;
use crate::value::Value;
use crate::vm::VM;

use super::arithmetic::{
    prim_abs, prim_add, prim_div, prim_even, prim_max, prim_min, prim_mod, prim_mul, prim_odd,
    prim_rem, prim_sub,
};
use super::comparison::{prim_eq, prim_ge, prim_gt, prim_le, prim_lt};
use super::concurrency::{prim_current_thread_id, prim_join, prim_sleep, prim_spawn};
use super::coroutines::{
    prim_coroutine_done, prim_coroutine_next, prim_coroutine_resume, prim_coroutine_status,
    prim_coroutine_to_iterator, prim_coroutine_value, prim_is_coroutine, prim_make_coroutine,
    prim_yield_from,
};
use super::debug::{prim_debug_print, prim_memory_usage, prim_profile, prim_trace};
use super::display::{prim_display, prim_newline};
use super::exception::{prim_exception, prim_exception_data, prim_exception_message, prim_throw};
use super::file_io::{
    prim_absolute_path, prim_append_file, prim_change_directory, prim_copy_file,
    prim_create_directory, prim_create_directory_all, prim_current_directory,
    prim_delete_directory, prim_delete_file, prim_file_exists, prim_file_extension, prim_file_name,
    prim_file_size, prim_is_directory, prim_is_file, prim_join_path, prim_list_directory,
    prim_parent_directory, prim_read_lines, prim_rename_file, prim_slurp, prim_spit,
};
use super::introspection::{
    prim_condition_backtrace, prim_condition_field, prim_condition_matches_type, prim_exception_id,
};
use super::jit::{prim_jit_compilable_p, prim_jit_compile, prim_jit_compiled_p, prim_jit_stats};
use super::json::{prim_json_parse, prim_json_serialize, prim_json_serialize_pretty};
use super::list::{
    prim_append, prim_cons, prim_drop, prim_empty, prim_first, prim_last, prim_length, prim_list,
    prim_nth, prim_rest, prim_reverse, prim_take,
};
use super::logic::{prim_and, prim_not, prim_or, prim_xor};
use super::macros::{prim_expand_macro, prim_is_macro};
use super::math::{
    prim_ceil, prim_cos, prim_e, prim_exp, prim_floor, prim_log, prim_pi, prim_pow, prim_round,
    prim_sin, prim_sqrt, prim_tan,
};
use super::meta::prim_gensym;
use super::module_loading::{prim_add_module_path, prim_import_file};
use super::package::{prim_package_info, prim_package_version};
use super::process::prim_exit;
use super::signaling::{prim_error, prim_signal, prim_warn};
use super::string::{
    prim_any_to_string, prim_char_at, prim_number_to_string, prim_string_append,
    prim_string_contains, prim_string_downcase, prim_string_ends_with, prim_string_index,
    prim_string_join, prim_string_length, prim_string_replace, prim_string_split,
    prim_string_starts_with, prim_string_to_float, prim_string_to_int, prim_string_trim,
    prim_string_upcase, prim_substring, prim_to_float, prim_to_int, prim_to_string,
};
use super::structs::{
    prim_struct, prim_struct_del, prim_struct_get, prim_struct_has, prim_struct_keys,
    prim_struct_length, prim_struct_put, prim_struct_values,
};
use super::table::{
    prim_get, prim_has_key, prim_keys, prim_table, prim_table_del, prim_table_length,
    prim_table_put, prim_values,
};
use super::type_check::{
    prim_is_boolean, prim_is_nil, prim_is_number, prim_is_pair, prim_is_string, prim_is_symbol,
    prim_type,
};
use super::vector::{prim_vector, prim_vector_length, prim_vector_ref, prim_vector_set};

/// Register all primitive functions with the VM
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
    register_fn(vm, symbols, "boolean?", prim_is_boolean);

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
    register_fn(vm, symbols, "empty?", prim_empty);
    register_fn(vm, symbols, "append", prim_append);
    register_fn(vm, symbols, "reverse", prim_reverse);

    // Type conversions
    register_fn(vm, symbols, "int", prim_to_int);
    register_fn(vm, symbols, "float", prim_to_float);
    register_fn(vm, symbols, "string", prim_to_string);
    // Scheme-style conversion names
    register_fn(vm, symbols, "string->int", prim_string_to_int);
    register_fn(vm, symbols, "string->float", prim_string_to_float);
    register_fn(vm, symbols, "any->string", prim_any_to_string);

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

    // Table operations (mutable) - now using polymorphic versions
    register_fn(vm, symbols, "table", prim_table);
    register_fn(vm, symbols, "get", prim_get); // Now polymorphic
    register_fn(vm, symbols, "put", prim_table_put); // Table-specific (mutates)
    register_fn(vm, symbols, "put!", prim_table_put); // Explicit mutation marker
    register_fn(vm, symbols, "del", prim_table_del); // Table-specific (mutates)
    register_fn(vm, symbols, "del!", prim_table_del); // Explicit mutation marker
    register_fn(vm, symbols, "keys", prim_keys); // Now polymorphic
    register_fn(vm, symbols, "values", prim_values); // Now polymorphic
    register_fn(vm, symbols, "has-key?", prim_has_key); // Now polymorphic
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
    register_fn(vm, symbols, "rem", prim_rem);
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

    // Exception handling (old string-based)
    register_fn(vm, symbols, "throw", prim_throw);
    register_fn(vm, symbols, "exception", prim_exception);
    register_fn(vm, symbols, "exception-message", prim_exception_message);
    register_fn(vm, symbols, "exception-data", prim_exception_data);

    // Condition system (new CL-style)
    register_fn(vm, symbols, "signal", prim_signal);
    register_fn(vm, symbols, "warn", prim_warn);
    register_fn(vm, symbols, "error", prim_error);

    // Exception introspection (Phase 8)
    register_fn(vm, symbols, "exception-id", prim_exception_id);
    register_fn(vm, symbols, "condition-field", prim_condition_field);
    register_fn(
        vm,
        symbols,
        "condition-matches-type",
        prim_condition_matches_type,
    );
    register_fn(vm, symbols, "condition-backtrace", prim_condition_backtrace);

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

    // Process control
    register_fn(vm, symbols, "exit", prim_exit);

    // Debugging and profiling primitives
    register_fn(vm, symbols, "debug-print", prim_debug_print);
    register_fn(vm, symbols, "trace", prim_trace);
    register_fn(vm, symbols, "profile", prim_profile);
    register_fn(vm, symbols, "memory-usage", prim_memory_usage);

    // File I/O primitives
    register_fn(vm, symbols, "slurp", prim_slurp);
    register_fn(vm, symbols, "spit", prim_spit);
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

    // JIT compilation
    register_fn(vm, symbols, "jit-compile", prim_jit_compile);
    register_fn(vm, symbols, "jit-compiled?", prim_jit_compiled_p);
    register_fn(vm, symbols, "jit-compilable?", prim_jit_compilable_p);
    register_fn(vm, symbols, "jit-stats", prim_jit_stats);

    // Coroutine primitives (Phase 6)
    register_fn(vm, symbols, "make-coroutine", prim_make_coroutine);
    register_fn(vm, symbols, "coroutine-status", prim_coroutine_status);
    register_fn(vm, symbols, "coroutine-done?", prim_coroutine_done);
    register_fn(vm, symbols, "coroutine-value", prim_coroutine_value);
    register_fn(vm, symbols, "coroutine?", prim_is_coroutine);

    // VM-aware coroutine primitives (Phase 6)
    register_vm_aware_fn(vm, symbols, "coroutine-resume", prim_coroutine_resume);
    register_vm_aware_fn(vm, symbols, "yield-from", prim_yield_from);
    register_fn(
        vm,
        symbols,
        "coroutine->iterator",
        prim_coroutine_to_iterator,
    );
    register_vm_aware_fn(vm, symbols, "coroutine-next", prim_coroutine_next);
}

/// Register a primitive function with the VM
fn register_fn(
    vm: &mut VM,
    symbols: &mut SymbolTable,
    name: &str,
    func: fn(&[Value]) -> Result<Value, String>,
) {
    let sym_id = symbols.intern(name);
    vm.set_global(sym_id.0, Value::NativeFn(func));
}

/// Register a VM-aware primitive function with the VM
fn register_vm_aware_fn(
    vm: &mut VM,
    symbols: &mut SymbolTable,
    name: &str,
    func: fn(&[Value], &mut VM) -> Result<Value, String>,
) {
    let sym_id = symbols.intern(name);
    vm.set_global(sym_id.0, Value::VmAwareFn(func));
}
