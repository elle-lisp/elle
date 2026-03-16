//! LIR to Cranelift IR translation
//!
//! This module contains `FunctionTranslator`, which translates individual
//! LIR instructions and terminators to Cranelift IR.

use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::{InstBuilder, MemFlags};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};

use crate::lir::{Label, LirInstr, Reg, Terminator};
use crate::value::repr::TAG_NIL;
use crate::value::SymbolId;

use super::compiler::RuntimeHelpers;
use super::JitError;

/// Helper to create a Variable from a register/slot index
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
    /// NaN-boxed bits of the closure being executed (for self-tail-call detection)
    pub(crate) self_bits: Option<cranelift_codegen::ir::Value>,
    /// Base index for arg variables (= num_regs)
    pub(crate) arg_var_base: u32,
    /// Base index for locally-defined variable Cranelift variables
    /// These are variables for let-bindings inside the function body.
    /// Layout: \[num_captures..., params..., locally_defined...\]
    /// In the JIT, locally_defined vars use Cranelift variables starting at this base.
    pub(crate) local_var_base: u32,
    /// Loop header block for self-tail-call jumps
    pub(crate) loop_header: Option<cranelift_codegen::ir::Block>,
    /// SCC peer functions: maps SymbolId -> Cranelift FuncId for direct calls.
    /// When a Call/TailCall targets a global in this map, we emit a direct
    /// Cranelift call instead of going through elle_jit_call.
    pub(crate) scc_peers: HashMap<SymbolId, FuncId>,
    /// Map from register to the SymbolId it was loaded from.
    /// Used to detect when a Call/TailCall targets an SCC peer.
    pub(crate) global_load_map: HashMap<Reg, SymbolId>,
    /// SymbolId of the function being compiled (for self-call detection)
    pub(crate) self_sym: Option<SymbolId>,
    /// Counter for yield point indices
    pub(crate) yield_point_index: u32,
    /// Counter for call site indices (for yield-through-call metadata)
    pub(crate) call_site_index: u32,
    /// Shared stack slot for spilling locals + operands at yield/call sites.
    /// Sized to the maximum spill requirement across all yield points and
    /// call sites. `None` if the function has no spill points.
    pub(crate) shared_spill_slot: Option<cranelift_codegen::ir::StackSlot>,
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
            self_bits: None,
            arg_var_base: 0,
            local_var_base: 0,
            loop_header: None,
            scc_peers: HashMap::new(),
            global_load_map: HashMap::new(),
            self_sym: None,
            yield_point_index: 0,
            call_site_index: 0,
            shared_spill_slot: None,
        }
    }

    /// Initialize locally-defined variables.
    /// Variables that need cells (captured or mutated) get LocalCell(NIL).
    /// Variables that don't need cells get NIL directly, avoiding heap allocation.
    pub(crate) fn init_locally_defined_vars(
        &mut self,
        builder: &mut FunctionBuilder,
        num_locally_defined: u32,
    ) -> Result<(), JitError> {
        use crate::value::Value;

        let nil_bits = builder.ins().iconst(I64, Value::NIL.to_bits() as i64);
        let lbox_locals_mask = self.lir.lbox_locals_mask;

        for i in 0..num_locally_defined {
            if i < 64 && (lbox_locals_mask & (1 << i)) != 0 {
                // This local needs a cell (captured or mutated)
                let cell = self.call_helper_unary(builder, self.helpers.make_lbox, nil_bits)?;
                builder.def_var(var(self.local_var_base + i), cell);
            } else {
                // No cell needed — store value directly
                builder.def_var(var(self.local_var_base + i), nil_bits);
            }
        }

        Ok(())
    }

    /// Translate a single LIR instruction
    /// Returns true if the instruction emitted a terminator (e.g., TailCall)
    pub(crate) fn translate_instr(
        &mut self,
        builder: &mut FunctionBuilder,
        instr: &LirInstr,
        _block_map: &HashMap<Label, cranelift_codegen::ir::Block>,
    ) -> Result<bool, JitError> {
        match instr {
            LirInstr::Const { dst, value } => {
                let val = self.translate_const(builder, value);
                builder.def_var(var(dst.0), val);
            }

            LirInstr::ValueConst { dst, value } => {
                let bits = value.to_bits();
                let val = builder.ins().iconst(I64, bits as i64);
                builder.def_var(var(dst.0), val);
            }

            LirInstr::LoadLocal { dst, slot } => {
                // In LIR, locals are just registers
                let val = builder.use_var(var(*slot as u32));
                builder.def_var(var(dst.0), val);
            }

            LirInstr::StoreLocal { slot, src } => {
                let val = builder.use_var(var(src.0));
                builder.def_var(var(*slot as u32), val);
            }

            LirInstr::LoadCapture { dst, index } => {
                // The LIR uses indices where:
                // - [0, num_captures) are captures (from env)
                // - [num_captures, num_captures + arity) are parameters (from args)
                // - [num_captures + arity, ...) are locally-defined variables
                let num_captures = self.lir.num_captures;
                let arity = self.lir.num_params as u16;
                if *index < num_captures {
                    // Load from closure environment (captures)
                    // Must auto-unwrap LocalCell if present (matches interpreter's LoadUpvalue)
                    let env_ptr = self.env_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCapture without env pointer".to_string())
                    })?;
                    let offset = (*index as i32) * 8;
                    let addr = builder.ins().iadd_imm(env_ptr, offset as i64);
                    let raw = builder.ins().load(I64, MemFlags::trusted(), addr, 0);
                    let val = self.call_helper_unary(builder, self.helpers.load_capture, raw)?;
                    builder.def_var(var(dst.0), val);
                } else if *index < num_captures + arity {
                    // Load from arg variables (NOT args pointer)
                    // This allows self-tail-calls to update args and loop back
                    let param_index = *index - num_captures;
                    let val = builder.use_var(var(self.arg_var_base + param_index as u32));
                    builder.def_var(var(dst.0), val);
                } else {
                    // Locally-defined variable - use Cranelift variable
                    let local_index = *index - num_captures - arity;
                    let local_val = builder.use_var(var(self.local_var_base + local_index as u32));
                    if (local_index as u32) < 64
                        && (self.lir.lbox_locals_mask & (1 << local_index)) != 0
                    {
                        // cell-wrapped: auto-unwrap via load_lbox
                        let result =
                            self.call_helper_unary(builder, self.helpers.load_lbox, local_val)?;
                        builder.def_var(var(dst.0), result);
                    } else {
                        // Direct value: no cell indirection
                        builder.def_var(var(dst.0), local_val);
                    }
                }
            }

            LirInstr::LoadCaptureRaw { dst, index } => {
                // Same as LoadCapture but doesn't unwrap cells (for forwarding)
                let num_captures = self.lir.num_captures;
                let arity = self.lir.num_params as u16;
                if *index < num_captures {
                    let env_ptr = self.env_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCaptureRaw without env pointer".to_string())
                    })?;
                    let offset = (*index as i32) * 8;
                    let addr = builder.ins().iadd_imm(env_ptr, offset as i64);
                    let val = builder.ins().load(I64, MemFlags::trusted(), addr, 0);
                    builder.def_var(var(dst.0), val);
                } else if *index < num_captures + arity {
                    // Load from arg variables (NOT args pointer)
                    let param_index = *index - num_captures;
                    let val = builder.use_var(var(self.arg_var_base + param_index as u32));
                    builder.def_var(var(dst.0), val);
                } else {
                    // Locally-defined variable - use Cranelift variable directly
                    // LoadCaptureRaw doesn't unwrap cells, so return the cell itself
                    let local_index = *index - num_captures - arity;
                    let val = builder.use_var(var(self.local_var_base + local_index as u32));
                    builder.def_var(var(dst.0), val);
                }
            }

            LirInstr::BinOp { dst, op, lhs, rhs } => {
                let lhs_val = builder.use_var(var(lhs.0));
                let rhs_val = builder.use_var(var(rhs.0));
                let result = self.call_binary_helper(builder, *op, lhs_val, rhs_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::UnaryOp { dst, op, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_unary_helper(builder, *op, src_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::Compare { dst, op, lhs, rhs } => {
                let lhs_val = builder.use_var(var(lhs.0));
                let rhs_val = builder.use_var(var(rhs.0));
                let result = self.call_compare_helper(builder, *op, lhs_val, rhs_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::IsNil { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_helper_unary(builder, self.helpers.is_nil, src_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::IsPair { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_helper_unary(builder, self.helpers.is_pair, src_val)?;
                builder.def_var(var(dst.0), result);
            }

            // === Phase 3: Data structures ===
            LirInstr::Cons { dst, head, tail } => {
                let head_val = builder.use_var(var(head.0));
                let tail_val = builder.use_var(var(tail.0));
                let result =
                    self.call_helper_binary(builder, self.helpers.cons, head_val, tail_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::Car { dst, pair } => {
                let pair_val = builder.use_var(var(pair.0));
                let result = self.call_helper_unary(builder, self.helpers.car, pair_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::Cdr { dst, pair } => {
                let pair_val = builder.use_var(var(pair.0));
                let result = self.call_helper_unary(builder, self.helpers.cdr, pair_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::MakeArrayMut { dst, elements } => {
                // Allocate stack space for elements
                if elements.is_empty() {
                    // Empty array - pass null pointer and 0 count
                    let null_ptr = builder.ins().iconst(I64, 0);
                    let count = builder.ins().iconst(I64, 0);
                    let result =
                        self.call_helper_binary(builder, self.helpers.make_array, null_ptr, count)?;
                    builder.def_var(var(dst.0), result);
                } else {
                    // Create stack slot for elements
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (elements.len() * 8) as u32,
                            0,
                        ));
                    // Store each element
                    for (i, elem_reg) in elements.iter().enumerate() {
                        let elem_val = builder.use_var(var(elem_reg.0));
                        builder.ins().stack_store(elem_val, slot, (i * 8) as i32);
                    }
                    let elements_addr = builder.ins().stack_addr(I64, slot, 0);
                    let count = builder.ins().iconst(I64, elements.len() as i64);
                    let result = self.call_helper_binary(
                        builder,
                        self.helpers.make_array,
                        elements_addr,
                        count,
                    )?;
                    builder.def_var(var(dst.0), result);
                }
            }

            // === Phase 3: Cell operations ===
            LirInstr::MakeLBox { dst, value } => {
                let val = builder.use_var(var(value.0));
                let result = self.call_helper_unary(builder, self.helpers.make_lbox, val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::LoadLBox { dst, cell } => {
                let cell_val = builder.use_var(var(cell.0));
                let result = self.call_helper_unary(builder, self.helpers.load_lbox, cell_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::StoreLBox { cell, value } => {
                let cell_val = builder.use_var(var(cell.0));
                let val = builder.use_var(var(value.0));
                let _result =
                    self.call_helper_binary(builder, self.helpers.store_lbox, cell_val, val)?;
                // Result is NIL, we don't need to store it
            }

            LirInstr::StoreCapture { index, src } => {
                let num_captures = self.lir.num_captures;
                let arity = self.lir.num_params as u16;
                let val = builder.use_var(var(src.0));

                if *index < num_captures + arity {
                    // Captures and parameters: use env pointer
                    let env_ptr = self.env_ptr.ok_or_else(|| {
                        JitError::InvalidLir("StoreCapture without env pointer".to_string())
                    })?;
                    let idx_val = builder.ins().iconst(I64, *index as i64);
                    let _result = self.call_helper_ternary(
                        builder,
                        self.helpers.store_capture,
                        env_ptr,
                        idx_val,
                        val,
                    )?;
                } else {
                    // Locally-defined variable
                    let local_index = *index - num_captures - arity;
                    if (local_index as u32) < 64
                        && (self.lir.lbox_locals_mask & (1 << local_index)) != 0
                    {
                        // cell-wrapped: store into the cell
                        let cell_val =
                            builder.use_var(var(self.local_var_base + local_index as u32));
                        let _result = self.call_helper_binary(
                            builder,
                            self.helpers.store_lbox,
                            cell_val,
                            val,
                        )?;
                    } else {
                        // Direct value: just update the Cranelift variable
                        builder.def_var(var(self.local_var_base + local_index as u32), val);
                    }
                }
            }

            // === Phase 3: Function calls ===
            LirInstr::Call { dst, func, args } => {
                let func_val = builder.use_var(var(func.0));
                let vm = self
                    .vm_ptr
                    .ok_or_else(|| JitError::InvalidLir("Call without vm pointer".to_string()))?;

                // Check if this is a direct call to an SCC peer
                let maybe_scc = self
                    .global_load_map
                    .get(func)
                    .and_then(|&sym| self.scc_peers.get(&sym).map(|&fid| (sym, fid)));
                if let Some((sym, peer_func_id)) = maybe_scc {
                    // Track call depth (direct SCC calls bypass elle_jit_call)
                    let overflow =
                        self.call_helper_unary(builder, self.helpers.call_depth_enter, vm)?;
                    // If overflow (non-zero return), bail out (signal already set)
                    let zero = builder.ins().iconst(I64, 0);
                    let is_overflow = builder.ins().icmp(IntCC::NotEqual, overflow, zero);
                    let overflow_block = builder.create_block();
                    let call_block = builder.create_block();
                    builder
                        .ins()
                        .brif(is_overflow, overflow_block, &[], call_block, &[]);

                    builder.switch_to_block(overflow_block);
                    builder.seal_block(overflow_block);
                    let nil = builder.ins().iconst(I64, TAG_NIL as i64);
                    builder.ins().return_(&[nil]);

                    builder.switch_to_block(call_block);
                    builder.seal_block(call_block);

                    // Direct call to SCC peer — skip elle_jit_call dispatch
                    let result = self.emit_direct_scc_call(builder, peer_func_id, sym, args, vm)?;
                    // Decrement call depth
                    self.call_helper_unary(builder, self.helpers.call_depth_exit, vm)?;
                    // Resolve pending tail call if the peer returned TAIL_CALL_SENTINEL
                    let resolved = self.call_helper_binary(
                        builder,
                        self.helpers.resolve_tail_call,
                        result,
                        vm,
                    )?;
                    builder.def_var(var(dst.0), resolved);
                    self.emit_exception_check_after_call(builder)?;
                    if self.lir.signal.may_suspend() {
                        let idx = self.call_site_index;
                        self.call_site_index += 1;
                        self.emit_yield_check_after_call(builder, idx)?;
                    }
                } else if args.is_empty() {
                    // No args - pass null pointer
                    let null_ptr = builder.ins().iconst(I64, 0);
                    let nargs = builder.ins().iconst(I64, 0);
                    let result = self.call_helper_call(builder, func_val, null_ptr, nargs, vm)?;
                    builder.def_var(var(dst.0), result);
                    // Check for exception after call - if set, bail out to interpreter
                    self.emit_exception_check_after_call(builder)?;
                    if self.lir.signal.may_suspend() {
                        let idx = self.call_site_index;
                        self.call_site_index += 1;
                        self.emit_yield_check_after_call(builder, idx)?;
                    }
                } else {
                    // Create stack slot for args
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (args.len() * 8) as u32,
                            0,
                        ));
                    // Store each arg
                    for (i, arg_reg) in args.iter().enumerate() {
                        let arg_val = builder.use_var(var(arg_reg.0));
                        builder.ins().stack_store(arg_val, slot, (i * 8) as i32);
                    }
                    let args_addr = builder.ins().stack_addr(I64, slot, 0);
                    let nargs = builder.ins().iconst(I64, args.len() as i64);
                    let result = self.call_helper_call(builder, func_val, args_addr, nargs, vm)?;
                    builder.def_var(var(dst.0), result);
                    // Check for exception after call - if set, bail out to interpreter
                    self.emit_exception_check_after_call(builder)?;
                    if self.lir.signal.may_suspend() {
                        let idx = self.call_site_index;
                        self.call_site_index += 1;
                        self.emit_yield_check_after_call(builder, idx)?;
                    }
                }
            }

            LirInstr::TailCall { func, args } => {
                let func_val = builder.use_var(var(func.0));
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("TailCall without vm pointer".to_string())
                })?;

                // Check if this is a self-tail-call (func == self_bits)
                // Only do this optimization if we have self_bits and loop_header
                if let (Some(self_bits), Some(loop_header)) = (self.self_bits, self.loop_header) {
                    // Check arity matches (self-call must have same number of args)
                    if args.len() == self.lir.num_params {
                        let is_self = builder.ins().icmp(IntCC::Equal, func_val, self_bits);

                        let self_call_block = builder.create_block();
                        let other_call_block = builder.create_block();

                        builder
                            .ins()
                            .brif(is_self, self_call_block, &[], other_call_block, &[]);

                        // Self-call path: update arg variables and jump to loop header
                        builder.switch_to_block(self_call_block);
                        builder.seal_block(self_call_block);

                        // Read all new arg values first (before updating any variables)
                        // This handles cases like (f b a) where args are swapped
                        let new_arg_vals: Vec<_> = args
                            .iter()
                            .map(|arg_reg| builder.use_var(var(arg_reg.0)))
                            .collect();

                        // Now update arg variables
                        for (i, arg_val) in new_arg_vals.into_iter().enumerate() {
                            builder.def_var(var(self.arg_var_base + i as u32), arg_val);
                        }

                        builder.ins().jump(loop_header, &[]);

                        // Other-call path: check SCC peers, then fall back to trampoline
                        builder.switch_to_block(other_call_block);
                        builder.seal_block(other_call_block);

                        let maybe_scc2 = self
                            .global_load_map
                            .get(func)
                            .and_then(|&sym| self.scc_peers.get(&sym).map(|&fid| (sym, fid)));
                        if let Some((sym2, peer_func_id)) = maybe_scc2 {
                            // Direct call to SCC peer + return
                            let result =
                                self.emit_direct_scc_call(builder, peer_func_id, sym2, args, vm)?;
                            builder.ins().return_(&[result]);
                        } else {
                            let result = if args.is_empty() {
                                let null_ptr = builder.ins().iconst(I64, 0);
                                let nargs = builder.ins().iconst(I64, 0);
                                self.call_helper_tail_call(builder, func_val, null_ptr, nargs, vm)?
                            } else {
                                let slot = builder.create_sized_stack_slot(
                                    cranelift_codegen::ir::StackSlotData::new(
                                        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                                        (args.len() * 8) as u32,
                                        0,
                                    ),
                                );
                                for (i, arg_reg) in args.iter().enumerate() {
                                    let arg_val = builder.use_var(var(arg_reg.0));
                                    builder.ins().stack_store(arg_val, slot, (i * 8) as i32);
                                }
                                let args_addr = builder.ins().stack_addr(I64, slot, 0);
                                let nargs = builder.ins().iconst(I64, args.len() as i64);
                                self.call_helper_tail_call(builder, func_val, args_addr, nargs, vm)?
                            };
                            builder.ins().return_(&[result]);
                        }
                        return Ok(true); // Block is terminated
                    }
                }

                // Fallback: no self-tail-call optimization (arity mismatch or no self_bits)
                // Check SCC peers before falling back to trampoline
                let maybe_scc3 = self
                    .global_load_map
                    .get(func)
                    .and_then(|&sym| self.scc_peers.get(&sym).map(|&fid| (sym, fid)));
                if let Some((sym3, peer_func_id)) = maybe_scc3 {
                    let result =
                        self.emit_direct_scc_call(builder, peer_func_id, sym3, args, vm)?;
                    builder.ins().return_(&[result]);
                    return Ok(true);
                }

                let result = if args.is_empty() {
                    let null_ptr = builder.ins().iconst(I64, 0);
                    let nargs = builder.ins().iconst(I64, 0);
                    self.call_helper_tail_call(builder, func_val, null_ptr, nargs, vm)?
                } else {
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (args.len() * 8) as u32,
                            0,
                        ));
                    for (i, arg_reg) in args.iter().enumerate() {
                        let arg_val = builder.use_var(var(arg_reg.0));
                        builder.ins().stack_store(arg_val, slot, (i * 8) as i32);
                    }
                    let args_addr = builder.ins().stack_addr(I64, slot, 0);
                    let nargs = builder.ins().iconst(I64, args.len() as i64);
                    self.call_helper_tail_call(builder, func_val, args_addr, nargs, vm)?
                };
                // Return the result (either the direct result for native/vm-aware,
                // or TAIL_CALL_SENTINEL for closures)
                builder.ins().return_(&[result]);
                return Ok(true); // Block is terminated
            }

            // === Still unsupported (Phase 4+) ===
            // NOTE: Keep group.rs::has_unsupported_instructions in sync with this list.
            LirInstr::MakeClosure { .. } => {
                return Err(JitError::UnsupportedInstruction("MakeClosure".to_string()));
            }
            LirInstr::LoadResumeValue { dst } => {
                // Resume goes through the interpreter. This block is
                // unreachable in JIT code. Emit NIL as dead code.
                let nil = builder.ins().iconst(I64, TAG_NIL as i64);
                builder.def_var(var(dst.0), nil);
            }
            LirInstr::CarDestructure { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("CarDestructure without vm pointer".to_string())
                })?;
                let result =
                    self.call_helper_binary(builder, self.helpers.car_destructure, src_val, vm)?;
                self.emit_exception_check_after_call(builder)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::CdrDestructure { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("CdrDestructure without vm pointer".to_string())
                })?;
                let result =
                    self.call_helper_binary(builder, self.helpers.cdr_destructure, src_val, vm)?;
                self.emit_exception_check_after_call(builder)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::ArrayMutRefDestructure { dst, src, index } => {
                let src_val = builder.use_var(var(src.0));
                let idx_val = builder.ins().iconst(I64, *index as i64);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("ArrayMutRefDestructure without vm pointer".to_string())
                })?;
                let result = self.call_helper_ternary(
                    builder,
                    self.helpers.array_ref_destructure,
                    src_val,
                    idx_val,
                    vm,
                )?;
                self.emit_exception_check_after_call(builder)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::ArrayMutSliceFrom { dst, src, index } => {
                let src_val = builder.use_var(var(src.0));
                let idx_val = builder.ins().iconst(I64, *index as i64);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("ArrayMutSliceFrom without vm pointer".to_string())
                })?;
                let result = self.call_helper_ternary(
                    builder,
                    self.helpers.array_slice_from,
                    src_val,
                    idx_val,
                    vm,
                )?;
                self.emit_exception_check_after_call(builder)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::IsArray { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_helper_unary(builder, self.helpers.is_array, src_val)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::IsArrayMut { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_helper_unary(builder, self.helpers.is_array_mut, src_val)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::IsStruct { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_helper_unary(builder, self.helpers.is_struct, src_val)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::IsStructMut { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result =
                    self.call_helper_unary(builder, self.helpers.is_struct_mut, src_val)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::ArrayMutLen { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_helper_unary(builder, self.helpers.array_len, src_val)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::TableGetOrNil { .. } => {
                return Err(JitError::UnsupportedInstruction(
                    "TableGetOrNil".to_string(),
                ));
            }
            LirInstr::TableGetDestructure { .. } => {
                return Err(JitError::UnsupportedInstruction(
                    "TableGetDestructure".to_string(),
                ));
            }
            LirInstr::StructRest { .. } => {
                return Err(JitError::UnsupportedInstruction("StructRest".to_string()));
            }
            LirInstr::CarOrNil { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_helper_unary(builder, self.helpers.car_or_nil, src_val)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::CdrOrNil { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_helper_unary(builder, self.helpers.cdr_or_nil, src_val)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::ArrayMutRefOrNil { dst, src, index } => {
                let src_val = builder.use_var(var(src.0));
                let idx_val = builder.ins().iconst(I64, *index as i64);
                let result = self.call_helper_binary(
                    builder,
                    self.helpers.array_ref_or_nil,
                    src_val,
                    idx_val,
                )?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::Eval { .. } => {
                return Err(JitError::UnsupportedInstruction("Eval".to_string()));
            }
            LirInstr::ArrayMutExtend { dst, array, source } => {
                let array_val = builder.use_var(var(array.0));
                let source_val = builder.use_var(var(source.0));
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("ArrayMutExtend without vm pointer".to_string())
                })?;
                let result = self.call_helper_ternary(
                    builder,
                    self.helpers.array_extend,
                    array_val,
                    source_val,
                    vm,
                )?;
                self.emit_exception_check_after_call(builder)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::ArrayMutPush { dst, array, value } => {
                let array_val = builder.use_var(var(array.0));
                let value_val = builder.use_var(var(value.0));
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("ArrayMutPush without vm pointer".to_string())
                })?;
                let result = self.call_helper_ternary(
                    builder,
                    self.helpers.array_push,
                    array_val,
                    value_val,
                    vm,
                )?;
                self.emit_exception_check_after_call(builder)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::CallArrayMut { .. } => {
                return Err(JitError::UnsupportedInstruction("CallArrayMut".to_string()));
            }
            LirInstr::TailCallArrayMut { .. } => {
                return Err(JitError::UnsupportedInstruction(
                    "TailCallArrayMut".to_string(),
                ));
            }
            LirInstr::RegionEnter | LirInstr::RegionExit => {
                // No-op in JIT (allocation regions not yet active)
            }
            LirInstr::PushParamFrame { pairs } => {
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("PushParamFrame without vm pointer".to_string())
                })?;
                let count = pairs.len();
                if count == 0 {
                    // Empty frame — call with null pointer and count 0
                    let null_ptr = builder.ins().iconst(I64, 0);
                    let count_val = builder.ins().iconst(I64, 0);
                    self.call_helper_ternary(
                        builder,
                        self.helpers.push_param_frame,
                        null_ptr,
                        count_val,
                        vm,
                    )?;
                } else {
                    // Spill pairs to a stack slot: [param0, val0, param1, val1, ...]
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (count * 2 * 8) as u32,
                            0,
                        ));
                    for (i, (param_reg, val_reg)) in pairs.iter().enumerate() {
                        let param_val = builder.use_var(var(param_reg.0));
                        let val_val = builder.use_var(var(val_reg.0));
                        let offset_param = (i * 2 * 8) as i32;
                        let offset_val = (i * 2 * 8 + 8) as i32;
                        builder.ins().stack_store(param_val, slot, offset_param);
                        builder.ins().stack_store(val_val, slot, offset_val);
                    }
                    let pairs_ptr = builder.ins().stack_addr(I64, slot, 0);
                    let count_val = builder.ins().iconst(I64, count as i64);
                    self.call_helper_ternary(
                        builder,
                        self.helpers.push_param_frame,
                        pairs_ptr,
                        count_val,
                        vm,
                    )?;
                }
                self.emit_exception_check_after_call(builder)?;
            }
            LirInstr::PopParamFrame => {
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("PopParamFrame without vm pointer".to_string())
                })?;
                self.call_helper_unary(builder, self.helpers.pop_param_frame, vm)?;
            }
            LirInstr::IsSet { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_helper_unary(builder, self.helpers.is_set, src_val)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::IsSetMut { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_helper_unary(builder, self.helpers.is_set_mut, src_val)?;
                builder.def_var(var(dst.0), result);
            }
            LirInstr::CheckSignalBound { .. } => {
                return Err(JitError::UnsupportedInstruction(
                    "CheckSignalBound".to_string(),
                ));
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
                let val = builder.use_var(var(reg.0));
                builder.ins().return_(&[val]);
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
                let cond_val = builder.use_var(var(cond.0));
                let then_block = block_map.get(then_label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown then target: {:?}", then_label))
                })?;
                let else_block = block_map.get(else_label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown else target: {:?}", else_label))
                })?;

                // Check truthiness: (value >> 48) != 0x7FF9
                let shifted = builder.ins().ushr_imm(cond_val, 48);
                let falsy_tag = builder.ins().iconst(I64, 0x7FF9_i64);
                let is_truthy = builder.ins().icmp(IntCC::NotEqual, shifted, falsy_tag);

                builder
                    .ins()
                    .brif(is_truthy, *then_block, &[], *else_block, &[]);
            }

            Terminator::Yield {
                value,
                resume_label: _,
            } => {
                let yielded_val = builder.use_var(var(value.0));
                let vm = self
                    .vm_ptr
                    .ok_or_else(|| JitError::InvalidLir("Yield without vm pointer".to_string()))?;
                let self_bits = self
                    .self_bits
                    .ok_or_else(|| JitError::InvalidLir("Yield without self_bits".to_string()))?;

                let yield_index = self.yield_point_index;
                self.yield_point_index += 1;

                // Read the stack_regs for this yield point from the LIR
                let stack_regs = self
                    .lir
                    .yield_points
                    .get(yield_index as usize)
                    .map(|yp| yp.stack_regs.as_slice())
                    .unwrap_or(&[]);

                // Spill locals + operand stack to match interpreter layout:
                // [param_0, ..., param_{arity-1}, local_0, ..., local_n, operand_0, ..., operand_m]
                let spilled_ptr = self.spill_locals_and_operands(builder, stack_regs)?;

                let yield_idx_val = builder.ins().iconst(I64, yield_index as i64);

                // Call elle_jit_yield(yielded, spilled_ptr, yield_index, vm, self_bits)
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.jit_yield, builder.func);
                let call = builder.ins().call(
                    func_ref,
                    &[yielded_val, spilled_ptr, yield_idx_val, vm, self_bits],
                );
                let result = builder.inst_results(call)[0];
                builder.ins().return_(&[result]);
            }

            Terminator::Unreachable => {
                builder
                    .ins()
                    .trap(cranelift_codegen::ir::TrapCode::unwrap_user(0));
            }
        }
        Ok(())
    }

    /// Allocate the shared spill slot sized to the maximum spill requirement
    /// across all yield points and call sites. Called once during function setup.
    pub(crate) fn allocate_shared_spill_slot(&mut self, builder: &mut FunctionBuilder) {
        let num_locals = self.lir.num_locals as usize;

        // Compute max operand stack size across all yield points and call sites
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
        let max_total = num_locals + max_operands;

        if max_total > 0 {
            let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                (max_total * 8) as u32,
                0,
            ));
            self.shared_spill_slot = Some(slot);
        }
    }

    /// Spill local variables and operand stack registers to the shared stack slot.
    ///
    /// The interpreter's stack layout at yield/call is:
    /// `[param_0, ..., param_{arity-1}, local_0, ..., local_n, operand_0, ..., operand_m]`
    ///
    /// The JIT stores params in arg variables and locals in local variables
    /// (Cranelift variables, i.e., CPU registers). This method spills them
    /// all into a contiguous buffer matching the interpreter's layout.
    ///
    /// Returns a Cranelift value pointing to the spilled buffer, or a null
    /// pointer constant if there's nothing to spill.
    pub(crate) fn spill_locals_and_operands(
        &mut self,
        builder: &mut FunctionBuilder,
        stack_regs: &[Reg],
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let arity = self.lir.num_params as u16;
        let num_locals = self.lir.num_locals;
        let num_locally_defined = num_locals.saturating_sub(arity);
        let total = num_locals as usize + stack_regs.len();

        if total == 0 {
            return Ok(builder.ins().iconst(I64, 0)); // null pointer
        }

        let slot = self
            .shared_spill_slot
            .expect("JIT bug: spill_locals_and_operands called but no shared spill slot allocated");

        let mut offset: i32 = 0;

        // 1. Spill parameters (from arg variables)
        for i in 0..arity as u32 {
            let val = builder.use_var(var(self.arg_var_base + i));
            builder.ins().stack_store(val, slot, offset * 8);
            offset += 1;
        }

        // 2. Spill locally-defined variables
        for i in 0..num_locally_defined as u32 {
            let val = builder.use_var(var(self.local_var_base + i));
            builder.ins().stack_store(val, slot, offset * 8);
            offset += 1;
        }

        // 3. Spill operand stack registers
        for reg in stack_regs {
            let val = builder.use_var(var(reg.0));
            builder.ins().stack_store(val, slot, offset * 8);
            offset += 1;
        }

        Ok(builder.ins().stack_addr(I64, slot, 0))
    }
}
