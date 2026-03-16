//! RuntimeHelpers vtable: pre-declared Cranelift FuncIds for all extern "C" helpers.
//!
//! This module registers every `elle_jit_*` symbol with the JITBuilder and
//! declares the corresponding `FuncId`s in the JITModule. The result is
//! `RuntimeHelpers`, a plain struct of `FuncId` fields that `JitCompiler`
//! and `FunctionTranslator` use to emit calls to runtime helpers.

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
    pub(crate) make_lbox: FuncId,
    pub(crate) load_lbox: FuncId,
    pub(crate) load_capture: FuncId,
    pub(crate) store_lbox: FuncId,
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
    pub(crate) make_closure: FuncId,
    pub(crate) jit_yield: FuncId,
    pub(crate) jit_yield_through_call: FuncId,
    pub(crate) has_signal: FuncId,
}

/// Register all `elle_jit_*` symbols with the JITBuilder.
///
/// Must be called before `JITModule::new` so that the linker resolves each
/// symbol to the corresponding Rust function.
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

    // Data structure, lbox, call, and yield helpers (dispatch.rs re-exports data.rs + suspend.rs)
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
        "elle_jit_make_lbox",
        dispatch::elle_jit_make_lbox as *const u8,
    );
    builder.symbol(
        "elle_jit_load_lbox",
        dispatch::elle_jit_load_lbox as *const u8,
    );
    builder.symbol(
        "elle_jit_load_capture",
        dispatch::elle_jit_load_capture as *const u8,
    );
    builder.symbol(
        "elle_jit_store_lbox",
        dispatch::elle_jit_store_lbox as *const u8,
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
}

