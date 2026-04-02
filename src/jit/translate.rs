//! LIR to Cranelift IR translation
//!
//! This module contains `FunctionTranslator`, which translates individual
//! LIR instructions and terminators to Cranelift IR.
//!
//! ## Variable layout
//!
//! Each LIR register `r` maps to TWO Cranelift variables:
//!   - tag:     `Variable::from_u32(2 * r)`
//!   - payload: `Variable::from_u32(2 * r + 1)`
//!
//! Arg variables and local variables use the same doubling scheme starting
//! at their respective bases.

use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::{InstBuilder, MemFlags};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};

use crate::lir::{Label, LirInstr, Reg, Terminator};
use crate::value::fiber::SignalBits;
use crate::value::repr::{TAG_FALSE, TAG_NIL, TAG_TRUE};
use crate::value::SymbolId;

use super::vtable::RuntimeHelpers;
use super::JitError;

/// Helper to create a Cranelift Variable from a slot index
#[inline]
fn var(n: u32) -> Variable {
    Variable::from_u32(n)
}

/// Translator for a single function
pub(crate) struct FunctionTranslator<'a> {
    pub(crate) module: &'a mut JITModule,
    pub(crate) helpers: &'a RuntimeHelpers,
    pub(crate) lir: &'a crate::lir::LirFunction,
    pub(crate) env_ptr: Option<cranelift_codegen::ir::Value>,
    pub(crate) vm_ptr: Option<cranelift_codegen::ir::Value>,
    /// (tag, payload) Cranelift values for the closure being executed
    /// (for self-tail-call detection)
    pub(crate) self_tag_payload:
        Option<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value)>,
    /// Base index for arg variables (= num_regs), in LIR register space
    pub(crate) arg_var_base: u32,
    /// Base index for locally-defined variable Cranelift variables
    pub(crate) local_var_base: u32,
    /// Loop header block for self-tail-call jumps
    pub(crate) loop_header: Option<cranelift_codegen::ir::Block>,
    /// SCC peer functions
    pub(crate) scc_peers: HashMap<SymbolId, FuncId>,
    /// Map from register to the SymbolId it was loaded from.
    pub(crate) global_load_map: HashMap<Reg, SymbolId>,
    /// SymbolId of the function being compiled (for self-call detection)
    pub(crate) self_sym: Option<SymbolId>,
    /// Counter for yield point indices
    pub(crate) yield_point_index: u32,
    /// Counter for call site indices
    pub(crate) call_site_index: u32,
    /// Shared stack slot for spilling locals + operands at yield/call sites.
    pub(crate) shared_spill_slot: Option<cranelift_codegen::ir::StackSlot>,
    /// Closure template Values built during MakeClosure translation.
    pub(crate) closure_constants: Vec<crate::value::Value>,
    /// Symbol name map for nested emitters (MakeClosure).
    pub(crate) symbol_names: HashMap<u32, String>,
}

impl<'a> FunctionTranslator<'a> {
    pub(crate) fn new(
        module: &'a mut JITModule,
        helpers: &'a RuntimeHelpers,
        lir: &'a crate::lir::LirFunction,
    ) -> Self {
        FunctionTranslator {
            module,
            helpers,
            lir,
            env_ptr: None,
            vm_ptr: None,
            self_tag_payload: None,
            arg_var_base: 0,
            local_var_base: 0,
            loop_header: None,
            scc_peers: HashMap::new(),
            global_load_map: HashMap::new(),
            self_sym: None,
            yield_point_index: 0,
            call_site_index: 0,
            shared_spill_slot: None,
            closure_constants: Vec::new(),
            symbol_names: HashMap::new(),
        }
    }

    /// Convert LIR register index to the Cranelift variable index for its tag.
    /// Variable layout: 2*r = tag, 2*r+1 = payload.
    #[inline]
    pub(crate) fn var_tag(&self, r: u32) -> u32 {
        2 * r
    }

    /// Convert LIR register index to the Cranelift variable index for its payload.
    #[inline]
    pub(crate) fn var_payload(&self, r: u32) -> u32 {
        2 * r + 1
    }

    /// Initialize locally-defined variables.
    pub(crate) fn init_locally_defined_vars(
        &mut self,
        builder: &mut FunctionBuilder,
        num_locally_defined: u32,
    ) -> Result<(), JitError> {
        let nil_tag = builder.ins().iconst(I64, TAG_NIL as i64);
        let zero = builder.ins().iconst(I64, 0);
        let lbox_locals_mask = self.lir.lbox_locals_mask;

        // The first num_local_params slots are non-LBox param copies
        // (initialized at function entry). lbox_locals_mask indexes from
        // the first let-bound local (after param copies).
        let nlp = self.lir.num_local_params as u32;

        for i in 0..num_locally_defined {
            let base = self.local_var_base + i;
            let mask_bit = i.saturating_sub(nlp);
            let needs_lbox =
                i >= nlp && (mask_bit >= 64 || (lbox_locals_mask & (1 << mask_bit)) != 0);
            if needs_lbox {
                let (cell_tag, cell_payload) =
                    self.call_helper_value_unary(builder, self.helpers.make_lbox, nil_tag, zero)?;
                builder.def_var(var(self.var_tag(base)), cell_tag);
                builder.def_var(var(self.var_payload(base)), cell_payload);
            } else {
                builder.def_var(var(self.var_tag(base)), nil_tag);
                builder.def_var(var(self.var_payload(base)), zero);
            }
        }

        Ok(())
    }

