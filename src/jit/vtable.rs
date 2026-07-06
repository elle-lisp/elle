//! RuntimeHelpers vtable: pre-declared Cranelift FuncIds for all extern "C" helpers.
//!
//! This module registers every `elle_jit_*` symbol with the JITBuilder and
//! declares the corresponding `FuncId`s in the JITModule. The result is
//! `RuntimeHelpers`, a plain struct of `FuncId` fields that `JitCompiler`
//! and `FunctionTranslator` use to emit calls to runtime helpers.
//!
//! ## Calling convention for Values
//!
//! Values are passed and returned as TWO `I64` Cranelift arguments: (tag, payload).
//! A "Value parameter" = two consecutive I64 params.
//! A "Value return" = two consecutive I64 return values.
//!
//! Helper arity table (counting Value params as 2 each):
//!   value_unary: (tag, payload) -> (tag, payload)           = 2 params, 2 returns
//!   value_binary: (atag, apay, btag, bpay) -> (tag, payload) = 4 params, 2 returns
//!   value_unary_vm: (tag, payload, vm) -> (tag, payload)     = 3 params, 2 returns
//!   value_binary_vm: (atag, apay, btag, bpay, vm) -> (tag, payload) = 5 params, 2 returns
//!   call: (ftag, fpay, args_ptr, nargs, vm) -> (tag, payload) = 5 params, 2 returns

