use crate::effects::Effect;
use crate::ffi_primitives;
use crate::symbol::SymbolTable;
use crate::value::{SymbolId, Value};
use crate::vm::VM;
use std::collections::HashMap;

use super::arithmetic::{
    prim_abs, prim_add, prim_div_vm, prim_even, prim_max, prim_min, prim_mod, prim_mul, prim_odd,
    prim_rem, prim_sub,
};
use super::cell::{prim_box, prim_box_p, prim_box_set, prim_unbox};
use super::comparison::{prim_eq, prim_ge, prim_gt, prim_le, prim_lt};
use super::concurrency::{prim_current_thread_id, prim_join, prim_sleep, prim_spawn};
use super::coroutines::{
    prim_coroutine_done, prim_coroutine_resume, prim_coroutine_status, prim_coroutine_to_iterator,
    prim_coroutine_value, prim_is_coroutine, prim_make_coroutine, prim_yield_from,
};
use super::debug::{prim_debug_print, prim_memory_usage, prim_profile, prim_trace};
use super::debugging::{
    prim_arity, prim_bytecode_size, prim_captures, prim_disbit, prim_disjit, prim_is_closure,
    prim_is_coro, prim_is_jit, prim_is_pure, prim_mutates_params, prim_raises,
};
use super::display::{prim_display, prim_newline, prim_print};

use super::fibers::{
    prim_fiber_bits, prim_fiber_cancel, prim_fiber_child, prim_fiber_mask, prim_fiber_new,
    prim_fiber_parent, prim_fiber_propagate, prim_fiber_resume, prim_fiber_signal,
    prim_fiber_status, prim_fiber_value, prim_is_fiber,
};
use super::file_io::{
    prim_absolute_path, prim_append_file, prim_change_directory, prim_copy_file,
    prim_create_directory, prim_create_directory_all, prim_current_directory,
    prim_delete_directory, prim_delete_file, prim_file_exists, prim_file_extension, prim_file_name,
    prim_file_size, prim_is_directory, prim_is_file, prim_join_path, prim_list_directory,
    prim_parent_directory, prim_read_lines, prim_rename_file, prim_slurp, prim_spit,
};

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

use super::string::{
    prim_any_to_string, prim_char_at, prim_keyword_to_string, prim_number_to_string,
    prim_string_append, prim_string_contains, prim_string_downcase, prim_string_ends_with,
    prim_string_index, prim_string_join, prim_string_replace, prim_string_split,
    prim_string_starts_with, prim_string_to_float, prim_string_to_int, prim_string_trim,
    prim_string_upcase, prim_substring, prim_symbol_to_string, prim_to_float, prim_to_int,
    prim_to_string,
};
use super::structs::{prim_struct, prim_struct_del};
use super::table::{
    prim_del, prim_get, prim_has_key, prim_keys, prim_put, prim_table, prim_values,
};
use super::type_check::{
    prim_is_boolean, prim_is_keyword, prim_is_list, prim_is_nil, prim_is_number, prim_is_pair,
    prim_is_string, prim_is_symbol, prim_type_of,
};
use super::vector::{prim_vector, prim_vector_ref, prim_vector_set};