    /// Translate a single LIR instruction.
    /// Returns true if the instruction emitted a terminator (e.g., TailCall).
    pub(crate) fn translate_instr(
        &mut self,
        builder: &mut FunctionBuilder,
        instr: &LirInstr,
        _block_map: &HashMap<Label, cranelift_codegen::ir::Block>,
    ) -> Result<bool, JitError> {
        match instr {
            LirInstr::Const { dst, value } => {
                let (tag, payload) = self.translate_const(builder, value);
                self.def_var_pair(builder, dst.0, tag, payload);
            }

            LirInstr::ValueConst { dst, value } => {
                let tag = builder.ins().iconst(I64, value.tag as i64);
                let payload = builder.ins().iconst(I64, value.payload as i64);
                self.def_var_pair(builder, dst.0, tag, payload);
            }

            LirInstr::LoadLocal { dst, slot } => {
                let base = self.local_slot_to_var(*slot);
                let (tag, payload) = self.use_var_pair(builder, base);
                self.def_var_pair(builder, dst.0, tag, payload);
            }

            LirInstr::StoreLocal { slot, src } => {
                let base = self.local_slot_to_var(*slot);
                let (tag, payload) = self.use_var_pair(builder, src.0);
                self.def_var_pair(builder, base, tag, payload);
            }

            LirInstr::LoadCapture { dst, index } => {
                let num_captures = self.lir.num_captures;
                let arity = self.lir.num_params as u16;
                if *index < num_captures {
                    // Load from closure environment (captures)
                    // Each Value is 16 bytes: tag at offset i*16, payload at i*16+8
                    let env_ptr = self.env_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCapture without env pointer".to_string())
                    })?;
                    let tag_offset = (*index as i32) * 16;
                    let payload_offset = (*index as i32) * 16 + 8;
                    let raw_tag = builder
                        .ins()
                        .load(I64, MemFlags::trusted(), env_ptr, tag_offset);
                    let raw_payload =
                        builder
                            .ins()
                            .load(I64, MemFlags::trusted(), env_ptr, payload_offset);
                    // Auto-unwrap LocalCell if present
                    let (val_tag, val_payload) = self.call_helper_value_unary(
                        builder,
                        self.helpers.load_capture,
                        raw_tag,
                        raw_payload,
                    )?;
                    self.def_var_pair(builder, dst.0, val_tag, val_payload);
                } else if *index < num_captures + arity {
                    // Load from arg variables
                    let param_index = *index - num_captures;
                    let base = self.arg_var_base + param_index as u32;
                    let (tag, payload) = self.use_var_pair(builder, base);
                    if (param_index as u32) < 64
                        && (self.lir.lbox_params_mask & (1 << param_index)) != 0
                    {
                        // cell-wrapped param: auto-unwrap via load_lbox
                        let (rt, rp) = self.call_helper_value_unary(
                            builder,
                            self.helpers.load_lbox,
                            tag,
                            payload,
                        )?;
                        self.def_var_pair(builder, dst.0, rt, rp);
                    } else {
                        self.def_var_pair(builder, dst.0, tag, payload);
                    }
                } else {
                    // Locally-defined variable
                    let local_index = *index - num_captures - arity;
                    let jit_slot = self.lir.num_local_params as u32 + local_index as u32;
                    let base = self.local_var_base + jit_slot;
                    let (tag, payload) = self.use_var_pair(builder, base);
                    let needs_lbox = (local_index as u32) >= 64
                        || (self.lir.lbox_locals_mask & (1 << local_index)) != 0;
                    if needs_lbox {
                        // cell-wrapped: auto-unwrap via load_lbox
                        let (rt, rp) = self.call_helper_value_unary(
                            builder,
                            self.helpers.load_lbox,
                            tag,
                            payload,
                        )?;
                        self.def_var_pair(builder, dst.0, rt, rp);
                    } else {
                        self.def_var_pair(builder, dst.0, tag, payload);
                    }
                }
            }

