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

use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::{AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

use super::JitError;
use super::{dispatch, runtime};

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
    pub(crate) cons: FuncId,
    pub(crate) car: FuncId,
    pub(crate) cdr: FuncId,
    pub(crate) make_array: FuncId,
    pub(crate) is_nil: FuncId,
    pub(crate) is_pair: FuncId,
    pub(crate) is_array: FuncId,
    pub(crate) is_array_mut: FuncId,
    pub(crate) is_struct: FuncId,
    pub(crate) is_struct_mut: FuncId,
    pub(crate) is_set: FuncId,
    pub(crate) is_set_mut: FuncId,
    pub(crate) car_or_nil: FuncId,
    pub(crate) cdr_or_nil: FuncId,
    pub(crate) array_len: FuncId,
    pub(crate) array_ref_or_nil: FuncId,
    pub(crate) car_destructure: FuncId,
    pub(crate) cdr_destructure: FuncId,
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
    pub(crate) region_enter: FuncId,
    pub(crate) region_exit: FuncId,
    pub(crate) region_exit_call: FuncId,
    pub(crate) rotate_pools: FuncId,
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
    builder.symbol("elle_jit_cons", dispatch::elle_jit_cons as *const u8);
    builder.symbol("elle_jit_car", dispatch::elle_jit_car as *const u8);
    builder.symbol("elle_jit_cdr", dispatch::elle_jit_cdr as *const u8);
    builder.symbol(
        "elle_jit_make_array",
        dispatch::elle_jit_make_array as *const u8,
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
        "elle_jit_car_or_nil",
        dispatch::elle_jit_car_or_nil as *const u8,
    );
    builder.symbol(
        "elle_jit_cdr_or_nil",
        dispatch::elle_jit_cdr_or_nil as *const u8,
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
        "elle_jit_car_destructure",
        dispatch::elle_jit_car_destructure as *const u8,
    );
    builder.symbol(
        "elle_jit_cdr_destructure",
        dispatch::elle_jit_cdr_destructure as *const u8,
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
        "elle_jit_rotate_pools",
        dispatch::elle_jit_rotate_pools as *const u8,
    );
}