/// Register all primitive functions with the VM.
/// Returns a map of primitive effects for use by the analyzer.
pub fn register_primitives(vm: &mut VM, symbols: &mut SymbolTable) -> HashMap<SymbolId, Effect> {
    let mut effects = HashMap::new();

    // Arithmetic - all can raise (arity/type errors)
    register_fn(vm, symbols, &mut effects, "+", prim_add, Effect::raises());
    register_fn(vm, symbols, &mut effects, "-", prim_sub, Effect::raises());
    register_fn(vm, symbols, &mut effects, "*", prim_mul, Effect::raises());
    register_fn(
        vm,
        symbols,
        &mut effects,
        "/",
        prim_div_vm,
        Effect::raises(),
    );

    // Comparisons - can raise on type errors
    register_fn(vm, symbols, &mut effects, "=", prim_eq, Effect::raises());
    register_fn(vm, symbols, &mut effects, "eq?", prim_eq, Effect::raises()); // Alias for =
    register_fn(vm, symbols, &mut effects, "<", prim_lt, Effect::raises());
    register_fn(vm, symbols, &mut effects, ">", prim_gt, Effect::raises());
    register_fn(vm, symbols, &mut effects, "<=", prim_le, Effect::raises());
    register_fn(vm, symbols, &mut effects, ">=", prim_ge, Effect::raises());

    // List operations
    register_fn(vm, symbols, &mut effects, "cons", prim_cons, Effect::none());
    register_fn(
        vm,
        symbols,
        &mut effects,
        "first",
        prim_first,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "rest",
        prim_rest,
        Effect::raises(),
    );
    register_fn(vm, symbols, &mut effects, "list", prim_list, Effect::none());

    // Type predicates - all pure
    register_fn(
        vm,
        symbols,
        &mut effects,
        "nil?",
        prim_is_nil,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "pair?",
        prim_is_pair,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "list?",
        prim_is_list,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "number?",
        prim_is_number,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "symbol?",
        prim_is_symbol,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string?",
        prim_is_string,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "boolean?",
        prim_is_boolean,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "keyword?",
        prim_is_keyword,
        Effect::none(),
    );

    // Logic - pure
    register_fn(vm, symbols, &mut effects, "not", prim_not, Effect::none());
    register_fn(vm, symbols, &mut effects, "and", prim_and, Effect::none());
    register_fn(vm, symbols, &mut effects, "or", prim_or, Effect::none());
    register_fn(vm, symbols, &mut effects, "xor", prim_xor, Effect::none());

    // Display - pure (no yield)
    register_fn(
        vm,
        symbols,
        &mut effects,
        "display",
        prim_display,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "print",
        prim_print,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "newline",
        prim_newline,
        Effect::none(),
    );

    // Additional list operations
    register_fn(
        vm,
        symbols,
        &mut effects,
        "length",
        prim_length,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "empty?",
        prim_empty,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "append",
        prim_append,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "reverse",
        prim_reverse,
        Effect::raises(),
    );

    // Type conversions - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "int",
        prim_to_int,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "float",
        prim_to_float,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string",
        prim_to_string,
        Effect::raises(),
    );
    // Scheme-style conversion names
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string->int",
        prim_string_to_int,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string->float",
        prim_string_to_float,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "any->string",
        prim_any_to_string,
        Effect::raises(),
    );

    // Min/Max - can raise
    register_fn(vm, symbols, &mut effects, "min", prim_min, Effect::raises());
    register_fn(vm, symbols, &mut effects, "max", prim_max, Effect::raises());

    // Absolute value - can raise
    register_fn(vm, symbols, &mut effects, "abs", prim_abs, Effect::raises());

    // String operations - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-append",
        prim_string_append,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-upcase",
        prim_string_upcase,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-downcase",
        prim_string_downcase,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "substring",
        prim_substring,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-index",
        prim_string_index,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "char-at",
        prim_char_at,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-split",
        prim_string_split,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-replace",
        prim_string_replace,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-trim",
        prim_string_trim,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-contains?",
        prim_string_contains,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-starts-with?",
        prim_string_starts_with,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-ends-with?",
        prim_string_ends_with,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-join",
        prim_string_join,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "number->string",
        prim_number_to_string,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "symbol->string",
        prim_symbol_to_string,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "keyword->string",
        prim_keyword_to_string,
        Effect::raises(),
    );

    // List utilities - can raise
    register_fn(vm, symbols, &mut effects, "nth", prim_nth, Effect::raises());
    register_fn(
        vm,
        symbols,
        &mut effects,
        "last",
        prim_last,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "take",
        prim_take,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "drop",
        prim_drop,
        Effect::raises(),
    );

    // Vector operations - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "vector",
        prim_vector,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "vector-ref",
        prim_vector_ref,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "vector-set!",
        prim_vector_set,
        Effect::raises(),
    );

    // Table/Struct operations (polymorphic) - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "table",
        prim_table,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "struct",
        prim_struct,
        Effect::none(),
    );
    register_fn(vm, symbols, &mut effects, "get", prim_get, Effect::raises());
    register_fn(vm, symbols, &mut effects, "put", prim_put, Effect::raises());
    register_fn(
        vm,
        symbols,
        &mut effects,
        "put!",
        prim_put,
        Effect::raises(),
    );
    register_fn(vm, symbols, &mut effects, "del", prim_del, Effect::raises());
    register_fn(
        vm,
        symbols,
        &mut effects,
        "del!",
        prim_del,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "keys",
        prim_keys,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "values",
        prim_values,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "has-key?",
        prim_has_key,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "struct-del",
        prim_struct_del,
        Effect::raises(),
    );

    // Type info - pure
    register_fn(
        vm,
        symbols,
        &mut effects,
        "type-of",
        prim_type_of,
        Effect::none(),
    );

    // Math functions - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "sqrt",
        prim_sqrt,
        Effect::raises(),
    );
    register_fn(vm, symbols, &mut effects, "sin", prim_sin, Effect::raises());
    register_fn(vm, symbols, &mut effects, "cos", prim_cos, Effect::raises());
    register_fn(vm, symbols, &mut effects, "tan", prim_tan, Effect::raises());
    register_fn(vm, symbols, &mut effects, "log", prim_log, Effect::raises());
    register_fn(vm, symbols, &mut effects, "exp", prim_exp, Effect::raises());
    register_fn(vm, symbols, &mut effects, "pow", prim_pow, Effect::raises());
    register_fn(
        vm,
        symbols,
        &mut effects,
        "floor",
        prim_floor,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "ceil",
        prim_ceil,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "round",
        prim_round,
        Effect::raises(),
    );

    // Math constants - pure
    register_fn(vm, symbols, &mut effects, "pi", prim_pi, Effect::none());
    register_fn(vm, symbols, &mut effects, "e", prim_e, Effect::none());

    // Additional utilities - can raise
    register_fn(vm, symbols, &mut effects, "mod", prim_mod, Effect::raises());
    register_fn(vm, symbols, &mut effects, "%", prim_mod, Effect::raises());
    register_fn(vm, symbols, &mut effects, "rem", prim_rem, Effect::raises());
    register_fn(
        vm,
        symbols,
        &mut effects,
        "even?",
        prim_even,
        Effect::none(),
    );
    register_fn(vm, symbols, &mut effects, "odd?", prim_odd, Effect::none());

    // FFI primitives - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "load-library",
        ffi_primitives::prim_load_library_wrapper,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "list-libraries",
        ffi_primitives::prim_list_libraries_wrapper,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "call-c-function",
        ffi_primitives::prim_call_c_function_wrapper,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "load-header-with-lib",
        ffi_primitives::prim_load_header_with_lib_wrapper,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "define-enum",
        ffi_primitives::prim_define_enum_wrapper,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "make-c-callback",
        ffi_primitives::prim_make_c_callback_wrapper,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "free-callback",
        ffi_primitives::prim_free_callback_wrapper,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "register-allocation",
        ffi_primitives::prim_register_allocation_wrapper,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "memory-stats",
        ffi_primitives::prim_memory_stats_wrapper,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "type-check",
        ffi_primitives::prim_type_check_wrapper,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "null-pointer?",
        ffi_primitives::prim_null_pointer_wrapper,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "ffi-last-error",
        ffi_primitives::prim_ffi_last_error_wrapper,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "with-ffi-safety-checks",
        ffi_primitives::prim_with_ffi_safety_checks_wrapper,
        Effect::raises(),
    );

    // Quoting and meta-programming - pure
    register_fn(
        vm,
        symbols,
        &mut effects,
        "gensym",
        prim_gensym,
        Effect::none(),
    );

    // Package manager - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "package-version",
        prim_package_version,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "package-info",
        prim_package_info,
        Effect::raises(),
    );

    // Module loading - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "import-file",
        prim_import_file,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "add-module-path",
        prim_add_module_path,
        Effect::raises(),
    );

    // Macro expansion - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "expand-macro",
        prim_expand_macro,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "macro?",
        prim_is_macro,
        Effect::none(),
    );

    // Concurrency primitives - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "spawn",
        prim_spawn,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "join",
        prim_join,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "sleep",
        prim_sleep,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "current-thread-id",
        prim_current_thread_id,
        Effect::none(),
    );

    // Process control - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "exit",
        prim_exit,
        Effect::raises(),
    );

    // Debugging and profiling primitives - pure
    register_fn(
        vm,
        symbols,
        &mut effects,
        "debug-print",
        prim_debug_print,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "trace",
        prim_trace,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "profile",
        prim_profile,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "memory-usage",
        prim_memory_usage,
        Effect::none(),
    );

    // Closure introspection - pure
    register_fn(
        vm,
        symbols,
        &mut effects,
        "closure?",
        prim_is_closure,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "jit?",
        prim_is_jit,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "pure?",
        prim_is_pure,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "coro?",
        prim_is_coro,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "mutates-params?",
        prim_mutates_params,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "raises?",
        prim_raises,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "arity",
        prim_arity,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "captures",
        prim_captures,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "bytecode-size",
        prim_bytecode_size,
        Effect::none(),
    );

    // Bytecode and JIT disassembly - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "disbit",
        prim_disbit,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "disjit",
        prim_disjit,
        Effect::raises(),
    );

    // File I/O primitives - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "slurp",
        prim_slurp,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "spit",
        prim_spit,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "append-file",
        prim_append_file,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "file-exists?",
        prim_file_exists,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "directory?",
        prim_is_directory,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "file?",
        prim_is_file,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "delete-file",
        prim_delete_file,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "delete-directory",
        prim_delete_directory,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "create-directory",
        prim_create_directory,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "create-directory-all",
        prim_create_directory_all,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "rename-file",
        prim_rename_file,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "copy-file",
        prim_copy_file,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "file-size",
        prim_file_size,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "list-directory",
        prim_list_directory,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "absolute-path",
        prim_absolute_path,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "current-directory",
        prim_current_directory,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "change-directory",
        prim_change_directory,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "join-path",
        prim_join_path,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "file-extension",
        prim_file_extension,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "file-name",
        prim_file_name,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "parent-directory",
        prim_parent_directory,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "read-lines",
        prim_read_lines,
        Effect::raises(),
    );

    // JSON operations - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "json-parse",
        prim_json_parse,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "json-serialize",
        prim_json_serialize,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "json-serialize-pretty",
        prim_json_serialize_pretty,
        Effect::raises(),
    );

    // Cell/Box primitives (mutable storage)
    register_fn(vm, symbols, &mut effects, "box", prim_box, Effect::none());
    register_fn(
        vm,
        symbols,
        &mut effects,
        "unbox",
        prim_unbox,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "box-set!",
        prim_box_set,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "box?",
        prim_box_p,
        Effect::none(),
    );

    // Coroutine primitives (Phase 6)
    register_fn(
        vm,
        symbols,
        &mut effects,
        "make-coroutine",
        prim_make_coroutine,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "coroutine-status",
        prim_coroutine_status,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "coroutine-done?",
        prim_coroutine_done,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "coroutine-value",
        prim_coroutine_value,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "coroutine?",
        prim_is_coroutine,
        Effect::none(),
    );

    // Coroutine primitives - return SIG_RESUME for VM to handle
    register_fn(
        vm,
        symbols,
        &mut effects,
        "coroutine-resume",
        prim_coroutine_resume,
        Effect::yields_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "yield-from",
        prim_yield_from,
        Effect::yields_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "coroutine->iterator",
        prim_coroutine_to_iterator,
        Effect::raises(),
    );
    // Fiber primitives
    register_fn(
        vm,
        symbols,
        &mut effects,
        "fiber/new",
        prim_fiber_new,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "fiber/resume",
        prim_fiber_resume,
        Effect::yields_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "fiber/signal",
        prim_fiber_signal,
        Effect::yields_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "fiber/status",
        prim_fiber_status,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "fiber/value",
        prim_fiber_value,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "fiber/bits",
        prim_fiber_bits,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "fiber/mask",
        prim_fiber_mask,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "fiber?",
        prim_is_fiber,
        Effect::none(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "fiber/parent",
        prim_fiber_parent,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "fiber/child",
        prim_fiber_child,
        Effect::raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "fiber/propagate",
        prim_fiber_propagate,
        Effect::yields_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "fiber/cancel",
        prim_fiber_cancel,
        Effect::raises(),
    );

    effects
}

/// Register a primitive function with the VM
fn register_fn(
    vm: &mut VM,
    symbols: &mut SymbolTable,
    effects: &mut HashMap<SymbolId, Effect>,
    name: &str,
    func: fn(&[Value]) -> (crate::value::fiber::SignalBits, Value),
    effect: Effect,
) {
    let sym_id = symbols.intern(name);
    vm.set_global(sym_id.0, Value::native_fn(func));
    effects.insert(sym_id, effect);
}
