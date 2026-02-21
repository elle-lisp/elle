use crate::effects::Effect;
use crate::error::LResult;
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
    prim_coroutine_done, prim_coroutine_next, prim_coroutine_resume, prim_coroutine_status,
    prim_coroutine_to_iterator, prim_coroutine_value, prim_is_coroutine, prim_make_coroutine,
    prim_yield_from,
};
use super::debug::{prim_debug_print, prim_memory_usage, prim_profile, prim_trace};
use super::debugging::{
    prim_arity, prim_bytecode_size, prim_captures, prim_disbit, prim_disjit, prim_is_closure,
    prim_is_coro, prim_is_jit, prim_is_pure, prim_mutates_params, prim_raises,
};
use super::display::{prim_display, prim_newline, prim_print};
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
    prim_string_join, prim_string_replace, prim_string_split, prim_string_starts_with,
    prim_string_to_float, prim_string_to_int, prim_string_trim, prim_string_upcase, prim_substring,
    prim_symbol_to_string, prim_to_float, prim_to_int, prim_to_string,
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
    register_fn(
        vm,
        symbols,
        &mut effects,
        "+",
        prim_add,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "-",
        prim_sub,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "*",
        prim_mul,
        Effect::pure_raises(),
    );
    register_vm_aware_fn(
        vm,
        symbols,
        &mut effects,
        "/",
        prim_div_vm,
        Effect::pure_raises(),
    );

    // Comparisons - can raise on type errors
    register_fn(
        vm,
        symbols,
        &mut effects,
        "=",
        prim_eq,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "eq?",
        prim_eq,
        Effect::pure_raises(),
    ); // Alias for =
    register_fn(
        vm,
        symbols,
        &mut effects,
        "<",
        prim_lt,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        ">",
        prim_gt,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "<=",
        prim_le,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        ">=",
        prim_ge,
        Effect::pure_raises(),
    );

    // List operations
    register_fn(vm, symbols, &mut effects, "cons", prim_cons, Effect::pure());
    register_fn(
        vm,
        symbols,
        &mut effects,
        "first",
        prim_first,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "rest",
        prim_rest,
        Effect::pure_raises(),
    );
    register_fn(vm, symbols, &mut effects, "list", prim_list, Effect::pure());

    // Type predicates - all pure
    register_fn(
        vm,
        symbols,
        &mut effects,
        "nil?",
        prim_is_nil,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "pair?",
        prim_is_pair,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "list?",
        prim_is_list,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "number?",
        prim_is_number,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "symbol?",
        prim_is_symbol,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string?",
        prim_is_string,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "boolean?",
        prim_is_boolean,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "keyword?",
        prim_is_keyword,
        Effect::pure(),
    );

    // Logic - pure
    register_fn(vm, symbols, &mut effects, "not", prim_not, Effect::pure());
    register_fn(vm, symbols, &mut effects, "and", prim_and, Effect::pure());
    register_fn(vm, symbols, &mut effects, "or", prim_or, Effect::pure());
    register_fn(vm, symbols, &mut effects, "xor", prim_xor, Effect::pure());

    // Display - pure (no yield)
    register_fn(
        vm,
        symbols,
        &mut effects,
        "display",
        prim_display,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "print",
        prim_print,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "newline",
        prim_newline,
        Effect::pure(),
    );

    // Additional list operations
    register_fn(
        vm,
        symbols,
        &mut effects,
        "length",
        prim_length,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "empty?",
        prim_empty,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "append",
        prim_append,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "reverse",
        prim_reverse,
        Effect::pure_raises(),
    );

    // Type conversions - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "int",
        prim_to_int,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "float",
        prim_to_float,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string",
        prim_to_string,
        Effect::pure_raises(),
    );
    // Scheme-style conversion names
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string->int",
        prim_string_to_int,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string->float",
        prim_string_to_float,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "any->string",
        prim_any_to_string,
        Effect::pure_raises(),
    );

    // Min/Max - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "min",
        prim_min,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "max",
        prim_max,
        Effect::pure_raises(),
    );

    // Absolute value - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "abs",
        prim_abs,
        Effect::pure_raises(),
    );

    // String operations - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-append",
        prim_string_append,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-upcase",
        prim_string_upcase,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-downcase",
        prim_string_downcase,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "substring",
        prim_substring,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-index",
        prim_string_index,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "char-at",
        prim_char_at,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-split",
        prim_string_split,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-replace",
        prim_string_replace,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-trim",
        prim_string_trim,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-contains?",
        prim_string_contains,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-starts-with?",
        prim_string_starts_with,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-ends-with?",
        prim_string_ends_with,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "string-join",
        prim_string_join,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "number->string",
        prim_number_to_string,
        Effect::pure_raises(),
    );
    register_vm_aware_fn(
        vm,
        symbols,
        &mut effects,
        "symbol->string",
        prim_symbol_to_string,
        Effect::pure_raises(),
    );

    // List utilities - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "nth",
        prim_nth,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "last",
        prim_last,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "take",
        prim_take,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "drop",
        prim_drop,
        Effect::pure_raises(),
    );

    // Vector operations - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "vector",
        prim_vector,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "vector-ref",
        prim_vector_ref,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "vector-set!",
        prim_vector_set,
        Effect::pure_raises(),
    );

    // Table/Struct operations (polymorphic) - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "table",
        prim_table,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "struct",
        prim_struct,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "get",
        prim_get,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "put",
        prim_put,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "put!",
        prim_put,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "del",
        prim_del,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "del!",
        prim_del,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "keys",
        prim_keys,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "values",
        prim_values,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "has-key?",
        prim_has_key,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "struct-del",
        prim_struct_del,
        Effect::pure_raises(),
    );

    // Type info - pure
    register_fn(
        vm,
        symbols,
        &mut effects,
        "type-of",
        prim_type_of,
        Effect::pure(),
    );

    // Math functions - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "sqrt",
        prim_sqrt,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "sin",
        prim_sin,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "cos",
        prim_cos,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "tan",
        prim_tan,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "log",
        prim_log,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "exp",
        prim_exp,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "pow",
        prim_pow,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "floor",
        prim_floor,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "ceil",
        prim_ceil,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "round",
        prim_round,
        Effect::pure_raises(),
    );

    // Math constants - pure
    register_fn(vm, symbols, &mut effects, "pi", prim_pi, Effect::pure());
    register_fn(vm, symbols, &mut effects, "e", prim_e, Effect::pure());

    // Additional utilities - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "mod",
        prim_mod,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "%",
        prim_mod,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "rem",
        prim_rem,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "even?",
        prim_even,
        Effect::pure(),
    );
    register_fn(vm, symbols, &mut effects, "odd?", prim_odd, Effect::pure());

    // FFI primitives - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "load-library",
        ffi_primitives::prim_load_library_wrapper,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "list-libraries",
        ffi_primitives::prim_list_libraries_wrapper,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "call-c-function",
        ffi_primitives::prim_call_c_function_wrapper,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "load-header-with-lib",
        ffi_primitives::prim_load_header_with_lib_wrapper,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "define-enum",
        ffi_primitives::prim_define_enum_wrapper,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "make-c-callback",
        ffi_primitives::prim_make_c_callback_wrapper,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "free-callback",
        ffi_primitives::prim_free_callback_wrapper,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "register-allocation",
        ffi_primitives::prim_register_allocation_wrapper,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "memory-stats",
        ffi_primitives::prim_memory_stats_wrapper,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "type-check",
        ffi_primitives::prim_type_check_wrapper,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "null-pointer?",
        ffi_primitives::prim_null_pointer_wrapper,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "ffi-last-error",
        ffi_primitives::prim_ffi_last_error_wrapper,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "with-ffi-safety-checks",
        ffi_primitives::prim_with_ffi_safety_checks_wrapper,
        Effect::pure_raises(),
    );

    // Exception handling (old string-based) - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "throw",
        prim_throw,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "exception",
        prim_exception,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "exception-message",
        prim_exception_message,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "exception-data",
        prim_exception_data,
        Effect::pure_raises(),
    );

    // Condition system (new CL-style) - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "signal",
        prim_signal,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "warn",
        prim_warn,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "error",
        prim_error,
        Effect::pure_raises(),
    );

    // Exception introspection (Phase 8) - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "exception-id",
        prim_exception_id,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "condition-field",
        prim_condition_field,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "condition-matches-type",
        prim_condition_matches_type,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "condition-backtrace",
        prim_condition_backtrace,
        Effect::pure_raises(),
    );

    // Quoting and meta-programming - pure
    register_fn(
        vm,
        symbols,
        &mut effects,
        "gensym",
        prim_gensym,
        Effect::pure(),
    );

    // Package manager - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "package-version",
        prim_package_version,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "package-info",
        prim_package_info,
        Effect::pure_raises(),
    );

    // Module loading - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "import-file",
        prim_import_file,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "add-module-path",
        prim_add_module_path,
        Effect::pure_raises(),
    );

    // Macro expansion - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "expand-macro",
        prim_expand_macro,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "macro?",
        prim_is_macro,
        Effect::pure(),
    );

    // Concurrency primitives - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "spawn",
        prim_spawn,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "join",
        prim_join,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "sleep",
        prim_sleep,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "current-thread-id",
        prim_current_thread_id,
        Effect::pure(),
    );

    // Process control - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "exit",
        prim_exit,
        Effect::pure_raises(),
    );

    // Debugging and profiling primitives - pure
    register_fn(
        vm,
        symbols,
        &mut effects,
        "debug-print",
        prim_debug_print,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "trace",
        prim_trace,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "profile",
        prim_profile,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "memory-usage",
        prim_memory_usage,
        Effect::pure(),
    );

    // Closure introspection - pure
    register_fn(
        vm,
        symbols,
        &mut effects,
        "closure?",
        prim_is_closure,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "jit?",
        prim_is_jit,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "pure?",
        prim_is_pure,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "coro?",
        prim_is_coro,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "mutates-params?",
        prim_mutates_params,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "raises?",
        prim_raises,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "arity",
        prim_arity,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "captures",
        prim_captures,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "bytecode-size",
        prim_bytecode_size,
        Effect::pure(),
    );

    // Bytecode and JIT disassembly - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "disbit",
        prim_disbit,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "disjit",
        prim_disjit,
        Effect::pure_raises(),
    );

    // File I/O primitives - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "slurp",
        prim_slurp,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "spit",
        prim_spit,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "append-file",
        prim_append_file,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "file-exists?",
        prim_file_exists,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "directory?",
        prim_is_directory,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "file?",
        prim_is_file,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "delete-file",
        prim_delete_file,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "delete-directory",
        prim_delete_directory,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "create-directory",
        prim_create_directory,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "create-directory-all",
        prim_create_directory_all,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "rename-file",
        prim_rename_file,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "copy-file",
        prim_copy_file,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "file-size",
        prim_file_size,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "list-directory",
        prim_list_directory,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "absolute-path",
        prim_absolute_path,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "current-directory",
        prim_current_directory,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "change-directory",
        prim_change_directory,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "join-path",
        prim_join_path,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "file-extension",
        prim_file_extension,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "file-name",
        prim_file_name,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "parent-directory",
        prim_parent_directory,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "read-lines",
        prim_read_lines,
        Effect::pure_raises(),
    );

    // JSON operations - can raise
    register_fn(
        vm,
        symbols,
        &mut effects,
        "json-parse",
        prim_json_parse,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "json-serialize",
        prim_json_serialize,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "json-serialize-pretty",
        prim_json_serialize_pretty,
        Effect::pure_raises(),
    );

    // Cell/Box primitives (mutable storage)
    register_fn(vm, symbols, &mut effects, "box", prim_box, Effect::pure());
    register_fn(
        vm,
        symbols,
        &mut effects,
        "unbox",
        prim_unbox,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "box-set!",
        prim_box_set,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "box?",
        prim_box_p,
        Effect::pure(),
    );

    // Coroutine primitives (Phase 6)
    register_fn(
        vm,
        symbols,
        &mut effects,
        "make-coroutine",
        prim_make_coroutine,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "coroutine-status",
        prim_coroutine_status,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "coroutine-done?",
        prim_coroutine_done,
        Effect::pure(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "coroutine-value",
        prim_coroutine_value,
        Effect::pure_raises(),
    );
    register_fn(
        vm,
        symbols,
        &mut effects,
        "coroutine?",
        prim_is_coroutine,
        Effect::pure(),
    );

    // VM-aware coroutine primitives (Phase 6) - yields
    register_vm_aware_fn(
        vm,
        symbols,
        &mut effects,
        "coroutine-resume",
        prim_coroutine_resume,
        Effect::yields_raises(),
    );
    register_vm_aware_fn(
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
        Effect::pure_raises(),
    );
    register_vm_aware_fn(
        vm,
        symbols,
        &mut effects,
        "coroutine-next",
        prim_coroutine_next,
        Effect::yields_raises(),
    );

    effects
}

/// Register a primitive function with the VM
fn register_fn(
    vm: &mut VM,
    symbols: &mut SymbolTable,
    effects: &mut HashMap<SymbolId, Effect>,
    name: &str,
    func: fn(&[Value]) -> Result<Value, crate::value::Condition>,
    effect: Effect,
) {
    let sym_id = symbols.intern(name);
    vm.set_global(sym_id.0, Value::native_fn(func));
    effects.insert(sym_id, effect);
}

/// Register a VM-aware primitive function with the VM
fn register_vm_aware_fn(
    vm: &mut VM,
    symbols: &mut SymbolTable,
    effects: &mut HashMap<SymbolId, Effect>,
    name: &str,
    func: fn(&[Value], &mut VM) -> LResult<Value>,
    effect: Effect,
) {
    let sym_id = symbols.intern(name);
    vm.set_global(sym_id.0, Value::vm_aware_fn(func));
    effects.insert(sym_id, effect);
}
