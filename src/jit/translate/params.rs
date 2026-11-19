// audited: 2026-11-18
//! Binding a call's arguments to the parameter variables, for the prologue and for a self tail call alike.
//!
//! docs/impl/jit.md
//! docs/impl/region/relocate.md
//!
//! The interpreter builds a callee's environment in `populate_env`
//! (src/vm/env.rs). `bind_params` is its compiled twin, and the one place
//! compiled code binds a parameter.

use super::*;

/// Where a binding runs, which decides what a rest list owes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BindSite {
    /// The prologue, on a call's arguments. A tail callee's moved surplus is
    /// the trampoline's to release.
    Entry,
    /// A self tail call's loop, on arguments the call moves. No trampoline
    /// runs, so the rest list releases the moved surplus itself.
    SelfTail,
}

impl<'a> FunctionTranslator<'a> {
    /// Is parameter `i` held by a capture cell rather than by its variable?
    pub(crate) fn param_is_celled(&self, i: usize) -> bool {
        self.lir.param_is_celled(i)
    }

    /// Bind every parameter from the `nargs` arguments at `args_ptr`. A
    /// missing optional parameter binds nil, a rest parameter binds a list of
    /// the arguments past the positional ones, and a celled parameter binds a
    /// fresh cell around its value.
    pub(crate) fn bind_params(
        &mut self,
        builder: &mut FunctionBuilder,
        args_ptr: cranelift_codegen::ir::Value,
        nargs: cranelift_codegen::ir::Value,
        site: BindSite,
    ) -> Result<(), JitError> {
        let arity = self.lir.arity();
        let list_variadic = matches!(arity, crate::value::Arity::AtLeast(_))
            && matches!(self.lir.vararg_kind(), crate::hir::VarargKind::List);
        let required = arity.fixed_params() as u32;
        let positional = if list_variadic {
            self.lir.num_params() as u32 - 1
        } else if matches!(arity, crate::value::Arity::Range(_, _)) {
            self.lir.num_params() as u32
        } else {
            required
        };

        for i in 0..positional {
            let (tag, payload) = if i < required {
                load_value_slot(builder, args_ptr, i)
            } else {
                self.load_optional_arg(builder, args_ptr, nargs, i)
            };
            self.def_param(builder, i, tag, payload)?;
        }

        if list_variadic {
            // Regions of the list's own, one per cons or one for the list as
            // `rest_list_layout` says (docs/impl/region/restlist.md), so the
            // list is independent of a JIT->JIT callee's region.
            let collect = match site {
                BindSite::Entry => self.helpers.collect_rest_list,
                BindSite::SelfTail => self.helpers.collect_moved_rest_list,
            };
            let vm = self
                .vm_ptr
                .ok_or_else(|| JitError::InvalidLir("rest list without vm pointer".to_string()))?;
            let start = builder.ins().iconst(I32, positional as i64);
            let nargs_i32 = builder.ins().ireduce(I32, nargs);
            let layout = builder
                .ins()
                .iconst(I32, self.lir.rest_list_layout() as i64);
            let func_ref = self.module.declare_func_in_func(collect, builder.func);
            let call = builder
                .ins()
                .call(func_ref, &[args_ptr, start, nargs_i32, layout, vm]);
            let (tag, payload) = (builder.inst_results(call)[0], builder.inst_results(call)[1]);
            self.def_param(builder, positional, tag, payload)?;
        }
        Ok(())
    }

    /// The `i`-th argument where `nargs` covers it, and nil where it does not.
    fn load_optional_arg(
        &mut self,
        builder: &mut FunctionBuilder,
        args_ptr: cranelift_codegen::ir::Value,
        nargs: cranelift_codegen::ir::Value,
        i: u32,
    ) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
        let threshold = builder.ins().iconst(I64, i as i64 + 1);
        let has_arg = builder
            .ins()
            .icmp(IntCC::UnsignedGreaterThanOrEqual, nargs, threshold);
        let then_block = builder.create_block();
        let else_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, I64);
        builder.append_block_param(merge_block, I64);
        builder
            .ins()
            .brif(has_arg, then_block, &[], else_block, &[]);

        builder.switch_to_block(then_block);
        builder.seal_block(then_block);
        let (tag, payload) = load_value_slot(builder, args_ptr, i);
        builder.ins().jump(
            merge_block,
            &[BlockArg::Value(tag), BlockArg::Value(payload)],
        );

        builder.switch_to_block(else_block);
        builder.seal_block(else_block);
        let nil_tag = builder.ins().iconst(I64, TAG_NIL as i64);
        let nil_payload = builder.ins().iconst(I64, 0);
        builder.ins().jump(
            merge_block,
            &[BlockArg::Value(nil_tag), BlockArg::Value(nil_payload)],
        );

        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        (
            builder.block_params(merge_block)[0],
            builder.block_params(merge_block)[1],
        )
    }

    /// Define parameter `i`'s variable: the value itself, or a cell around it
    /// in a region of the cell's own, as the interpreter's `push_param` mints
    /// one (src/vm/env.rs).
    fn def_param(
        &mut self,
        builder: &mut FunctionBuilder,
        i: u32,
        tag: cranelift_codegen::ir::Value,
        payload: cranelift_codegen::ir::Value,
    ) -> Result<(), JitError> {
        let (tag, payload) = if self.param_is_celled(i as usize) {
            let vm = self
                .vm_ptr
                .ok_or_else(|| JitError::InvalidLir("cell without vm pointer".to_string()))?;
            self.call_helper_value_vm(builder, self.helpers.make_capture_owned, tag, payload, vm)?
        } else {
            (tag, payload)
        };
        self.def_var_pair(builder, self.arg_var_base + i, tag, payload);
        Ok(())
    }
}