use cranelift_codegen::ir::types::{I32, I64};
use cranelift_codegen::ir::{AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

use super::JitError;
use super::{dispatch, runtime};

mod helpers;
pub(crate) use helpers::*;

/// Pre-declared runtime helper function IDs.
///
/// Each field maps to a `#[no_mangle] extern "C"` function in `runtime.rs`
/// or `dispatch.rs` / `data.rs` / `suspend.rs`. The IDs are declared in the
/// JITModule at construction time so that `FunctionTranslator` can reference
/// them without re-declaring on every function compilation.
pub(crate) struct RuntimeHelpers {
    pub(crate) add: FuncId,
    pub(crate) sub: FuncId,
    pub(crate) mul: FuncId,
    pub(crate) div: FuncId,
    pub(crate) rem: FuncId,
    pub(crate) bit_and: FuncId,
    pub(crate) bit_or: FuncId,
    pub(crate) bit_xor: FuncId,
    pub(crate) shl: FuncId,
    pub(crate) shr: FuncId,
    pub(crate) neg: FuncId,
    pub(crate) not: FuncId,
    pub(crate) bit_not: FuncId,
    pub(crate) int_to_float: FuncId,
    pub(crate) float_to_int: FuncId,
    pub(crate) eq: FuncId,
    pub(crate) ne: FuncId,
    pub(crate) lt: FuncId,
    pub(crate) le: FuncId,
    pub(crate) gt: FuncId,
    pub(crate) ge: FuncId,
    pub(crate) pair: FuncId,
    pub(crate) first: FuncId,
    pub(crate) rest: FuncId,
    pub(crate) make_array: FuncId,
    /// Materialize a heap literal (string, or quoted compound data) into the
    /// current alloc region from a JIT-code-owned `ConstTemplate`. See
    /// `dispatch::elle_jit_materialize_const`.
    pub(crate) materialize_const: FuncId,
    pub(crate) is_nil: FuncId,
    pub(crate) is_pair: FuncId,
    pub(crate) is_array: FuncId,
    pub(crate) is_array_mut: FuncId,
    pub(crate) is_struct: FuncId,
    pub(crate) is_struct_mut: FuncId,
    pub(crate) is_set: FuncId,
    pub(crate) is_set_mut: FuncId,
    pub(crate) first_or_nil: FuncId,
    pub(crate) rest_or_nil: FuncId,
    pub(crate) array_len: FuncId,
    pub(crate) array_ref_or_nil: FuncId,
    pub(crate) match_fail: FuncId,
    pub(crate) first_destructure: FuncId,
    pub(crate) rest_destructure: FuncId,
    pub(crate) array_ref_destructure: FuncId,
    pub(crate) array_slice_from: FuncId,
    pub(crate) struct_get_or_nil: FuncId,
    pub(crate) struct_get_destructure: FuncId,
    pub(crate) struct_rest: FuncId,
    pub(crate) check_signal_bound: FuncId,
    pub(crate) array_push: FuncId,
    pub(crate) array_extend: FuncId,
    pub(crate) push_param_frame: FuncId,
    #[allow(dead_code)]
    pub(crate) is_truthy: FuncId,
    pub(crate) make_capture: FuncId,
    /// Capture cell minted into its OWN fresh per-execution region (JIT-prologue
    /// env path; mirrors the interpreter's `env_value_region`). See
    /// `dispatch::elle_jit_make_capture_owned`.
    pub(crate) make_capture_owned: FuncId,
    /// Variadic rest list with per-cons fresh regions (JIT-prologue env path;
    /// mirrors the interpreter's `args_to_list`). See
    /// `dispatch::elle_jit_collect_rest_list`.
    pub(crate) collect_rest_list: FuncId,
    pub(crate) load_capture_cell: FuncId,
    pub(crate) load_capture: FuncId,
    pub(crate) store_capture_cell: FuncId,
    pub(crate) store_capture: FuncId,
    pub(crate) call: FuncId,
    pub(crate) tail_call: FuncId,
    pub(crate) has_exception: FuncId,
    pub(crate) resolve_tail_call: FuncId,
    pub(crate) call_depth_enter: FuncId,
    pub(crate) call_depth_exit: FuncId,
    pub(crate) pop_param_frame: FuncId,
    pub(crate) call_array: FuncId,
    pub(crate) tail_call_array: FuncId,
    #[allow(dead_code)] // infrastructure for future JIT MakeClosure support
    pub(crate) make_closure: FuncId,
    pub(crate) jit_yield: FuncId,
    pub(crate) jit_yield_through_call: FuncId,
    pub(crate) has_signal: FuncId,
    #[allow(dead_code)] // JIT region infrastructure — wired incrementally
    pub(crate) region_enter: FuncId,
    #[allow(dead_code)]
    pub(crate) region_exit: FuncId,
    #[allow(dead_code)]
    pub(crate) region_exit_call: FuncId,
    #[allow(dead_code)]
    pub(crate) region_rotate: FuncId,
    pub(crate) incref_region: FuncId,
    pub(crate) decref_region: FuncId,
    pub(crate) decref_value_region: FuncId,
    pub(crate) decref_cell_region: FuncId,
    pub(crate) incref_value_region: FuncId,
    /// Link a child value's region as Owned by a parent value's region — the
    /// `AdoptRegion` instruction's JIT helper, mirroring `handle_adopt_region`.
    pub(crate) adopt_region: FuncId,
    /// Link a child value's region as Owned by a parent value's region using
    /// `region_of` (NOT `result_region_of`) — the `AdoptCellRegion` instruction's
    /// JIT helper, mirroring `handle_adopt_cell_region`. Adopts a capture cell's
    /// OWN region (never unwrapped).
    pub(crate) adopt_cell_region: FuncId,
    /// Adopt a child value's region into the current activation's owner node —
    /// the `AdoptIntoActivation` instruction's JIT helper, mirroring
    /// `handle_adopt_into_activation`.
    pub(crate) adopt_into_activation: FuncId,
    /// Free the current activation's owner node at the compiled `Return` path —
    /// the JIT twin of the interpreter trampoline's clean-break release.
    pub(crate) release_activation_owner_node: FuncId,
    /// Free a co-owned region group as one unit — the `FreeRegionGroup`
    /// instruction's JIT helper, mirroring `handle_free_region_group`.
    pub(crate) free_region_group: FuncId,
    pub(crate) push_region_map: FuncId,
    pub(crate) pop_region_map: FuncId,
    pub(crate) resolve_alloc_region: FuncId,
    /// The mint-or-reuse variant of `resolve_alloc_region`, selected at emit time
    /// for a slot in `LirFunction.merged_slots` (builder-idiom merge;
    /// docs/impl/region-model.md § Merging).
    pub(crate) resolve_alloc_region_merged: FuncId,
    #[allow(dead_code)]
    pub(crate) rotate_pools: FuncId,
    #[allow(dead_code)]
    pub(crate) incref: FuncId,
    #[allow(dead_code)]
    pub(crate) decref: FuncId,
    // New intrinsic helpers
    pub(crate) is_empty: FuncId,
    pub(crate) is_bool: FuncId,
    pub(crate) is_int: FuncId,
    pub(crate) is_float: FuncId,
    pub(crate) is_string: FuncId,
    pub(crate) is_keyword: FuncId,
    pub(crate) is_symbol_check: FuncId,
    pub(crate) is_bytes: FuncId,
    pub(crate) is_box: FuncId,
    pub(crate) is_closure: FuncId,
    pub(crate) is_fiber: FuncId,
    pub(crate) type_of: FuncId,
    pub(crate) length: FuncId,
    pub(crate) get: FuncId,
    pub(crate) put: FuncId,
    pub(crate) del: FuncId,
    pub(crate) has: FuncId,
    pub(crate) intr_push: FuncId,
    pub(crate) intr_string_push: FuncId,
    pub(crate) intr_bytes_push: FuncId,
    pub(crate) pop: FuncId,
    pub(crate) freeze: FuncId,
    pub(crate) thaw: FuncId,
    pub(crate) identical: FuncId,
}

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
        "elle_jit_resolve_tail_call",
        dispatch::elle_jit_resolve_tail_call as *const u8,
    );
    builder.symbol(
        "elle_jit_call_depth_enter",
        dispatch::elle_jit_call_depth_enter as *const u8,
    );
    builder.symbol(
        "elle_jit_call_depth_exit",
        dispatch::elle_jit_call_depth_exit as *const u8,
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
        "elle_jit_release_activation_owner_node",
        dispatch::elle_jit_release_activation_owner_node as *const u8,
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
