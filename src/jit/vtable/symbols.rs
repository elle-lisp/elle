// audited: 2026-09-13
// docs/impl/jit.md
//! One `builder.symbol` line per `elle_jit_*` helper: the address the JIT
//! linker resolves each name to.
//!
//! Split from the vtable root because this list and the `RuntimeHelpers`
//! struct beside it grow independently — a name here, a `FuncId` there.

use cranelift_jit::JITBuilder;

use super::super::{dispatch, runtime};

/// Register all `elle_jit_*` symbols with the JITBuilder.
pub(crate) fn register_symbols(builder: &mut JITBuilder) {
    // Arithmetic and comparison (runtime.rs)
    builder.symbol("elle_jit_add", runtime::elle_jit_add as *const u8);
    builder.symbol("elle_jit_sub", runtime::elle_jit_sub as *const u8);
    builder.symbol("elle_jit_mul", runtime::elle_jit_mul as *const u8);
    builder.symbol("elle_jit_div", runtime::elle_jit_div as *const u8);
    builder.symbol("elle_jit_rem", runtime::elle_jit_rem as *const u8);
    builder.symbol("elle_jit_bit_and", runtime::elle_jit_bit_and as *const u8);
    builder.symbol("elle_jit_bit_or", runtime::elle_jit_bit_or as *const u8);
    builder.symbol("elle_jit_bit_xor", runtime::elle_jit_bit_xor as *const u8);
    builder.symbol("elle_jit_shl", runtime::elle_jit_shl as *const u8);
    builder.symbol("elle_jit_shr", runtime::elle_jit_shr as *const u8);
    builder.symbol("elle_jit_neg", runtime::elle_jit_neg as *const u8);
    builder.symbol("elle_jit_not", runtime::elle_jit_not as *const u8);
    builder.symbol("elle_jit_bit_not", runtime::elle_jit_bit_not as *const u8);
    builder.symbol(
        "elle_jit_int_to_float",
        runtime::elle_jit_int_to_float as *const u8,
    );
    builder.symbol(
        "elle_jit_float_to_int",
        runtime::elle_jit_float_to_int as *const u8,
    );
    builder.symbol("elle_jit_eq", runtime::elle_jit_eq as *const u8);
    builder.symbol("elle_jit_ne", runtime::elle_jit_ne as *const u8);
    builder.symbol("elle_jit_lt", runtime::elle_jit_lt as *const u8);
    builder.symbol("elle_jit_le", runtime::elle_jit_le as *const u8);
    builder.symbol("elle_jit_gt", runtime::elle_jit_gt as *const u8);
    builder.symbol("elle_jit_ge", runtime::elle_jit_ge as *const u8);
    builder.symbol("elle_jit_is_nil", runtime::elle_jit_is_nil as *const u8);
    builder.symbol(
        "elle_jit_is_truthy",
        runtime::elle_jit_is_truthy as *const u8,
    );

    // Data structure, lbox, call, and yield helpers
    builder.symbol("elle_jit_pair", dispatch::elle_jit_pair as *const u8);
    builder.symbol("elle_jit_first", dispatch::elle_jit_first as *const u8);
    builder.symbol("elle_jit_rest", dispatch::elle_jit_rest as *const u8);
    builder.symbol(
        "elle_jit_make_array",
        dispatch::elle_jit_make_array as *const u8,
    );
    builder.symbol(
        "elle_jit_materialize_const",
        dispatch::elle_jit_materialize_const as *const u8,
    );
    builder.symbol("elle_jit_is_pair", dispatch::elle_jit_is_pair as *const u8);
    builder.symbol(
        "elle_jit_is_array",
        dispatch::elle_jit_is_array as *const u8,
    );
    builder.symbol(
        "elle_jit_is_array_mut",
        dispatch::elle_jit_is_array_mut as *const u8,
    );
    builder.symbol(
        "elle_jit_is_struct",
        dispatch::elle_jit_is_struct as *const u8,
    );
    builder.symbol(
        "elle_jit_is_struct_mut",
        dispatch::elle_jit_is_struct_mut as *const u8,
    );
    builder.symbol("elle_jit_is_set", dispatch::elle_jit_is_set as *const u8);
    builder.symbol(
        "elle_jit_is_set_mut",
        dispatch::elle_jit_is_set_mut as *const u8,
    );
    builder.symbol(
        "elle_jit_first_or_nil",
        dispatch::elle_jit_first_or_nil as *const u8,
    );
    builder.symbol(
        "elle_jit_rest_or_nil",
        dispatch::elle_jit_rest_or_nil as *const u8,
    );
    builder.symbol(
        "elle_jit_array_len",
        dispatch::elle_jit_array_len as *const u8,
    );
    builder.symbol(
        "elle_jit_array_ref_or_nil",
        dispatch::elle_jit_array_ref_or_nil as *const u8,
    );
    builder.symbol(
        "elle_jit_match_fail",
        dispatch::elle_jit_match_fail as *const u8,
    );
    builder.symbol(
        "elle_jit_first_destructure",
        dispatch::elle_jit_first_destructure as *const u8,
    );
    builder.symbol(
        "elle_jit_rest_destructure",
        dispatch::elle_jit_rest_destructure as *const u8,
    );
    builder.symbol(
        "elle_jit_array_ref_destructure",
        dispatch::elle_jit_array_ref_destructure as *const u8,
    );
    builder.symbol(
        "elle_jit_array_slice_from",
        dispatch::elle_jit_array_slice_from as *const u8,
    );
    builder.symbol(
        "elle_jit_struct_get_or_nil",
        dispatch::elle_jit_struct_get_or_nil as *const u8,
    );
    builder.symbol(
        "elle_jit_struct_get_destructure",
        dispatch::elle_jit_struct_get_destructure as *const u8,
    );
    builder.symbol(
        "elle_jit_struct_rest",
        dispatch::elle_jit_struct_rest as *const u8,
    );
    builder.symbol(
        "elle_jit_check_signal_bound",
        dispatch::elle_jit_check_signal_bound as *const u8,
    );
    builder.symbol(
        "elle_jit_array_push",
        dispatch::elle_jit_array_push as *const u8,
    );
    builder.symbol(
        "elle_jit_array_extend",
        dispatch::elle_jit_array_extend as *const u8,
    );
    builder.symbol(
        "elle_jit_push_param_frame",
        dispatch::elle_jit_push_param_frame as *const u8,
    );
    builder.symbol(
        "elle_jit_make_capture",
        dispatch::elle_jit_make_capture as *const u8,
    );
    builder.symbol(
        "elle_jit_make_capture_owned",
        dispatch::elle_jit_make_capture_owned as *const u8,
    );
    builder.symbol(
        "elle_jit_collect_rest_list",
        dispatch::elle_jit_collect_rest_list as *const u8,
    );
    builder.symbol(
        "elle_jit_load_capture_cell",
        dispatch::elle_jit_load_capture_cell as *const u8,
    );
    builder.symbol(
        "elle_jit_load_capture",
        dispatch::elle_jit_load_capture as *const u8,
    );
    builder.symbol(
        "elle_jit_store_capture_cell",
        dispatch::elle_jit_store_capture_cell as *const u8,
    );
    builder.symbol(
        "elle_jit_store_capture",
        dispatch::elle_jit_store_capture as *const u8,
    );
    builder.symbol("elle_jit_call", dispatch::elle_jit_call as *const u8);
    builder.symbol(
        "elle_jit_tail_call",
        dispatch::elle_jit_tail_call as *const u8,
    );
    builder.symbol(
        "elle_jit_has_exception",
        dispatch::elle_jit_has_exception as *const u8,
    );
    builder.symbol(
        "elle_jit_pop_param_frame",
        dispatch::elle_jit_pop_param_frame as *const u8,
    );
    builder.symbol(
        "elle_jit_call_array",
        dispatch::elle_jit_call_array as *const u8,
    );
    builder.symbol(
        "elle_jit_tail_call_array",
        dispatch::elle_jit_tail_call_array as *const u8,
    );
    builder.symbol(
        "elle_jit_make_closure",
        dispatch::elle_jit_make_closure as *const u8,
    );
    builder.symbol("elle_jit_yield", dispatch::elle_jit_yield as *const u8);
    builder.symbol(
        "elle_jit_yield_through_call",
        dispatch::elle_jit_yield_through_call as *const u8,
    );
    builder.symbol(
        "elle_jit_has_signal",
        dispatch::elle_jit_has_signal as *const u8,
    );
    builder.symbol(
        "elle_jit_region_enter",
        dispatch::elle_jit_region_enter as *const u8,
    );
    builder.symbol(
        "elle_jit_region_exit",
        dispatch::elle_jit_region_exit as *const u8,
    );
    builder.symbol(
        "elle_jit_region_exit_call",
        dispatch::elle_jit_region_exit_call as *const u8,
    );
    builder.symbol(
        "elle_jit_region_rotate",
        dispatch::elle_jit_region_rotate as *const u8,
    );
    builder.symbol(
        "elle_jit_incref_region",
        dispatch::elle_jit_incref_region as *const u8,
    );
    builder.symbol(
        "elle_jit_decref_region",
        dispatch::elle_jit_decref_region as *const u8,
    );
    builder.symbol(
        "elle_jit_decref_value_region",
        dispatch::elle_jit_decref_value_region as *const u8,
    );
    builder.symbol(
        "elle_jit_decref_cell_region",
        dispatch::elle_jit_decref_cell_region as *const u8,
    );
    builder.symbol(
        "elle_jit_incref_value_region",
        dispatch::elle_jit_incref_value_region as *const u8,
    );
    builder.symbol(
        "elle_jit_adopt_region",
        dispatch::elle_jit_adopt_region as *const u8,
    );
    builder.symbol(
        "elle_jit_adopt_cell_region",
        dispatch::elle_jit_adopt_cell_region as *const u8,
    );
    builder.symbol(
        "elle_jit_adopt_into_activation",
        dispatch::elle_jit_adopt_into_activation as *const u8,
    );
    builder.symbol(
        "elle_jit_release_activation_dues",
        dispatch::elle_jit_release_activation_dues as *const u8,
    );
    builder.symbol(
        "elle_jit_release_abandoned_frame",
        dispatch::elle_jit_release_abandoned_frame as *const u8,
    );
    builder.symbol(
        "elle_jit_free_region_group",
        dispatch::elle_jit_free_region_group as *const u8,
    );
    builder.symbol(
        "elle_jit_push_region_map",
        dispatch::elle_jit_push_region_map as *const u8,
    );
    builder.symbol(
        "elle_jit_pop_region_map",
        dispatch::elle_jit_pop_region_map as *const u8,
    );
    builder.symbol(
        "elle_jit_resolve_alloc_region",
        dispatch::elle_jit_resolve_alloc_region as *const u8,
    );
    builder.symbol(
        "elle_jit_resolve_alloc_region_merged",
        dispatch::elle_jit_resolve_alloc_region_merged as *const u8,
    );
    builder.symbol(
        "elle_jit_rotate_pools",
        dispatch::elle_jit_rotate_pools as *const u8,
    );
    builder.symbol("elle_jit_incref", dispatch::elle_jit_incref as *const u8);
    builder.symbol("elle_jit_decref", dispatch::elle_jit_decref as *const u8);
    // New intrinsic helpers
    builder.symbol("elle_jit_is_empty", runtime::elle_jit_is_empty as *const u8);
    builder.symbol("elle_jit_is_bool", runtime::elle_jit_is_bool as *const u8);
    builder.symbol("elle_jit_is_int", runtime::elle_jit_is_int as *const u8);
    builder.symbol("elle_jit_is_float", runtime::elle_jit_is_float as *const u8);
    builder.symbol(
        "elle_jit_is_string",
        runtime::elle_jit_is_string as *const u8,
    );
    builder.symbol(
        "elle_jit_is_keyword",
        runtime::elle_jit_is_keyword as *const u8,
    );
    builder.symbol(
        "elle_jit_is_symbol_check",
        runtime::elle_jit_is_symbol_check as *const u8,
    );
    builder.symbol("elle_jit_is_bytes", runtime::elle_jit_is_bytes as *const u8);
    builder.symbol("elle_jit_is_box", runtime::elle_jit_is_box as *const u8);
    builder.symbol(
        "elle_jit_is_closure",
        runtime::elle_jit_is_closure as *const u8,
    );
    builder.symbol("elle_jit_is_fiber", runtime::elle_jit_is_fiber as *const u8);
    builder.symbol("elle_jit_type_of", runtime::elle_jit_type_of as *const u8);
    builder.symbol("elle_jit_length", runtime::elle_jit_length as *const u8);
    builder.symbol("elle_jit_get", runtime::elle_jit_get as *const u8);
    builder.symbol("elle_jit_put", runtime::elle_jit_put as *const u8);
    builder.symbol("elle_jit_del", runtime::elle_jit_del as *const u8);
    builder.symbol("elle_jit_has", runtime::elle_jit_has as *const u8);
    builder.symbol("elle_jit_push", runtime::elle_jit_push as *const u8);
    builder.symbol(
        "elle_jit_string_push",
        runtime::elle_jit_string_push as *const u8,
    );
    builder.symbol(
        "elle_jit_bytes_push",
        runtime::elle_jit_bytes_push as *const u8,
    );
    builder.symbol("elle_jit_pop", runtime::elle_jit_pop as *const u8);
    builder.symbol("elle_jit_freeze", runtime::elle_jit_freeze as *const u8);
    builder.symbol("elle_jit_thaw", runtime::elle_jit_thaw as *const u8);
    builder.symbol(
        "elle_jit_identical",
        runtime::elle_jit_identical as *const u8,
    );
}