/// Declare all runtime helper functions in the JITModule, returning their FuncIds.
///
/// All helpers take/return Values as (tag: I64, payload: I64) pairs.
/// vm pointers are plain I64. array/count args are plain I64.
pub(crate) fn declare_helpers(module: &mut JITModule) -> Result<RuntimeHelpers, JitError> {
    // Helper: make a signature
    fn make_sig(
        module: &JITModule,
        params: &[cranelift_codegen::ir::Type],
        returns: &[cranelift_codegen::ir::Type],
    ) -> Signature {
        let mut sig = module.make_signature();
        for &p in params {
            sig.params.push(AbiParam::new(p));
        }
        for &r in returns {
            sig.returns.push(AbiParam::new(r));
        }
        sig
    }

    let declare =
        |module: &mut JITModule, name: &str, sig: &Signature| -> Result<FuncId, JitError> {
            module
                .declare_function(name, Linkage::Import, sig)
                .map_err(|e| JitError::CompilationFailed(e.to_string()))
        };

    // Value unary: (tag, payload) -> (tag, payload)
    let value_unary = make_sig(module, &[I64, I64], &[I64, I64]);
    // Value binary: (atag, apay, btag, bpay) -> (tag, payload)
    let value_binary = make_sig(module, &[I64, I64, I64, I64], &[I64, I64]);
    // Value unary + vm: (tag, payload, vm) -> (tag, payload)
    let value_unary_vm = make_sig(module, &[I64, I64, I64], &[I64, I64]);
    // Value binary + vm: (atag, apay, btag, bpay, vm) -> (tag, payload)
    let value_binary_vm = make_sig(module, &[I64, I64, I64, I64, I64], &[I64, I64]);
    // Value ternary + vm: (t1,p1, t2,p2, t3,p3, vm) -> (tag, payload) -- not needed currently
    // vm only (pointer param): (vm) -> (tag, payload)
    let vm_only = make_sig(module, &[I64], &[I64, I64]);
    // make_array: (elements_ptr, count) -> (tag, payload)  -- ptr is I64, count is I64
    let make_array_sig = make_sig(module, &[I64, I64], &[I64, I64]);
    // call: (func_tag, func_payload, args_ptr, nargs, vm) -> (tag, payload)
    let call_sig = make_sig(module, &[I64, I64, I64, I64, I64], &[I64, I64]);
    // resolve_tail_call: (result_tag, result_payload, vm) -> (tag, payload)
    let resolve_tc_sig = make_sig(module, &[I64, I64, I64], &[I64, I64]);
    // store_capture: (env_ptr, index, val_tag, val_payload) -> (tag, payload)
    let store_capture_sig = make_sig(module, &[I64, I64, I64, I64], &[I64, I64]);
    // store_capture_cell: (cell_tag, cell_payload, val_tag, val_payload) -> (tag, payload)
    let store_capture_cell_sig = make_sig(module, &[I64, I64, I64, I64], &[I64, I64]);
    // array_ref_or_nil: (tag, payload, index) -> (tag, payload)
    let array_ref_or_nil_sig = make_sig(module, &[I64, I64, I64], &[I64, I64]);
    // array_ref_destructure: (tag, payload, index, vm) -> (tag, payload)
    let array_ref_destr_sig = make_sig(module, &[I64, I64, I64, I64], &[I64, I64]);
    // array_slice_from: (tag, payload, index, vm) -> (tag, payload)
    let array_slice_sig = make_sig(module, &[I64, I64, I64, I64], &[I64, I64]);
    // struct_get_or_nil: (stag, spay, ktag, kpay, vm) -> (tag, payload)
    let struct_get_sig = make_sig(module, &[I64, I64, I64, I64, I64], &[I64, I64]);
    // struct_rest: (stag, spay, exclude_ptr, count, vm) -> (tag, payload)
    let struct_rest_sig = make_sig(module, &[I64, I64, I64, I64, I64], &[I64, I64]);
    // check_signal_bound: (tag, payload, allowed_bits, vm) -> (tag, payload)
    let signal_bound_sig = make_sig(module, &[I64, I64, I64, I64], &[I64, I64]);
    // push_param_frame: (pairs_ptr, count, vm) -> (tag, payload)
    let push_param_sig = make_sig(module, &[I64, I64, I64], &[I64, I64]);
    // make_closure: (template_tag, template_payload, captures_ptr, count) -> (tag, payload)
    let make_closure_sig = make_sig(module, &[I64, I64, I64, I64], &[I64, I64]);
    // call_array: (func_tag, func_payload, arr_tag, arr_payload, vm) -> (tag, payload)
    let call_array_sig = make_sig(module, &[I64, I64, I64, I64, I64], &[I64, I64]);
    // cons: (car_tag, car_pay, cdr_tag, cdr_pay) -> (tag, payload)
    let cons_sig = make_sig(module, &[I64, I64, I64, I64], &[I64, I64]);
    // jit_yield: (ytag, ypay, spilled_ptr, yield_idx, vm, ctag, cpay, signal_bits) -> (tag, payload)
    let yield_sig = make_sig(
        module,
        &[I64, I64, I64, I64, I64, I64, I64, I64],
        &[I64, I64],
    );
    // jit_yield_through_call: (spilled_ptr, call_site_idx, vm, ctag, cpay) -> (tag, payload)
    let ytc_sig = make_sig(module, &[I64, I64, I64, I64, I64], &[I64, I64]);
    // void -> (tag, payload)  (no arguments, returns NIL)
    let void_to_value = make_sig(module, &[], &[I64, I64]);
    // vm -> void  (vm pointer, no return)
    let vm_to_void = make_sig(module, &[I64], &[]);

    Ok(RuntimeHelpers {
        add: declare(module, "elle_jit_add", &value_binary)?,
        sub: declare(module, "elle_jit_sub", &value_binary)?,
        mul: declare(module, "elle_jit_mul", &value_binary)?,
        div: declare(module, "elle_jit_div", &value_binary)?,
        rem: declare(module, "elle_jit_rem", &value_binary)?,
        bit_and: declare(module, "elle_jit_bit_and", &value_binary)?,
        bit_or: declare(module, "elle_jit_bit_or", &value_binary)?,
        bit_xor: declare(module, "elle_jit_bit_xor", &value_binary)?,
        shl: declare(module, "elle_jit_shl", &value_binary)?,
        shr: declare(module, "elle_jit_shr", &value_binary)?,
        neg: declare(module, "elle_jit_neg", &value_unary)?,
        not: declare(module, "elle_jit_not", &value_unary)?,
        bit_not: declare(module, "elle_jit_bit_not", &value_unary)?,
        int_to_float: declare(module, "elle_jit_int_to_float", &value_unary)?,
        float_to_int: declare(module, "elle_jit_float_to_int", &value_unary)?,
        eq: declare(module, "elle_jit_eq", &value_binary)?,
        ne: declare(module, "elle_jit_ne", &value_binary)?,
        lt: declare(module, "elle_jit_lt", &value_binary)?,
        le: declare(module, "elle_jit_le", &value_binary)?,
        gt: declare(module, "elle_jit_gt", &value_binary)?,
        ge: declare(module, "elle_jit_ge", &value_binary)?,
        cons: declare(module, "elle_jit_cons", &cons_sig)?,
        car: declare(module, "elle_jit_car", &value_unary)?,
        cdr: declare(module, "elle_jit_cdr", &value_unary)?,
        make_array: declare(module, "elle_jit_make_array", &make_array_sig)?,
        is_nil: declare(module, "elle_jit_is_nil", &value_unary)?,
        is_pair: declare(module, "elle_jit_is_pair", &value_unary)?,
        is_array: declare(module, "elle_jit_is_array", &value_unary)?,
        is_array_mut: declare(module, "elle_jit_is_array_mut", &value_unary)?,
        is_struct: declare(module, "elle_jit_is_struct", &value_unary)?,
        is_struct_mut: declare(module, "elle_jit_is_struct_mut", &value_unary)?,
        is_set: declare(module, "elle_jit_is_set", &value_unary)?,
        is_set_mut: declare(module, "elle_jit_is_set_mut", &value_unary)?,
        car_or_nil: declare(module, "elle_jit_car_or_nil", &value_unary)?,
        cdr_or_nil: declare(module, "elle_jit_cdr_or_nil", &value_unary)?,
        array_len: declare(module, "elle_jit_array_len", &value_unary)?,
        array_ref_or_nil: declare(module, "elle_jit_array_ref_or_nil", &array_ref_or_nil_sig)?,
        car_destructure: declare(module, "elle_jit_car_destructure", &value_unary_vm)?,
        cdr_destructure: declare(module, "elle_jit_cdr_destructure", &value_unary_vm)?,
        array_ref_destructure: declare(
            module,
            "elle_jit_array_ref_destructure",
            &array_ref_destr_sig,
        )?,
        array_slice_from: declare(module, "elle_jit_array_slice_from", &array_slice_sig)?,
        struct_get_or_nil: declare(module, "elle_jit_struct_get_or_nil", &struct_get_sig)?,
        struct_get_destructure: declare(
            module,
            "elle_jit_struct_get_destructure",
            &struct_get_sig,
        )?,
        struct_rest: declare(module, "elle_jit_struct_rest", &struct_rest_sig)?,
        check_signal_bound: declare(module, "elle_jit_check_signal_bound", &signal_bound_sig)?,
        array_push: declare(module, "elle_jit_array_push", &value_binary_vm)?,
        array_extend: declare(module, "elle_jit_array_extend", &value_binary_vm)?,
        push_param_frame: declare(module, "elle_jit_push_param_frame", &push_param_sig)?,
        is_truthy: declare(module, "elle_jit_is_truthy", &value_unary)?,
        make_capture: declare(module, "elle_jit_make_capture", &value_unary)?,
        load_capture_cell: declare(module, "elle_jit_load_capture_cell", &value_unary)?,
        load_capture: declare(module, "elle_jit_load_capture", &value_unary)?,
        store_capture_cell: declare(
            module,
            "elle_jit_store_capture_cell",
            &store_capture_cell_sig,
        )?,
        store_capture: declare(module, "elle_jit_store_capture", &store_capture_sig)?,
        call: declare(module, "elle_jit_call", &call_sig)?,
        tail_call: declare(module, "elle_jit_tail_call", &call_sig)?,
        has_exception: declare(module, "elle_jit_has_exception", &vm_only)?,
        resolve_tail_call: declare(module, "elle_jit_resolve_tail_call", &resolve_tc_sig)?,
        call_depth_enter: declare(module, "elle_jit_call_depth_enter", &vm_only)?,
        call_depth_exit: declare(module, "elle_jit_call_depth_exit", &vm_only)?,
        pop_param_frame: declare(module, "elle_jit_pop_param_frame", &vm_only)?,
        call_array: declare(module, "elle_jit_call_array", &call_array_sig)?,
        tail_call_array: declare(module, "elle_jit_tail_call_array", &call_array_sig)?,
        make_closure: declare(module, "elle_jit_make_closure", &make_closure_sig)?,
        jit_yield: declare(module, "elle_jit_yield", &yield_sig)?,
        jit_yield_through_call: declare(module, "elle_jit_yield_through_call", &ytc_sig)?,
        has_signal: declare(module, "elle_jit_has_signal", &vm_only)?,
        region_enter: declare(module, "elle_jit_region_enter", &void_to_value)?,
        region_exit: declare(module, "elle_jit_region_exit", &void_to_value)?,
        region_exit_call: declare(module, "elle_jit_region_exit_call", &void_to_value)?,
        rotate_pools: declare(module, "elle_jit_rotate_pools", &vm_to_void)?,
    })
}