/// Declare all runtime helper functions in the JITModule, returning their FuncIds.
///
/// The module must have been created after `register_symbols` so that each
/// imported function name resolves to the correct symbol.
pub(crate) fn declare_helpers(module: &mut JITModule) -> Result<RuntimeHelpers, JitError> {
    // Binary function signature: (i64, i64) -> i64
    let mut binary_sig = module.make_signature();
    binary_sig.params.push(AbiParam::new(I64));
    binary_sig.params.push(AbiParam::new(I64));
    binary_sig.returns.push(AbiParam::new(I64));

    // Unary function signature: (i64) -> i64
    let mut unary_sig = module.make_signature();
    unary_sig.params.push(AbiParam::new(I64));
    unary_sig.returns.push(AbiParam::new(I64));

    // Ternary function signature: (i64, i64, i64) -> i64
    let mut ternary_sig = module.make_signature();
    ternary_sig.params.push(AbiParam::new(I64));
    ternary_sig.params.push(AbiParam::new(I64));
    ternary_sig.params.push(AbiParam::new(I64));
    ternary_sig.returns.push(AbiParam::new(I64));

    // Make array signature: (ptr, count) -> i64
    let mut make_array_sig = module.make_signature();
    make_array_sig.params.push(AbiParam::new(I64)); // elements ptr
    make_array_sig.params.push(AbiParam::new(I64)); // count (as i64)
    make_array_sig.returns.push(AbiParam::new(I64));

    // Call signature: (func, args_ptr, nargs, vm) -> i64
    let mut call_sig = module.make_signature();
    call_sig.params.push(AbiParam::new(I64)); // func
    call_sig.params.push(AbiParam::new(I64)); // args_ptr
    call_sig.params.push(AbiParam::new(I64)); // nargs (as i64)
    call_sig.params.push(AbiParam::new(I64)); // vm
    call_sig.returns.push(AbiParam::new(I64));

    // Quaternary function signature: (i64, i64, i64, i64) -> i64
    let mut quaternary_sig = module.make_signature();
    for _ in 0..4 {
        quaternary_sig.params.push(AbiParam::new(I64));
    }
    quaternary_sig.returns.push(AbiParam::new(I64));

    // elle_jit_yield: 5 params (yielded, spilled_ptr, yield_index, vm, closure_bits)
    let mut yield_sig = module.make_signature();
    for _ in 0..5 {
        yield_sig.params.push(AbiParam::new(I64));
    }
    yield_sig.returns.push(AbiParam::new(I64));

    // elle_jit_yield_through_call: 4 params (spilled_ptr, call_site_index, vm, closure_bits)
    let mut ytc_sig = module.make_signature();
    for _ in 0..4 {
        ytc_sig.params.push(AbiParam::new(I64));
    }
    ytc_sig.returns.push(AbiParam::new(I64));

    let declare =
        |module: &mut JITModule, name: &str, sig: &Signature| -> Result<FuncId, JitError> {
            module
                .declare_function(name, Linkage::Import, sig)
                .map_err(|e| JitError::CompilationFailed(e.to_string()))
        };

    Ok(RuntimeHelpers {
        add: declare(module, "elle_jit_add", &binary_sig)?,
        sub: declare(module, "elle_jit_sub", &binary_sig)?,
        mul: declare(module, "elle_jit_mul", &binary_sig)?,
        div: declare(module, "elle_jit_div", &binary_sig)?,
        rem: declare(module, "elle_jit_rem", &binary_sig)?,
        bit_and: declare(module, "elle_jit_bit_and", &binary_sig)?,
        bit_or: declare(module, "elle_jit_bit_or", &binary_sig)?,
        bit_xor: declare(module, "elle_jit_bit_xor", &binary_sig)?,
        shl: declare(module, "elle_jit_shl", &binary_sig)?,
        shr: declare(module, "elle_jit_shr", &binary_sig)?,
        neg: declare(module, "elle_jit_neg", &unary_sig)?,
        not: declare(module, "elle_jit_not", &unary_sig)?,
        bit_not: declare(module, "elle_jit_bit_not", &unary_sig)?,
        eq: declare(module, "elle_jit_eq", &binary_sig)?,
        ne: declare(module, "elle_jit_ne", &binary_sig)?,
        lt: declare(module, "elle_jit_lt", &binary_sig)?,
        le: declare(module, "elle_jit_le", &binary_sig)?,
        gt: declare(module, "elle_jit_gt", &binary_sig)?,
        ge: declare(module, "elle_jit_ge", &binary_sig)?,
        cons: declare(module, "elle_jit_cons", &binary_sig)?,
        car: declare(module, "elle_jit_car", &unary_sig)?,
        cdr: declare(module, "elle_jit_cdr", &unary_sig)?,
        make_array: declare(module, "elle_jit_make_array", &make_array_sig)?,
        is_nil: declare(module, "elle_jit_is_nil", &unary_sig)?,
        is_pair: declare(module, "elle_jit_is_pair", &unary_sig)?,
        is_array: declare(module, "elle_jit_is_array", &unary_sig)?,
        is_array_mut: declare(module, "elle_jit_is_array_mut", &unary_sig)?,
        is_struct: declare(module, "elle_jit_is_struct", &unary_sig)?,
        is_struct_mut: declare(module, "elle_jit_is_struct_mut", &unary_sig)?,
        is_set: declare(module, "elle_jit_is_set", &unary_sig)?,
        is_set_mut: declare(module, "elle_jit_is_set_mut", &unary_sig)?,
        car_or_nil: declare(module, "elle_jit_car_or_nil", &unary_sig)?,
        cdr_or_nil: declare(module, "elle_jit_cdr_or_nil", &unary_sig)?,
        array_len: declare(module, "elle_jit_array_len", &unary_sig)?,
        array_ref_or_nil: declare(module, "elle_jit_array_ref_or_nil", &binary_sig)?,
        car_destructure: declare(module, "elle_jit_car_destructure", &binary_sig)?,
        cdr_destructure: declare(module, "elle_jit_cdr_destructure", &binary_sig)?,
        array_ref_destructure: declare(module, "elle_jit_array_ref_destructure", &ternary_sig)?,
        array_slice_from: declare(module, "elle_jit_array_slice_from", &ternary_sig)?,
        struct_get_or_nil: declare(module, "elle_jit_struct_get_or_nil", &ternary_sig)?,
        struct_get_destructure: declare(module, "elle_jit_struct_get_destructure", &ternary_sig)?,
        struct_rest: declare(module, "elle_jit_struct_rest", &quaternary_sig)?,
        check_signal_bound: declare(module, "elle_jit_check_signal_bound", &ternary_sig)?,
        array_push: declare(module, "elle_jit_array_push", &ternary_sig)?,
        array_extend: declare(module, "elle_jit_array_extend", &ternary_sig)?,
        push_param_frame: declare(module, "elle_jit_push_param_frame", &ternary_sig)?,
        is_truthy: declare(module, "elle_jit_is_truthy", &unary_sig)?,
        make_lbox: declare(module, "elle_jit_make_lbox", &unary_sig)?,
        load_lbox: declare(module, "elle_jit_load_lbox", &unary_sig)?,
        load_capture: declare(module, "elle_jit_load_capture", &unary_sig)?,
        store_lbox: declare(module, "elle_jit_store_lbox", &binary_sig)?,
        store_capture: declare(module, "elle_jit_store_capture", &ternary_sig)?,
        call: declare(module, "elle_jit_call", &call_sig)?,
        tail_call: declare(module, "elle_jit_tail_call", &call_sig)?,
        has_exception: declare(module, "elle_jit_has_exception", &unary_sig)?,
        resolve_tail_call: declare(module, "elle_jit_resolve_tail_call", &binary_sig)?,
        call_depth_enter: declare(module, "elle_jit_call_depth_enter", &unary_sig)?,
        call_depth_exit: declare(module, "elle_jit_call_depth_exit", &unary_sig)?,
        pop_param_frame: declare(module, "elle_jit_pop_param_frame", &unary_sig)?,
        call_array: declare(module, "elle_jit_call_array", &ternary_sig)?,
        tail_call_array: declare(module, "elle_jit_tail_call_array", &ternary_sig)?,
        make_closure: declare(module, "elle_jit_make_closure", &ternary_sig)?,
        jit_yield: declare(module, "elle_jit_yield", &yield_sig)?,
        jit_yield_through_call: declare(module, "elle_jit_yield_through_call", &ytc_sig)?,
        has_signal: declare(module, "elle_jit_has_signal", &unary_sig)?,
    })
}
