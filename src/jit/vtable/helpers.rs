use super::*;

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
    // make_array: (elements_ptr, count, region, vm) -> (tag, payload). The vm
    // pointer names the heap the array is born on (the driving instance's own).
    let make_array_sig = make_sig(module, &[I64, I64, I32, I64], &[I64, I64]);
    // materialize_const: (template_ptr, region, vm) -> (tag, payload). The vm
    // pointer lets a quoted-symbol leaf re-intern its name into the instance's
    // own table.
    let materialize_sig = make_sig(module, &[I64, I32, I64], &[I64, I64]);
    // call: (func_tag, func_payload, args_ptr, nargs, vm, region_id) -> (tag, payload)
    // region_id (I32) routes native-result allocation and gates the
    // pass-through retain, mirroring the interpreter's `call_inner`.
    let call_sig = make_sig(module, &[I64, I64, I64, I64, I64, I32], &[I64, I64]);
    // resolve_tail_call: (result_tag, result_payload, vm) -> (tag, payload)
    let resolve_tc_sig = make_sig(module, &[I64, I64, I64], &[I64, I64]);
    // store_capture: (env_ptr, index, val_tag, val_payload, vm) -> (tag, payload)
    let store_capture_sig = make_sig(module, &[I64, I64, I64, I64, I64], &[I64, I64]);
    // store_capture_cell: (cell_tag, cell_payload, val_tag, val_payload, vm) -> (tag, payload)
    let store_capture_cell_sig = make_sig(module, &[I64, I64, I64, I64, I64], &[I64, I64]);
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
    // make_closure: (template_ptr, captures_ptr, count, region: I32, vm) -> (tag, payload)
    let make_closure_sig = make_sig(module, &[I64, I64, I64, I32, I64], &[I64, I64]);
    // call_array: (func_tag, func_payload, arr_tag, arr_payload, vm, region_id) -> (tag, payload)
    let call_array_sig = make_sig(module, &[I64, I64, I64, I64, I64, I32], &[I64, I64]);
    // cons: (car_tag, car_pay, cdr_tag, cdr_pay, region, vm) -> (tag, payload).
    // The trailing vm pointer names the heap the cons cell is born on.
    let cons_sig = make_sig(module, &[I64, I64, I64, I64, I32, I64], &[I64, I64]);
    // make_capture: (tag, payload, region: I32, vm) -> (tag, payload). The vm
    // pointer names the heap the cell is born on (the driving instance's own).
    let make_capture_sig = make_sig(module, &[I64, I64, I32, I64], &[I64, I64]);
    // put + jit_ctx: (otag,opay, ktag,kpay, vtag,vpay, jit_ctx) -> (tag, payload).
    // The trailing I64 is the `*mut JitCtx` the intrinsic resolves its VM from.
    let put_ctx_sig = make_sig(module, &[I64, I64, I64, I64, I64, I64, I64], &[I64, I64]);
    // freeze/thaw + jit_ctx: (tag, payload, region: I32, jit_ctx) -> (tag, payload).
    let freeze_ctx_sig = make_sig(module, &[I64, I64, I32, I64], &[I64, I64]);
    // resolve_alloc_region: (vm, slot: I32) -> region: I32
    let resolve_alloc_region_sig = make_sig(module, &[I64, I32], &[I32]);
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
        pair: declare(module, "elle_jit_pair", &cons_sig)?,
        first: declare(module, "elle_jit_first", &value_unary)?,
        rest: declare(module, "elle_jit_rest", &value_unary)?,
        make_array: declare(module, "elle_jit_make_array", &make_array_sig)?,
        materialize_const: declare(module, "elle_jit_materialize_const", &materialize_sig)?,
        is_nil: declare(module, "elle_jit_is_nil", &value_unary)?,
        is_pair: declare(module, "elle_jit_is_pair", &value_unary)?,
        is_array: declare(module, "elle_jit_is_array", &value_unary)?,
        is_array_mut: declare(module, "elle_jit_is_array_mut", &value_unary)?,
        is_struct: declare(module, "elle_jit_is_struct", &value_unary)?,
        is_struct_mut: declare(module, "elle_jit_is_struct_mut", &value_unary)?,
        is_set: declare(module, "elle_jit_is_set", &value_unary)?,
        is_set_mut: declare(module, "elle_jit_is_set_mut", &value_unary)?,
        first_or_nil: declare(module, "elle_jit_first_or_nil", &value_unary)?,
        rest_or_nil: declare(module, "elle_jit_rest_or_nil", &value_unary)?,
        array_len: declare(module, "elle_jit_array_len", &value_unary)?,
        array_ref_or_nil: declare(module, "elle_jit_array_ref_or_nil", &array_ref_or_nil_sig)?,
        match_fail: declare(module, "elle_jit_match_fail", &value_unary_vm)?,
        first_destructure: declare(module, "elle_jit_first_destructure", &value_unary_vm)?,
        rest_destructure: declare(module, "elle_jit_rest_destructure", &value_unary_vm)?,
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
        make_capture: declare(module, "elle_jit_make_capture", &make_capture_sig)?,
        make_capture_owned: declare(module, "elle_jit_make_capture_owned", &value_unary_vm)?,
        // collect_rest_list: (args_ptr, start: I32, nargs: I32, vm) -> (tag, payload)
        collect_rest_list: declare(
            module,
            "elle_jit_collect_rest_list",
            &make_sig(module, &[I64, I32, I32, I64], &[I64, I64]),
        )?,
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
        region_rotate: declare(module, "elle_jit_region_rotate", &void_to_value)?,
        incref_region: declare(
            module,
            "elle_jit_incref_region",
            &make_sig(module, &[I64, I32], &[]),
        )?,
        decref_region: declare(
            module,
            "elle_jit_decref_region",
            &make_sig(module, &[I64, I32], &[]),
        )?,
        decref_value_region: declare(
            module,
            "elle_jit_decref_value_region",
            &make_sig(module, &[I64, I64, I64], &[]),
        )?,
        decref_cell_region: declare(
            module,
            "elle_jit_decref_cell_region",
            &make_sig(module, &[I64, I64, I64], &[]),
        )?,
        incref_value_region: declare(
            module,
            "elle_jit_incref_value_region",
            &make_sig(module, &[I64, I64, I64], &[]),
        )?,
        // adopt_region: (parent_tag, parent_payload, child_tag, child_payload, vm)
        // -> () — both operands are value-resolved (no static slot), mirroring
        // `IncrefValueRegion`/`DecrefValueRegion`.
        adopt_region: declare(
            module,
            "elle_jit_adopt_region",
            &make_sig(module, &[I64, I64, I64, I64, I64], &[]),
        )?,
        // adopt_cell_region: same ABI as adopt_region — (parent, child, vm) -> ()
        // — but the helper resolves both operands with `region_of`, so a capture
        // cell's OWN region is adopted (the cell↔closure containment).
        adopt_cell_region: declare(
            module,
            "elle_jit_adopt_cell_region",
            &make_sig(module, &[I64, I64, I64, I64, I64], &[]),
        )?,
        // adopt_into_activation: (child_tag, child_payload, vm) -> () — the
        // child is value-resolved; the parent (the activation's owner node) is
        // VM state, so no parent operand exists.
        adopt_into_activation: declare(
            module,
            "elle_jit_adopt_into_activation",
            &make_sig(module, &[I64, I64, I64], &[]),
        )?,
        // release_activation_owner_node: (vm) -> () — the compiled Return
        // path's owner-node free.
        release_activation_owner_node: declare(
            module,
            "elle_jit_release_activation_owner_node",
            &make_sig(module, &[I64], &[]),
        )?,
        // free_region_group: (members_ptr, count, vm) -> () — the members are
        // spilled to a stack slot as Value pairs, exactly like push_param_frame.
        free_region_group: declare(
            module,
            "elle_jit_free_region_group",
            &make_sig(module, &[I64, I64, I64], &[]),
        )?,
        push_region_map: declare(module, "elle_jit_push_region_map", &vm_to_void)?,
        pop_region_map: declare(module, "elle_jit_pop_region_map", &vm_to_void)?,
        resolve_alloc_region: declare(
            module,
            "elle_jit_resolve_alloc_region",
            &resolve_alloc_region_sig,
        )?,
        resolve_alloc_region_merged: declare(
            module,
            "elle_jit_resolve_alloc_region_merged",
            &resolve_alloc_region_sig,
        )?,
        rotate_pools: declare(module, "elle_jit_rotate_pools", &vm_to_void)?,
        incref: declare(module, "elle_jit_incref", &value_unary)?,
        decref: declare(module, "elle_jit_decref", &value_unary)?,
        // New intrinsic helpers
        is_empty: declare(module, "elle_jit_is_empty", &value_unary)?,
        is_bool: declare(module, "elle_jit_is_bool", &value_unary)?,
        is_int: declare(module, "elle_jit_is_int", &value_unary)?,
        is_float: declare(module, "elle_jit_is_float", &value_unary)?,
        is_string: declare(module, "elle_jit_is_string", &value_unary)?,
        is_keyword: declare(module, "elle_jit_is_keyword", &value_unary)?,
        is_symbol_check: declare(module, "elle_jit_is_symbol_check", &value_unary)?,
        is_bytes: declare(module, "elle_jit_is_bytes", &value_unary)?,
        is_box: declare(module, "elle_jit_is_box", &value_unary)?,
        is_closure: declare(module, "elle_jit_is_closure", &value_unary)?,
        is_fiber: declare(module, "elle_jit_is_fiber", &value_unary)?,
        type_of: declare(module, "elle_jit_type_of", &value_unary)?,
        length: declare(module, "elle_jit_length", &value_unary)?,
        // get/pop allocate nothing through a PrimFn and read no VM state, so they
        // take no `JitCtx`. The rest run `PrimFn` bodies and resolve their VM from
        // the threaded `JitCtx` (trailing I64), keeping the VM dependency explicit
        // (docs/impl/region/ctx.md "JIT intrinsic helpers reach the VM through a
        // JitCtx").
        get: declare(module, "elle_jit_get", &value_binary)?,
        put: declare(module, "elle_jit_put", &put_ctx_sig)?,
        del: declare(module, "elle_jit_del", &value_binary_vm)?,
        has: declare(module, "elle_jit_has", &value_binary_vm)?,
        intr_push: declare(module, "elle_jit_push", &value_binary_vm)?,
        intr_string_push: declare(module, "elle_jit_string_push", &value_binary_vm)?,
        intr_bytes_push: declare(module, "elle_jit_bytes_push", &value_binary_vm)?,
        pop: declare(module, "elle_jit_pop", &value_unary_vm)?,
        // freeze/thaw: (tag, payload, region: I32, jit_ctx) -> (tag, payload). The
        // I32 region is the emitter-resolved physical SLOT region (these are
        // `IntrinsicOp::allocates` ops with a `DecrefRegion(slot)`), threaded so
        // the fresh copy is born in that region; the trailing `jit_ctx` carries
        // the VM whose heap it is born on.
        freeze: declare(module, "elle_jit_freeze", &freeze_ctx_sig)?,
        thaw: declare(module, "elle_jit_thaw", &freeze_ctx_sig)?,
        identical: declare(module, "elle_jit_identical", &value_binary)?,
    })
}