            LirInstr::LoadCaptureRaw { dst, index } => {
                let num_captures = self.lir.num_captures;
                let arity = self.lir.num_params as u16;
                if *index < num_captures {
                    let env_ptr = self.env_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCaptureRaw without env pointer".to_string())
                    })?;
                    let tag_offset = (*index as i32) * 16;
                    let payload_offset = (*index as i32) * 16 + 8;
                    let raw_tag = builder
                        .ins()
                        .load(I64, MemFlags::trusted(), env_ptr, tag_offset);
                    let raw_payload =
                        builder
                            .ins()
                            .load(I64, MemFlags::trusted(), env_ptr, payload_offset);
                    self.def_var_pair(builder, dst.0, raw_tag, raw_payload);
                } else if *index < num_captures + arity {
                    let param_index = *index - num_captures;
                    let base = self.arg_var_base + param_index as u32;
                    let (tag, payload) = self.use_var_pair(builder, base);
                    self.def_var_pair(builder, dst.0, tag, payload);
                } else {
                    let local_index = *index - num_captures - arity;
                    let jit_slot = self.lir.num_local_params as u32 + local_index as u32;
                    let base = self.local_var_base + jit_slot;
                    let (tag, payload) = self.use_var_pair(builder, base);
                    self.def_var_pair(builder, dst.0, tag, payload);
                }
            }

            LirInstr::BinOp { dst, op, lhs, rhs } => {
                let (lt, lp) = self.use_var_pair(builder, lhs.0);
                let (rt, rp) = self.use_var_pair(builder, rhs.0);
                let (rt2, rp2) = self.call_binary_helper(builder, *op, lt, lp, rt, rp)?;
                self.def_var_pair(builder, dst.0, rt2, rp2);
            }

            LirInstr::UnaryOp { dst, op, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) = self.call_unary_helper(builder, *op, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::Compare { dst, op, lhs, rhs } => {
                let (lt, lp) = self.use_var_pair(builder, lhs.0);
                let (rt, rp) = self.use_var_pair(builder, rhs.0);
                let (crt, crp) = self.call_compare_helper(builder, *op, lt, lp, rt, rp)?;
                self.def_var_pair(builder, dst.0, crt, crp);
            }

            LirInstr::IsNil { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_nil, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::IsPair { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_pair, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::Cons { dst, head, tail } => {
                let (ht, hp) = self.use_var_pair(builder, head.0);
                let (tt, tp) = self.use_var_pair(builder, tail.0);
                let (rt, rp) =
                    self.call_helper_value_binary(builder, self.helpers.cons, ht, hp, tt, tp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::Car { dst, pair } => {
                let (pt, pp) = self.use_var_pair(builder, pair.0);
                let (rt, rp) = self.call_helper_value_unary(builder, self.helpers.car, pt, pp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::Cdr { dst, pair } => {
                let (pt, pp) = self.use_var_pair(builder, pair.0);
                let (rt, rp) = self.call_helper_value_unary(builder, self.helpers.cdr, pt, pp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::MakeArrayMut { dst, elements } => {
                if elements.is_empty() {
                    let null_ptr = builder.ins().iconst(I64, 0);
                    let count = builder.ins().iconst(I64, 0);
                    let func_ref = self
                        .module
                        .declare_func_in_func(self.helpers.make_array, builder.func);
                    let call = builder.ins().call(func_ref, &[null_ptr, count]);
                    let rt = builder.inst_results(call)[0];
                    let rp = builder.inst_results(call)[1];
                    self.def_var_pair(builder, dst.0, rt, rp);
                } else {
                    // Each Value is 16 bytes on the stack
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (elements.len() * 16) as u32,
                            0,
                        ));
                    for (i, elem_reg) in elements.iter().enumerate() {
                        let (et, ep) = self.use_var_pair(builder, elem_reg.0);
                        let tag_offset = (i * 16) as i32;
                        let payload_offset = (i * 16 + 8) as i32;
                        builder.ins().stack_store(et, slot, tag_offset);
                        builder.ins().stack_store(ep, slot, payload_offset);
                    }
                    let elements_addr = builder.ins().stack_addr(I64, slot, 0);
                    let count = builder.ins().iconst(I64, elements.len() as i64);
                    let func_ref = self
                        .module
                        .declare_func_in_func(self.helpers.make_array, builder.func);
                    let call = builder.ins().call(func_ref, &[elements_addr, count]);
                    let rt = builder.inst_results(call)[0];
                    let rp = builder.inst_results(call)[1];
                    self.def_var_pair(builder, dst.0, rt, rp);
                }
            }

            LirInstr::MakeLBox { dst, value } => {
                let (vt, vp) = self.use_var_pair(builder, value.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.make_lbox, vt, vp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::LoadLBox { dst, cell } => {
                let (ct, cp) = self.use_var_pair(builder, cell.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.load_lbox, ct, cp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::StoreLBox { cell, value } => {
                let (ct, cp) = self.use_var_pair(builder, cell.0);
                let (vt, vp) = self.use_var_pair(builder, value.0);
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.store_lbox, builder.func);
                let call = builder.ins().call(func_ref, &[ct, cp, vt, vp]);
                let _ = builder.inst_results(call);
            }

            LirInstr::StoreCapture { index, src } => {
                let num_captures = self.lir.num_captures;
                let arity = self.lir.num_params as u16;
                let (vt, vp) = self.use_var_pair(builder, src.0);

                if *index < num_captures {
                    // Store to a capture slot in the closure env
                    let env_ptr = self.env_ptr.ok_or_else(|| {
                        JitError::InvalidLir("StoreCapture without env pointer".to_string())
                    })?;
                    let idx_val = builder.ins().iconst(I64, *index as i64);
                    let func_ref = self
                        .module
                        .declare_func_in_func(self.helpers.store_capture, builder.func);
                    let call = builder.ins().call(func_ref, &[env_ptr, idx_val, vt, vp]);
                    let _ = builder.inst_results(call);
                } else if *index < num_captures + arity {
                    // Store to a param slot — use store_lbox if lbox-wrapped
                    let param_index = *index - num_captures;
                    let base = self.arg_var_base + param_index as u32;
                    if (param_index as u32) < 64
                        && (self.lir.lbox_params_mask & (1 << param_index)) != 0
                    {
                        let (ct, cp) = self.use_var_pair(builder, base);
                        let func_ref = self
                            .module
                            .declare_func_in_func(self.helpers.store_lbox, builder.func);
                        let call = builder.ins().call(func_ref, &[ct, cp, vt, vp]);
                        let _ = builder.inst_results(call);
                    } else {
                        self.def_var_pair(builder, base, vt, vp);
                    }
                } else {
                    let local_index = *index - num_captures - arity;
                    let jit_slot = self.lir.num_local_params as u32 + local_index as u32;
                    let base = self.local_var_base + jit_slot;
                    // Use store_lbox if the mask says so, OR if beyond bit 63
                    // (conservative — matches emitter which treats these as cell locals)
                    let needs_lbox = (local_index as u32) >= 64
                        || (self.lir.lbox_locals_mask & (1 << local_index)) != 0;
                    if needs_lbox {
                        let (ct, cp) = self.use_var_pair(builder, base);
                        let func_ref = self
                            .module
                            .declare_func_in_func(self.helpers.store_lbox, builder.func);
                        let call = builder.ins().call(func_ref, &[ct, cp, vt, vp]);
                        let _ = builder.inst_results(call);
                    } else {
                        self.def_var_pair(builder, base, vt, vp);
                    }
                }
            }

            LirInstr::Call { dst, func, args } => {
                let (ft, fp) = self.use_var_pair(builder, func.0);
                let vm = self
                    .vm_ptr
                    .ok_or_else(|| JitError::InvalidLir("Call without vm pointer".to_string()))?;

                let maybe_scc = self
                    .global_load_map
                    .get(func)
                    .and_then(|&sym| self.scc_peers.get(&sym).map(|&fid| (sym, fid)));
                if let Some((sym, peer_func_id)) = maybe_scc {
                    // Call depth check
                    let (overflow_tag, _) =
                        self.call_helper_vm_only(builder, self.helpers.call_depth_enter, vm)?;
                    let tag_true = builder.ins().iconst(I64, TAG_TRUE as i64);
                    let is_overflow = builder.ins().icmp(IntCC::Equal, overflow_tag, tag_true);
                    let overflow_block = builder.create_block();
                    let call_block = builder.create_block();
                    builder
                        .ins()
                        .brif(is_overflow, overflow_block, &[], call_block, &[]);

                    builder.switch_to_block(overflow_block);
                    builder.seal_block(overflow_block);
                    let nil_t = builder.ins().iconst(I64, TAG_NIL as i64);
                    let zero = builder.ins().iconst(I64, 0);
                    builder.ins().return_(&[nil_t, zero]);

                    builder.switch_to_block(call_block);
                    builder.seal_block(call_block);

                    let (rt, rp) =
                        self.emit_direct_scc_call(builder, peer_func_id, sym, args, vm)?;
                    self.call_helper_vm_only(builder, self.helpers.call_depth_exit, vm)?;
                    // Resolve pending tail call
                    let func_ref = self
                        .module
                        .declare_func_in_func(self.helpers.resolve_tail_call, builder.func);
                    let call = builder.ins().call(func_ref, &[rt, rp, vm]);
                    let resolved_t = builder.inst_results(call)[0];
                    let resolved_p = builder.inst_results(call)[1];
                    self.def_var_pair(builder, dst.0, resolved_t, resolved_p);
                    self.emit_exception_check_after_call(builder)?;
                    if self.lir.signal.may_suspend() {
                        let idx = self.call_site_index;
                        self.call_site_index += 1;
                        self.emit_yield_check_after_call(builder, idx)?;
                    }
                } else if args.is_empty() {
                    let null_ptr = builder.ins().iconst(I64, 0);
                    let nargs = builder.ins().iconst(I64, 0);
                    let (rt, rp) = self.call_helper_call(builder, ft, fp, null_ptr, nargs, vm)?;
                    self.def_var_pair(builder, dst.0, rt, rp);
                    self.emit_exception_check_after_call(builder)?;
                    if self.lir.signal.may_suspend() {
                        let idx = self.call_site_index;
                        self.call_site_index += 1;
                        self.emit_yield_check_after_call(builder, idx)?;
                    }
                } else {
                    // Spill args to stack (16 bytes each)
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (args.len() * 16) as u32,
                            0,
                        ));
                    for (i, arg_reg) in args.iter().enumerate() {
                        let (at, ap) = self.use_var_pair(builder, arg_reg.0);
                        let tag_offset = (i * 16) as i32;
                        let payload_offset = (i * 16 + 8) as i32;
                        builder.ins().stack_store(at, slot, tag_offset);
                        builder.ins().stack_store(ap, slot, payload_offset);
                    }
                    let args_addr = builder.ins().stack_addr(I64, slot, 0);
                    let nargs = builder.ins().iconst(I64, args.len() as i64);
                    let (rt, rp) = self.call_helper_call(builder, ft, fp, args_addr, nargs, vm)?;
                    self.def_var_pair(builder, dst.0, rt, rp);
                    self.emit_exception_check_after_call(builder)?;
                    if self.lir.signal.may_suspend() {
                        let idx = self.call_site_index;
                        self.call_site_index += 1;
                        self.emit_yield_check_after_call(builder, idx)?;
                    }
                }
            }

            LirInstr::TailCall { func, args } => {
                let (ft, fp) = self.use_var_pair(builder, func.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("TailCall without vm pointer".to_string())
                })?;

                // Self-tail-call optimization
                if let (Some((self_tag, self_payload)), Some(loop_header)) =
                    (self.self_tag_payload, self.loop_header)
                {
                    if args.len() == self.lir.num_params {
                        // Check if func == self (tag AND payload match)
                        let tag_eq = builder.ins().icmp(IntCC::Equal, ft, self_tag);
                        let pay_eq = builder.ins().icmp(IntCC::Equal, fp, self_payload);
                        let is_self = builder.ins().band(tag_eq, pay_eq);

                        let self_call_block = builder.create_block();
                        let other_call_block = builder.create_block();
                        builder
                            .ins()
                            .brif(is_self, self_call_block, &[], other_call_block, &[]);

                        // Self-call path
                        builder.switch_to_block(self_call_block);
                        builder.seal_block(self_call_block);

                        let new_arg_vals: Vec<(
                            cranelift_codegen::ir::Value,
                            cranelift_codegen::ir::Value,
                        )> = args
                            .iter()
                            .map(|arg_reg| self.use_var_pair(builder, arg_reg.0))
                            .collect();

                        for (i, (at, ap)) in new_arg_vals.into_iter().enumerate() {
                            let base = self.arg_var_base + i as u32;
                            self.def_var_pair(builder, base, at, ap);
                        }
                        builder.ins().jump(loop_header, &[]);

                        // Other-call path
                        builder.switch_to_block(other_call_block);
                        builder.seal_block(other_call_block);

                        let maybe_scc2 = self
                            .global_load_map
                            .get(func)
                            .and_then(|&sym| self.scc_peers.get(&sym).map(|&fid| (sym, fid)));
                        if let Some((sym2, peer_func_id)) = maybe_scc2 {
                            let (rt, rp) =
                                self.emit_direct_scc_call(builder, peer_func_id, sym2, args, vm)?;
                            builder.ins().return_(&[rt, rp]);
                        } else {
                            let (rt, rp) =
                                self.emit_tail_call_with_args(builder, ft, fp, args, vm)?;
                            builder.ins().return_(&[rt, rp]);
                        }
                        return Ok(true);
                    }
                }

                // Fallback: no self-tail-call optimization
                let maybe_scc3 = self
                    .global_load_map
                    .get(func)
                    .and_then(|&sym| self.scc_peers.get(&sym).map(|&fid| (sym, fid)));
                if let Some((sym3, peer_func_id)) = maybe_scc3 {
                    let (rt, rp) =
                        self.emit_direct_scc_call(builder, peer_func_id, sym3, args, vm)?;
                    builder.ins().return_(&[rt, rp]);
                    return Ok(true);
                }

                let (rt, rp) = self.emit_tail_call_with_args(builder, ft, fp, args, vm)?;
                builder.ins().return_(&[rt, rp]);
                return Ok(true);
            }

            LirInstr::MakeClosure {
                dst,
                func,
                captures,
            } => {
                let mut emitter = crate::lir::Emitter::new_with_symbols(self.symbol_names.clone());
                let (nested_bytecode, nested_yield_points, nested_call_sites) = emitter.emit(func);
                let mut nested_lir = func.as_ref().clone();
                nested_lir.yield_points = nested_yield_points;
                nested_lir.call_sites = nested_call_sites;

                let template = crate::value::ClosureTemplate {
                    bytecode: std::rc::Rc::new(nested_bytecode.instructions),
                    arity: func.arity,
                    num_locals: func.num_locals as usize,
                    num_captures: captures.len(),
                    num_params: func.num_params,
                    constants: std::rc::Rc::new(nested_bytecode.constants),
                    signal: func.signal,
                    lbox_params_mask: func.lbox_params_mask,
                    lbox_locals_mask: func.lbox_locals_mask,
                    symbol_names: std::rc::Rc::new(nested_bytecode.symbol_names),
                    location_map: std::rc::Rc::new(nested_bytecode.location_map),
                    lir_function: Some(std::rc::Rc::new(nested_lir)),
                    doc: func.doc,
                    syntax: func.syntax.clone(),
                    vararg_kind: func.vararg_kind.clone(),
                    name: func.name.clone().map(|s| std::rc::Rc::from(s.as_str())),
                    result_is_immediate: func.result_is_immediate,
                    has_outward_heap_set: func.has_outward_heap_set,
                    wasm_func_idx: None,
                };
                let template_closure = crate::value::Closure {
                    template: std::rc::Rc::new(template),
                    env: std::rc::Rc::new(vec![]),
                    squelch_mask: SignalBits::EMPTY,
                };
                let template_value = crate::value::Value::closure(template_closure);

                self.closure_constants.push(template_value);

                let template_tag = builder.ins().iconst(I64, template_value.tag as i64);
                let template_payload = builder.ins().iconst(I64, template_value.payload as i64);

                // Spill captures to a stack slot (16 bytes each)
                let (captures_ptr, count_val) = if captures.is_empty() {
                    (builder.ins().iconst(I64, 0), builder.ins().iconst(I64, 0))
                } else {
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (captures.len() * 16) as u32,
                            0,
                        ));
                    for (i, cap_reg) in captures.iter().enumerate() {
                        let (ct, cp) = self.use_var_pair(builder, cap_reg.0);
                        let tag_offset = (i * 16) as i32;
                        let payload_offset = (i * 16 + 8) as i32;
                        builder.ins().stack_store(ct, slot, tag_offset);
                        builder.ins().stack_store(cp, slot, payload_offset);
                    }
                    let ptr = builder.ins().stack_addr(I64, slot, 0);
                    let cnt = builder.ins().iconst(I64, captures.len() as i64);
                    (ptr, cnt)
                };

                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.make_closure, builder.func);
                let call = builder.ins().call(
                    func_ref,
                    &[template_tag, template_payload, captures_ptr, count_val],
                );
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::LoadResumeValue { dst } => {
                // Resume goes through the interpreter. Emit NIL as dead code.
                let nil_t = builder.ins().iconst(I64, TAG_NIL as i64);
                let zero = builder.ins().iconst(I64, 0);
                self.def_var_pair(builder, dst.0, nil_t, zero);
            }

            LirInstr::CarDestructure { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("CarDestructure without vm pointer".to_string())
                })?;
                let (rt, rp) =
                    self.call_helper_value_vm(builder, self.helpers.car_destructure, st, sp, vm)?;
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::CdrDestructure { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("CdrDestructure without vm pointer".to_string())
                })?;
                let (rt, rp) =
                    self.call_helper_value_vm(builder, self.helpers.cdr_destructure, st, sp, vm)?;
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::ArrayMutRefDestructure { dst, src, index } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let idx_val = builder.ins().iconst(I64, *index as i64);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("ArrayMutRefDestructure without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.array_ref_destructure, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, idx_val, vm]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::ArrayMutSliceFrom { dst, src, index } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let idx_val = builder.ins().iconst(I64, *index as i64);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("ArrayMutSliceFrom without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.array_slice_from, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, idx_val, vm]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::IsArray { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_array, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::IsArrayMut { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_array_mut, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::IsStruct { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_struct, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::IsStructMut { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_struct_mut, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::ArrayMutLen { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.array_len, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::StructGetOrNil { dst, src, key } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (kt, kp) = self.translate_const(builder, key);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("StructGetOrNil without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.struct_get_or_nil, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, kt, kp, vm]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::StructGetDestructure { dst, src, key } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (kt, kp) = self.translate_const(builder, key);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("StructGetDestructure without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.struct_get_destructure, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, kt, kp, vm]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::StructRest {
                dst,
                src,
                exclude_keys,
            } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("StructRest without vm pointer".to_string())
                })?;
                let count = exclude_keys.len();
                let (exclude_ptr, count_val) = if count == 0 {
                    (builder.ins().iconst(I64, 0), builder.ins().iconst(I64, 0))
                } else {
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (count * 16) as u32,
                            0,
                        ));
                    for (i, key) in exclude_keys.iter().enumerate() {
                        let (kt, kp) = self.translate_const(builder, key);
                        let tag_offset = (i * 16) as i32;
                        let payload_offset = (i * 16 + 8) as i32;
                        builder.ins().stack_store(kt, slot, tag_offset);
                        builder.ins().stack_store(kp, slot, payload_offset);
                    }
                    let ptr = builder.ins().stack_addr(I64, slot, 0);
                    let cnt = builder.ins().iconst(I64, count as i64);
                    (ptr, cnt)
                };
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.struct_rest, builder.func);
                let call = builder
                    .ins()
                    .call(func_ref, &[st, sp, exclude_ptr, count_val, vm]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::CarOrNil { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.car_or_nil, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::CdrOrNil { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.cdr_or_nil, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::ArrayMutRefOrNil { dst, src, index } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let idx_val = builder.ins().iconst(I64, *index as i64);
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.array_ref_or_nil, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, idx_val]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::Eval { .. } => {
                return Err(JitError::UnsupportedInstruction("Eval".to_string()));
            }

            LirInstr::SuspendingCall { .. } => {
                return Err(JitError::UnsupportedInstruction(
                    "SuspendingCall".to_string(),
                ));
            }

            LirInstr::ArrayMutExtend { dst, array, source } => {
                let (at, ap) = self.use_var_pair(builder, array.0);
                let (srt, srp) = self.use_var_pair(builder, source.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("ArrayMutExtend without vm pointer".to_string())
                })?;
                let (rt, rp) = self.call_helper_value_binary_vm(
                    builder,
                    self.helpers.array_extend,
                    at,
                    ap,
                    srt,
                    srp,
                    vm,
                )?;
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::ArrayMutPush { dst, array, value } => {
                let (at, ap) = self.use_var_pair(builder, array.0);
                let (vt, vp) = self.use_var_pair(builder, value.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("ArrayMutPush without vm pointer".to_string())
                })?;
                let (rt, rp) = self.call_helper_value_binary_vm(
                    builder,
                    self.helpers.array_push,
                    at,
                    ap,
                    vt,
                    vp,
                    vm,
                )?;
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::CallArrayMut { dst, func, args } => {
                let (ft, fp) = self.use_var_pair(builder, func.0);
                let (art, arp) = self.use_var_pair(builder, args.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("CallArrayMut without vm pointer".to_string())
                })?;
                // call_array: (func_tag, func_payload, arr_tag, arr_payload, vm)
                let (rt, rp) = self.call_helper_value_binary_vm(
                    builder,
                    self.helpers.call_array,
                    ft,
                    fp,
                    art,
                    arp,
                    vm,
                )?;
                self.def_var_pair(builder, dst.0, rt, rp);
                self.emit_exception_check_after_call(builder)?;
                if self.lir.signal.may_suspend() {
                    let idx = self.call_site_index;
                    self.call_site_index += 1;
                    self.emit_yield_check_after_call(builder, idx)?;
                }
            }

            LirInstr::TailCallArrayMut { func, args } => {
                let (ft, fp) = self.use_var_pair(builder, func.0);
                let (art, arp) = self.use_var_pair(builder, args.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("TailCallArrayMut without vm pointer".to_string())
                })?;
                let (rt, rp) = self.call_helper_value_binary_vm(
                    builder,
                    self.helpers.tail_call_array,
                    ft,
                    fp,
                    art,
                    arp,
                    vm,
                )?;
                builder.ins().return_(&[rt, rp]);
                return Ok(true);
            }

            LirInstr::RegionEnter => {
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.region_enter, builder.func);
                let call = builder.ins().call(func_ref, &[]);
                let _ = builder.inst_results(call);
            }
            LirInstr::RegionExit => {
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.region_exit, builder.func);
                let call = builder.ins().call(func_ref, &[]);
                let _ = builder.inst_results(call);
            }

            LirInstr::PushParamFrame { pairs } => {
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("PushParamFrame without vm pointer".to_string())
                })?;
                let count = pairs.len();
                if count == 0 {
                    let null_ptr = builder.ins().iconst(I64, 0);
                    let count_val = builder.ins().iconst(I64, 0);
                    let func_ref = self
                        .module
                        .declare_func_in_func(self.helpers.push_param_frame, builder.func);
                    let call = builder.ins().call(func_ref, &[null_ptr, count_val, vm]);
                    let _ = builder.inst_results(call);
                } else {
                    // Spill pairs as Values (16 bytes each): [param0, val0, param1, val1, ...]
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (count * 2 * 16) as u32,
                            0,
                        ));
                    for (i, (param_reg, val_reg)) in pairs.iter().enumerate() {
                        let (pt, pp) = self.use_var_pair(builder, param_reg.0);
                        let (vt, vp) = self.use_var_pair(builder, val_reg.0);
                        let base = i * 2 * 16;
                        builder.ins().stack_store(pt, slot, base as i32);
                        builder.ins().stack_store(pp, slot, (base + 8) as i32);
                        builder.ins().stack_store(vt, slot, (base + 16) as i32);
                        builder.ins().stack_store(vp, slot, (base + 24) as i32);
                    }
                    let pairs_ptr = builder.ins().stack_addr(I64, slot, 0);
                    let count_val = builder.ins().iconst(I64, count as i64);
                    let func_ref = self
                        .module
                        .declare_func_in_func(self.helpers.push_param_frame, builder.func);
                    let call = builder.ins().call(func_ref, &[pairs_ptr, count_val, vm]);
                    let _ = builder.inst_results(call);
                }
                self.emit_exception_check_after_call(builder)?;
            }

            LirInstr::PopParamFrame => {
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("PopParamFrame without vm pointer".to_string())
                })?;
                self.call_helper_vm_only(builder, self.helpers.pop_param_frame, vm)?;
            }

            LirInstr::IsSet { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_set, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::IsSetMut { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_set_mut, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::CheckSignalBound { src, allowed_bits } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let allowed_val = builder.ins().iconst(I64, allowed_bits.raw() as i64);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("CheckSignalBound without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.check_signal_bound, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, allowed_val, vm]);
                let _ = builder.inst_results(call);
                self.emit_exception_check_after_call(builder)?;
            }
        }
        Ok(false)
    }

    /// Translate a terminator
    pub(crate) fn translate_terminator(
        &mut self,
        builder: &mut FunctionBuilder,
        term: &Terminator,
        block_map: &HashMap<Label, cranelift_codegen::ir::Block>,
    ) -> Result<(), JitError> {
        match term {
            Terminator::Return(reg) => {
                let (tag, payload) = self.use_var_pair(builder, reg.0);
                builder.ins().return_(&[tag, payload]);
            }

            Terminator::Jump(label) => {
                let target = block_map.get(label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown jump target: {:?}", label))
                })?;
                builder.ins().jump(*target, &[]);
            }

            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                let (cond_tag, _) = self.use_var_pair(builder, cond.0);
                let then_block = block_map.get(then_label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown then target: {:?}", then_label))
                })?;
                let else_block = block_map.get(else_label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown else target: {:?}", else_label))
                })?;

                // Truthiness: tag != TAG_NIL (2) AND tag != TAG_FALSE (4)
                // Equivalently: is_truthy if tag != NIL and tag != FALSE.
                // Simple check: tag == TAG_FALSE || tag == TAG_NIL → falsy
                let tag_nil = builder.ins().iconst(I64, TAG_NIL as i64);
                let tag_false = builder.ins().iconst(I64, TAG_FALSE as i64);
                let is_nil = builder.ins().icmp(IntCC::Equal, cond_tag, tag_nil);
                let is_false = builder.ins().icmp(IntCC::Equal, cond_tag, tag_false);
                let is_falsy = builder.ins().bor(is_nil, is_false);
                // brif on is_falsy goes to else, otherwise then
                builder
                    .ins()
                    .brif(is_falsy, *else_block, &[], *then_block, &[]);
            }

            Terminator::Yield {
                value,
                resume_label: _,
            } => {
                let (yt, yp) = self.use_var_pair(builder, value.0);
                let vm = self
                    .vm_ptr
                    .ok_or_else(|| JitError::InvalidLir("Yield without vm pointer".to_string()))?;
                let (self_tag, self_payload) = self.self_tag_payload.ok_or_else(|| {
                    JitError::InvalidLir("Yield without self_tag_payload".to_string())
                })?;

                let yield_index = self.yield_point_index;
                self.yield_point_index += 1;

                let stack_regs = self
                    .lir
                    .yield_points
                    .get(yield_index as usize)
                    .map(|yp| yp.stack_regs.as_slice())
                    .unwrap_or(&[]);

                let spilled_ptr = self.spill_locals_and_operands(builder, stack_regs)?;
                let yield_idx_val = builder.ins().iconst(I64, yield_index as i64);

                // Call elle_jit_yield(ytag, ypay, spilled_ptr, yield_index, vm, ctag, cpay)
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.jit_yield, builder.func);
                let call = builder.ins().call(
                    func_ref,
                    &[
                        yt,
                        yp,
                        spilled_ptr,
                        yield_idx_val,
                        vm,
                        self_tag,
                        self_payload,
                    ],
                );
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                builder.ins().return_(&[rt, rp]);
            }

            Terminator::Unreachable => {
                builder
                    .ins()
                    .trap(cranelift_codegen::ir::TrapCode::unwrap_user(0));
            }
        }
        Ok(())
    }

    /// Allocate the shared spill slot sized to the maximum spill requirement.
    pub(crate) fn allocate_shared_spill_slot(&mut self, builder: &mut FunctionBuilder) {
        let num_locals = self.lir.num_locals as usize;

        let max_yield_operands = self
            .lir
            .yield_points
            .iter()
            .map(|yp| yp.stack_regs.len())
            .max()
            .unwrap_or(0);
        let max_call_operands = self
            .lir
            .call_sites
            .iter()
            .map(|cs| cs.stack_regs.len())
            .max()
            .unwrap_or(0);
        let max_operands = std::cmp::max(max_yield_operands, max_call_operands);
        // Spill saves: arity params (arg_vars) + num_locals locals (local_var_base)
        // + operand stack entries.
        let arity = self.lir.num_params;
        let max_total = arity + num_locals + max_operands;

        if max_total > 0 {
            // Each Value is 16 bytes
            let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                (max_total * 16) as u32,
                0,
            ));
            self.shared_spill_slot = Some(slot);
        }
    }

    /// Spill local variables and operand stack registers to the shared stack slot.
    ///
    /// Returns a Cranelift value pointing to the spilled buffer (*const Value),
    /// or a null pointer constant if there's nothing to spill.
    pub(crate) fn spill_locals_and_operands(
        &mut self,
        builder: &mut FunctionBuilder,
        stack_regs: &[Reg],
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let arity = self.lir.num_params as u16;
        let num_locals = self.lir.num_locals;
        // Spill params (from arg vars) + all local vars (from local_var_base).
        // num_locals includes non-LBox param copies + let-bound locals.
        let num_locally_defined = num_locals;
        let total = arity as usize + num_locally_defined as usize + stack_regs.len();

        if total == 0 {
            return Ok(builder.ins().iconst(I64, 0)); // null pointer
        }

        let slot = self
            .shared_spill_slot
            .expect("JIT bug: spill_locals_and_operands called but no shared spill slot allocated");

        let mut slot_idx: i32 = 0;

        // 1. Spill parameters (from arg variables)
        for i in 0..arity as u32 {
            let base = self.arg_var_base + i;
            let (tag, payload) = self.use_var_pair(builder, base);
            let tag_offset = slot_idx * 16;
            let payload_offset = slot_idx * 16 + 8;
            builder.ins().stack_store(tag, slot, tag_offset);
            builder.ins().stack_store(payload, slot, payload_offset);
            slot_idx += 1;
        }

        // 2. Spill locally-defined variables
        for i in 0..num_locally_defined as u32 {
            let base = self.local_var_base + i;
            let (tag, payload) = self.use_var_pair(builder, base);
            let tag_offset = slot_idx * 16;
            let payload_offset = slot_idx * 16 + 8;
            builder.ins().stack_store(tag, slot, tag_offset);
            builder.ins().stack_store(payload, slot, payload_offset);
            slot_idx += 1;
        }

        // 3. Spill operand stack registers
        for reg in stack_regs {
            let (tag, payload) = self.use_var_pair(builder, reg.0);
            let tag_offset = slot_idx * 16;
            let payload_offset = slot_idx * 16 + 8;
            builder.ins().stack_store(tag, slot, tag_offset);
            builder.ins().stack_store(payload, slot, payload_offset);
            slot_idx += 1;
        }

        Ok(builder.ins().stack_addr(I64, slot, 0))
    }

    /// Helper: emit a tail call with args spilled to stack.
    fn emit_tail_call_with_args(
        &mut self,
        builder: &mut FunctionBuilder,
        ft: cranelift_codegen::ir::Value,
        fp: cranelift_codegen::ir::Value,
        args: &[Reg],
        vm: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
        if args.is_empty() {
            let null_ptr = builder.ins().iconst(I64, 0);
            let nargs = builder.ins().iconst(I64, 0);
            self.call_helper_tail_call(builder, ft, fp, null_ptr, nargs, vm)
        } else {
            let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                (args.len() * 16) as u32,
                0,
            ));
            for (i, arg_reg) in args.iter().enumerate() {
                let (at, ap) = self.use_var_pair(builder, arg_reg.0);
                let tag_offset = (i * 16) as i32;
                let payload_offset = (i * 16 + 8) as i32;
                builder.ins().stack_store(at, slot, tag_offset);
                builder.ins().stack_store(ap, slot, payload_offset);
            }
            let args_addr = builder.ins().stack_addr(I64, slot, 0);
            let nargs = builder.ins().iconst(I64, args.len() as i64);
            self.call_helper_tail_call(builder, ft, fp, args_addr, nargs, vm)
        }
    }
}
