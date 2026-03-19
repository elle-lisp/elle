//! JIT compiler: LirFunction -> Cranelift IR -> Native code
//!
//! This module translates LIR (Low-level Intermediate Representation) to
//! Cranelift IR, then compiles to native x86_64 code.

use std::collections::HashMap;
use std::sync::Arc;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::{AbiParam, Function, InstBuilder, MemFlags, Signature, UserFuncName};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

use crate::lir::{Label, LirFunction};
use crate::value::{Arity, SymbolId};

use super::code::JitCode;
use super::translate::FunctionTranslator;
use super::vtable::{self, RuntimeHelpers};
use super::JitError;

/// Helper to create a Variable from a register/slot index
#[inline]
fn var(n: u32) -> Variable {
    Variable::from_u32(n)
}

/// A member of a compilation group (SCC) for batch JIT compilation.
pub struct BatchMember<'a> {
    /// Symbol ID for this function (used to identify it for direct calls)
    pub sym: SymbolId,
    /// The LIR function to compile
    pub lir: &'a LirFunction,
}

/// JIT compiler that translates LirFunction to native code
pub struct JitCompiler {
    module: JITModule,
    /// Runtime helper function IDs
    helpers: RuntimeHelpers,
}

impl JitCompiler {
    /// Create a new JIT compiler
    pub fn new() -> Result<Self, JitError> {
        // Configure Cranelift for the host target
        let mut flag_builder = settings::builder();
        flag_builder
            .set("use_colocated_libcalls", "false")
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;
        flag_builder
            .set("is_pic", "false")
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;
        flag_builder
            .set("opt_level", "speed")
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        let isa_builder =
            cranelift_native::builder().map_err(|e| JitError::CompilationFailed(e.to_string()))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        // Create JIT module with runtime symbols
        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

        // Register all elle_jit_* symbols with the JIT linker
        vtable::register_symbols(&mut builder);

        let mut module = JITModule::new(builder);

        // Declare runtime helper functions
        let helpers = vtable::declare_helpers(&mut module)?;

        Ok(JitCompiler { module, helpers })
    }

    /// Build the standard JIT function signature.
    /// fn(env: *const Value, args: *const Value, nargs: u32, vm: *mut VM,
    ///    self_tag: u64, self_payload: u64) -> JitValue  (two I64s in rax:rdx)
    fn make_jit_signature(&self) -> Signature {
        let mut sig = self.module.make_signature();
        sig.call_conv = CallConv::SystemV;
        sig.params.push(AbiParam::new(I64)); // env pointer (*const Value)
        sig.params.push(AbiParam::new(I64)); // args pointer (*const Value)
        sig.params.push(AbiParam::new(I64)); // nargs
        sig.params.push(AbiParam::new(I64)); // vm pointer
        sig.params.push(AbiParam::new(I64)); // self_tag
        sig.params.push(AbiParam::new(I64)); // self_payload
        sig.returns.push(AbiParam::new(I64)); // result tag
        sig.returns.push(AbiParam::new(I64)); // result payload
        sig
    }

    /// Compile a LirFunction to native code
    pub fn compile(
        mut self,
        lir: &LirFunction,
        self_sym: Option<SymbolId>,
    ) -> Result<JitCode, JitError> {
        // Only reject polymorphic signals (signal depends on arguments).
        // Yielding functions are now supported via side-exit.
        if lir.signal.propagates != 0 {
            return Err(JitError::Polymorphic);
        }

        // Variadic functions with struct/named varargs require fiber access
        // for error reporting on invalid keyword arguments. The JIT entry
        // block has no fiber pointer, so these fall back to the interpreter.
        // VarargKind::List variadics are fully supported (cons loop in entry block).
        if matches!(lir.arity, Arity::AtLeast(_))
            && !matches!(lir.vararg_kind, crate::hir::VarargKind::List)
        {
            return Err(JitError::UnsupportedInstruction(
                "variadic function with struct/named varargs".to_string(),
            ));
        }

        // Create function signature
        let sig = self.make_jit_signature();

        // Declare the function
        let func_name = lir.name.as_deref().unwrap_or("jit_func");
        let func_id = self
            .module
            .declare_function(func_name, Linkage::Local, &sig)
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        // Build a one-entry scc_peers map for direct self-calls
        let scc_peers = self_sym.map(|sym| {
            let mut map = HashMap::new();
            map.insert(sym, func_id);
            map
        });

        // Create function context
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(0, func_id.as_u32());

        // Translate LIR to Cranelift IR
        let closure_constants =
            self.translate_function(lir, &mut ctx.func, scc_peers.as_ref(), self_sym)?;

        // Compile the function
        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        // Finalize and get the function pointer
        self.module
            .finalize_definitions()
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;
        let fn_ptr = self.module.get_finalized_function(func_id);

        // Convert yield point metadata from LIR to JIT format
        let yield_metas: Vec<super::dispatch::YieldPointMeta> = lir
            .yield_points
            .iter()
            .map(|yp| super::dispatch::YieldPointMeta {
                resume_ip: yp.resume_ip,
                num_spilled: yp.stack_regs.len() as u16,
                num_locals: yp.num_locals,
            })
            .collect();

        // Convert call site metadata from LIR to JIT format
        let call_site_metas: Vec<super::dispatch::CallSiteMeta> = lir
            .call_sites
            .iter()
            .map(|cs| super::dispatch::CallSiteMeta {
                resume_ip: cs.resume_ip,
                num_spilled: cs.num_locals + cs.stack_regs.len() as u16,
                num_locals: cs.num_locals,
            })
            .collect();

        // Wrap in JitCode (module is moved to keep code alive)
        Ok(JitCode::new_with_metadata(
            fn_ptr,
            self.module,
            yield_metas,
            call_site_metas,
            closure_constants,
        ))
    }

    /// Build Cranelift IR for a LirFunction and return it as lines of text.
    /// Does NOT compile to native code — this is for diagnostic display only.
    pub fn clif_text(
        mut self,
        lir: &LirFunction,
        self_sym: Option<SymbolId>,
    ) -> Result<Vec<String>, JitError> {
        if lir.signal.propagates != 0 {
            return Err(JitError::Polymorphic);
        }

        let sig = self.make_jit_signature();

        let func_name = lir.name.as_deref().unwrap_or("jit_func");
        let func_id = self
            .module
            .declare_function(func_name, Linkage::Local, &sig)
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        // Build a one-entry scc_peers map for direct self-calls
        let scc_peers = self_sym.map(|sym| {
            let mut map = HashMap::new();
            map.insert(sym, func_id);
            map
        });

        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(0, func_id.as_u32());

        self.translate_function(lir, &mut ctx.func, scc_peers.as_ref(), self_sym)?;
        // closure_constants from clif_text are discarded — diagnostic only

        let text = format!("{}", ctx.func);
        Ok(text.lines().map(String::from).collect())
    }

    /// Compile multiple mutually recursive functions into a single Cranelift module.
    ///
    /// Functions within the group call each other via direct Cranelift `call`
    /// instructions, eliminating the `elle_jit_call` dispatch overhead.
    /// External calls (to functions outside the group) still use `elle_jit_call`.
    pub fn compile_batch(
        mut self,
        members: &[BatchMember],
    ) -> Result<Vec<(SymbolId, JitCode)>, JitError> {
        // Validate all members are non-polymorphic and non-yielding.
        // Yielding functions require per-function YieldPointMeta in JitCode,
        // but compile_batch creates shared JitCode with empty yield_points.
        // If a yielding function were batch-compiled, elle_jit_yield would
        // panic on index-out-of-bounds when looking up yield point metadata.
        for member in members {
            if member.lir.signal.propagates != 0 {
                return Err(JitError::Polymorphic);
            }
            if member.lir.signal.may_suspend() {
                return Err(JitError::Yielding);
            }
            if matches!(member.lir.arity, Arity::AtLeast(_))
                && !matches!(member.lir.vararg_kind, crate::hir::VarargKind::List)
            {
                return Err(JitError::UnsupportedInstruction(
                    "variadic function with struct/named varargs".to_string(),
                ));
            }
        }

        let sig = self.make_jit_signature();

        // Declare all functions upfront so they can reference each other
        let mut func_ids: Vec<(SymbolId, FuncId)> = Vec::with_capacity(members.len());
        let mut scc_peers: HashMap<SymbolId, FuncId> = HashMap::new();

        for (i, member) in members.iter().enumerate() {
            let name = member
                .lir
                .name
                .as_deref()
                .map(|n| format!("scc_{}_{}", i, n))
                .unwrap_or_else(|| format!("scc_{}", i));
            let func_id = self
                .module
                .declare_function(&name, Linkage::Local, &sig)
                .map_err(|e| JitError::CompilationFailed(e.to_string()))?;
            func_ids.push((member.sym, func_id));
            scc_peers.insert(member.sym, func_id);
        }

        // Define each function with the SCC peer map
        for (i, member) in members.iter().enumerate() {
            let (_, func_id) = func_ids[i];
            let mut ctx = self.module.make_context();
            ctx.func.signature = sig.clone();
            ctx.func.name = UserFuncName::user(0, func_id.as_u32());

            // closure_constants from batch compile are dropped — batch JitCodes use
            // new_shared which has no closure_constants field to populate from here.
            // Closures containing inner lambdas are capture-free, so MakeClosure in
            // SCC peers is rare; if it occurs the inner template Value leaks its Rc
            // until the JIT module is freed.
            let _closure_constants = self.translate_function(
                member.lir,
                &mut ctx.func,
                Some(&scc_peers),
                Some(member.sym),
            )?;

            self.module
                .define_function(func_id, &mut ctx)
                .map_err(|e| JitError::CompilationFailed(e.to_string()))?;
        }

        // Finalize all functions at once
        self.module
            .finalize_definitions()
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        // Collect fn_ptrs before moving module into Arc
        let fn_ptrs: Vec<(SymbolId, *const u8)> = func_ids
            .iter()
            .map(|(sym, fid)| (*sym, self.module.get_finalized_function(*fid)))
            .collect();

        // Wrap module in shared Arc so all JitCode entries keep it alive
        let shared_module = Arc::new(super::code::ModuleHolder::new(self.module));

        // Build results — all share the same module
        let results = fn_ptrs
            .into_iter()
            .map(|(sym, ptr)| (sym, JitCode::new_shared(ptr, shared_module.clone())))
            .collect();

        Ok(results)
    }

    /// Translate LIR function to Cranelift IR
    ///
    /// Each LIR register maps to TWO Cranelift variables: (tag, payload).
    /// The entry block extracts 6 parameters:
    ///   env_ptr, args_ptr, nargs, vm_ptr, self_tag, self_payload
    /// and loads arg Values (16 bytes each) into the doubled arg variables.
    fn translate_function(
        &mut self,
        lir: &LirFunction,
        func: &mut Function,
        scc_peers: Option<&HashMap<SymbolId, FuncId>>,
        self_sym: Option<SymbolId>,
    ) -> Result<Vec<crate::value::Value>, JitError> {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(func, &mut builder_ctx);

        // Create translator context
        let mut translator = FunctionTranslator::new(&mut self.module, &self.helpers, lir);

        translator.self_sym = self_sym;

        if let Some(peers) = scc_peers {
            translator.scc_peers = peers.clone();
        }

        // Variable layout: each LIR register index `r` maps to TWO Cranelift variables:
        //   tag     at Cranelift var index 2*r
        //   payload at Cranelift var index 2*r+1
        //
        // The "logical" variable space covers:
        //   [0,       num_regs)             - LIR work registers
        //   [num_regs, num_regs+num_locals)  - locals (args + locally-defined)
        // The max logical index is max(num_regs, local_var_base + num_locally_defined).
        // Each logical slot needs 2 Cranelift variables.
        let arg_var_base = lir.num_regs;
        let is_list_variadic = matches!(lir.arity, Arity::AtLeast(_))
            && matches!(lir.vararg_kind, crate::hir::VarargKind::List);
        let arity_params = if is_list_variadic {
            lir.num_params as u16
        } else {
            lir.arity.fixed_params() as u16
        };
        let num_locally_defined = lir.num_locals.saturating_sub(arity_params) as u32;
        let local_var_base = arg_var_base + arity_params as u32;
        let max_logical = std::cmp::max(
            std::cmp::max(lir.num_regs, lir.num_locals as u32),
            local_var_base + num_locally_defined,
        );
        // Declare 2 * max_logical Cranelift variables (tag + payload per slot)
        for i in 0..(2 * max_logical) {
            builder.declare_var(var(i), I64);
        }
        translator.arg_var_base = arg_var_base;
        translator.local_var_base = local_var_base;

        // Create blocks
        let entry_block = builder.create_block();
        let loop_header = builder.create_block();

        let mut block_map: HashMap<Label, cranelift_codegen::ir::Block> = HashMap::new();
        for bb in &lir.blocks {
            let cl_block = builder.create_block();
            block_map.insert(bb.label, cl_block);
        }

        // Entry block: extract 6 function parameters
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let env_ptr = builder.block_params(entry_block)[0];
        let args_ptr = builder.block_params(entry_block)[1];
        let nargs = builder.block_params(entry_block)[2];
        let vm_ptr = builder.block_params(entry_block)[3];
        let self_tag = builder.block_params(entry_block)[4];
        let self_payload = builder.block_params(entry_block)[5];

        translator.env_ptr = Some(env_ptr);
        translator.vm_ptr = Some(vm_ptr);
        translator.self_tag_payload = Some((self_tag, self_payload));

        if is_list_variadic {
            // --- Variadic entry: load fixed params, then build cons list for rest ---
            let fixed = lir.arity.fixed_params();

            // Load fixed params from args pointer (16 bytes per Value)
            for i in 0..fixed as u32 {
                let tag_offset = (i as i32) * 16;
                let payload_offset = (i as i32) * 16 + 8;
                let arg_tag = builder
                    .ins()
                    .load(I64, MemFlags::trusted(), args_ptr, tag_offset);
                let arg_payload =
                    builder
                        .ins()
                        .load(I64, MemFlags::trusted(), args_ptr, payload_offset);
                let base = arg_var_base + i;
                translator.def_var_pair(&mut builder, base, arg_tag, arg_payload);
            }

            // Build cons list from remaining args (reverse iteration)
            let rest_var_idx = arg_var_base + fixed as u32;

            let empty_tag = builder
                .ins()
                .iconst(I64, crate::value::Value::EMPTY_LIST.tag as i64);
            let empty_pay = builder.ins().iconst(I64, 0);
            let one = builder.ins().iconst(I64, 1);
            let initial_i = builder.ins().isub(nargs, one);
            let fixed_val = builder.ins().iconst(I64, fixed as i64);

            // Block structure (accumulator carries tag+payload as phi params):
            // loop_head(i, acc_tag, acc_payload): ...
            let cons_loop_head = builder.create_block();
            let cons_loop_body = builder.create_block();
            let cons_loop_exit = builder.create_block();

            builder
                .ins()
                .jump(cons_loop_head, &[initial_i, empty_tag, empty_pay]);

            // loop_head(i, acc_tag, acc_payload)
            builder.switch_to_block(cons_loop_head);
            builder.append_block_param(cons_loop_head, I64); // i
            builder.append_block_param(cons_loop_head, I64); // acc_tag
            builder.append_block_param(cons_loop_head, I64); // acc_payload
            let i_param = builder.block_params(cons_loop_head)[0];
            let acc_tag_param = builder.block_params(cons_loop_head)[1];
            let acc_pay_param = builder.block_params(cons_loop_head)[2];
            let cmp = builder
                .ins()
                .icmp(IntCC::SignedGreaterThanOrEqual, i_param, fixed_val);
            builder.ins().brif(
                cmp,
                cons_loop_body,
                &[i_param, acc_tag_param, acc_pay_param],
                cons_loop_exit,
                &[acc_tag_param, acc_pay_param],
            );

            // loop_body(i, acc_tag, acc_payload)
            builder.switch_to_block(cons_loop_body);
            builder.append_block_param(cons_loop_body, I64); // i
            builder.append_block_param(cons_loop_body, I64); // acc_tag
            builder.append_block_param(cons_loop_body, I64); // acc_payload
            builder.seal_block(cons_loop_body);
            let i_body = builder.block_params(cons_loop_body)[0];
            let acc_tag_body = builder.block_params(cons_loop_body)[1];
            let acc_pay_body = builder.block_params(cons_loop_body)[2];
            // Load args[i] at byte offset i*16 (tag) and i*16+8 (payload)
            let byte_offset = builder.ins().imul_imm(i_body, 16);
            let tag_addr = builder.ins().iadd(args_ptr, byte_offset);
            let arg_tag = builder.ins().load(I64, MemFlags::trusted(), tag_addr, 0);
            let arg_payload = builder.ins().load(I64, MemFlags::trusted(), tag_addr, 8);
            // cons(args[i], acc) -> new_acc
            let cons_ref = translator
                .module
                .declare_func_in_func(translator.helpers.cons, builder.func);
            let call_inst = builder.ins().call(
                cons_ref,
                &[arg_tag, arg_payload, acc_tag_body, acc_pay_body],
            );
            let new_acc_tag = builder.inst_results(call_inst)[0];
            let new_acc_pay = builder.inst_results(call_inst)[1];
            let new_i = builder.ins().isub(i_body, one);
            builder
                .ins()
                .jump(cons_loop_head, &[new_i, new_acc_tag, new_acc_pay]);

            // loop_exit(acc_tag, acc_payload)
            builder.switch_to_block(cons_loop_exit);
            builder.append_block_param(cons_loop_exit, I64); // acc_tag
            builder.append_block_param(cons_loop_exit, I64); // acc_payload
            builder.seal_block(cons_loop_exit);
            let rest_tag = builder.block_params(cons_loop_exit)[0];
            let rest_payload = builder.block_params(cons_loop_exit)[1];

            // Handle lbox_params_mask for the rest param
            let rest_param_index = fixed;
            if rest_param_index < 64 && (lir.lbox_params_mask & (1 << rest_param_index)) != 0 {
                let (cell_t, cell_p) = translator.call_helper_value_unary(
                    &mut builder,
                    translator.helpers.make_lbox,
                    rest_tag,
                    rest_payload,
                )?;
                translator.def_var_pair(&mut builder, rest_var_idx, cell_t, cell_p);
            } else {
                translator.def_var_pair(&mut builder, rest_var_idx, rest_tag, rest_payload);
            }

            // NOTE: cons_loop_head is NOT sealed here — sealed by seal_all_blocks() below.
        } else {
            // --- Non-variadic entry: load all args directly (16 bytes each) ---
            for i in 0..arity_params as u32 {
                let tag_offset = (i as i32) * 16;
                let payload_offset = (i as i32) * 16 + 8;
                let arg_tag = builder
                    .ins()
                    .load(I64, MemFlags::trusted(), args_ptr, tag_offset);
                let arg_payload =
                    builder
                        .ins()
                        .load(I64, MemFlags::trusted(), args_ptr, payload_offset);
                let base = arg_var_base + i;
                translator.def_var_pair(&mut builder, base, arg_tag, arg_payload);
            }
        }

        // Initialize locally-defined variables
        if num_locally_defined > 0 {
            translator.init_locally_defined_vars(&mut builder, num_locally_defined)?;
        }

        // Allocate shared spill slot for yield/call sites (if any)
        if lir.signal.may_suspend() {
            translator.allocate_shared_spill_slot(&mut builder);
        }

        builder.ins().jump(loop_header, &[]);

        // Loop header: merge point for self-tail-calls
        builder.switch_to_block(loop_header);
        let first_lir_block = block_map[&lir.entry];
        builder.ins().jump(first_lir_block, &[]);

        translator.loop_header = Some(loop_header);

        // Translate LIR blocks
        for bb in &lir.blocks {
            let cl_block = block_map[&bb.label];
            builder.switch_to_block(cl_block);

            let mut block_terminated = false;
            for spanned in &bb.instructions {
                if translator.translate_instr(&mut builder, &spanned.instr, &block_map)? {
                    block_terminated = true;
                    break;
                }
            }

            if !block_terminated {
                translator.translate_terminator(
                    &mut builder,
                    &bb.terminator.terminator,
                    &block_map,
                )?;
            }
        }

        builder.seal_all_blocks();
        builder.finalize();
        Ok(translator.closure_constants)
    }
}

impl Default for JitCompiler {
    fn default() -> Self {
        Self::new().expect("Failed to create JIT compiler")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lir::{
        BasicBlock, BinOp, LirInstr, Reg, SpannedInstr, SpannedTerminator, Terminator,
    };
    use crate::signals::Signal;
    use crate::syntax::Span;
    use crate::value::Arity;

    fn make_simple_lir() -> LirFunction {
        // Create a simple function that returns its first argument
        // fn(x) -> x
        // The LIR uses LoadCapture to access parameters.
        // With num_captures=0, LoadCapture index 0 loads from args[0].
        let mut func = LirFunction::new(Arity::Exact(1));
        func.num_regs = 1;
        func.num_captures = 0;
        func.signal = Signal::silent();

        let mut entry = BasicBlock::new(Label(0));
        // Load argument 0 into register 0
        entry.instructions.push(SpannedInstr::new(
            LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            },
            Span::synthetic(),
        ));
        entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), Span::synthetic());

        func.blocks.push(entry);
        func.entry = Label(0);
        func
    }

    fn make_add_lir() -> LirFunction {
        // Create a function that adds two arguments
        // fn(x, y) -> x + y
        // With num_captures=0, LoadCapture index 0 and 1 load from args[0] and args[1].
        let mut func = LirFunction::new(Arity::Exact(2));
        func.num_regs = 3;
        func.num_captures = 0;
        func.signal = Signal::silent();

        let mut entry = BasicBlock::new(Label(0));
        // Load arguments into registers
        entry.instructions.push(SpannedInstr::new(
            LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            },
            Span::synthetic(),
        ));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::LoadCapture {
                dst: Reg(1),
                index: 1,
            },
            Span::synthetic(),
        ));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::BinOp {
                dst: Reg(2),
                op: BinOp::Add,
                lhs: Reg(0),
                rhs: Reg(1),
            },
            Span::synthetic(),
        ));
        entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), Span::synthetic());

        func.blocks.push(entry);
        func.entry = Label(0);
        func
    }

    #[test]
    fn test_compile_identity() {
        let lir = make_simple_lir();
        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let code = compiler.compile(&lir, None).expect("Failed to compile");

        // Call the compiled function with self_tag=0, self_payload=0 (no self-tail-call)
        let args = [crate::value::Value::int(42)];
        let value = unsafe {
            code.call(
                std::ptr::null(),
                args.as_ptr(),
                1,
                std::ptr::null_mut(),
                0,
                0,
            )
        }
        .to_value();
        assert_eq!(value.as_int(), Some(42));
    }

    #[test]
    fn test_compile_add() {
        let lir = make_add_lir();
        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let code = compiler.compile(&lir, None).expect("Failed to compile");

        // Call the compiled function with self_tag=0, self_payload=0
        let args = [crate::value::Value::int(10), crate::value::Value::int(32)];
        let value = unsafe {
            code.call(
                std::ptr::null(),
                args.as_ptr(),
                2,
                std::ptr::null_mut(),
                0,
                0,
            )
        }
        .to_value();
        assert_eq!(value.as_int(), Some(42));
    }

    #[test]
    fn test_reject_polymorphic() {
        let mut lir = make_simple_lir();
        lir.signal = Signal::polymorphic(0);

        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let result = compiler.compile(&lir, None);
        assert!(matches!(result, Err(JitError::Polymorphic)));
    }

    #[test]
    fn test_accept_yielding() {
        let mut lir = make_simple_lir();
        lir.signal = Signal::yields();

        let compiler = JitCompiler::new().expect("Failed to create compiler");
        // Should compile (no Yield terminators in this simple LIR)
        let result = compiler.compile(&lir, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_batch_single_function() {
        // A batch with one function should work identically to compile()
        let lir = make_simple_lir();
        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let members = vec![BatchMember {
            sym: SymbolId(0),
            lir: &lir,
        }];
        let results = compiler
            .compile_batch(&members)
            .expect("Failed to compile batch");

        assert_eq!(results.len(), 1);
        let (sym, code) = &results[0];
        assert_eq!(*sym, SymbolId(0));

        let args = [crate::value::Value::int(42)];
        let value = unsafe {
            code.call(
                std::ptr::null(),
                args.as_ptr(),
                1,
                std::ptr::null_mut(),
                0,
                0,
            )
        }
        .to_value();
        assert_eq!(value.as_int(), Some(42));
    }

    #[test]
    fn test_compile_batch_mutual_calls() {
        // Two functions that call each other via ValueConst + Call.
        // f(x) = if x <= 0 then x else g(x - 1)
        // g(x) = f(x)  (just forwards to f)
        //
        // We can't actually CALL these without a VM (the direct SCC calls
        // still need a valid vm pointer for exception checks), but this test
        // verifies that batch compilation with cross-references succeeds.
        use crate::lir::{CmpOp, LirConst};

        let sym_f = SymbolId(100);
        let sym_g = SymbolId(101);

        // Build f: if x <= 0 then x else call g(x - 1)
        let mut f = LirFunction::new(Arity::Exact(1));
        f.name = Some("f".to_string());
        f.num_regs = 8;
        f.num_captures = 0;
        f.signal = Signal::silent();

        // Block 0 (entry): load arg, check condition
        let mut b0 = BasicBlock::new(Label(0));
        b0.instructions.push(SpannedInstr::new(
            LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            },
            Span::synthetic(),
        ));
        b0.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(1),
                value: LirConst::Int(0),
            },
            Span::synthetic(),
        ));
        b0.instructions.push(SpannedInstr::new(
            LirInstr::Compare {
                dst: Reg(2),
                op: CmpOp::Le,
                lhs: Reg(0),
                rhs: Reg(1),
            },
            Span::synthetic(),
        ));
        b0.terminator = SpannedTerminator::new(
            Terminator::Branch {
                cond: Reg(2),
                then_label: Label(1),
                else_label: Label(2),
            },
            Span::synthetic(),
        );

        // Block 1 (base case): return x
        let mut b1 = BasicBlock::new(Label(1));
        b1.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), Span::synthetic());

        // Block 2 (recursive case): call g(x - 1)
        let mut b2 = BasicBlock::new(Label(2));
        b2.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(3),
                value: LirConst::Int(1),
            },
            Span::synthetic(),
        ));
        b2.instructions.push(SpannedInstr::new(
            LirInstr::BinOp {
                dst: Reg(4),
                op: BinOp::Sub,
                lhs: Reg(0),
                rhs: Reg(3),
            },
            Span::synthetic(),
        ));
        b2.instructions.push(SpannedInstr::new(
            LirInstr::ValueConst {
                dst: Reg(5),
                value: crate::value::Value::NIL,
            },
            Span::synthetic(),
        ));
        b2.instructions.push(SpannedInstr::new(
            LirInstr::Call {
                dst: Reg(6),
                func: Reg(5),
                args: vec![Reg(4)],
            },
            Span::synthetic(),
        ));
        b2.terminator = SpannedTerminator::new(Terminator::Return(Reg(6)), Span::synthetic());

        f.blocks = vec![b0, b1, b2];
        f.entry = Label(0);

        // Build g: tail-call f(x)
        let mut g = LirFunction::new(Arity::Exact(1));
        g.name = Some("g".to_string());
        g.num_regs = 4;
        g.num_captures = 0;
        g.signal = Signal::silent();

        let mut gb0 = BasicBlock::new(Label(0));
        gb0.instructions.push(SpannedInstr::new(
            LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            },
            Span::synthetic(),
        ));
        gb0.instructions.push(SpannedInstr::new(
            LirInstr::ValueConst {
                dst: Reg(1),
                value: crate::value::Value::NIL,
            },
            Span::synthetic(),
        ));
        gb0.instructions.push(SpannedInstr::new(
            LirInstr::TailCall {
                func: Reg(1),
                args: vec![Reg(0)],
            },
            Span::synthetic(),
        ));
        gb0.terminator = SpannedTerminator::new(Terminator::Unreachable, Span::synthetic());

        g.blocks = vec![gb0];
        g.entry = Label(0);

        // Compile both together
        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let members = vec![
            BatchMember {
                sym: sym_f,
                lir: &f,
            },
            BatchMember {
                sym: sym_g,
                lir: &g,
            },
        ];
        let results = compiler
            .compile_batch(&members)
            .expect("Failed to compile batch");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, sym_f);
        assert_eq!(results[1].0, sym_g);
    }

    #[test]
    fn test_compile_batch_rejects_polymorphic() {
        let mut lir = make_simple_lir();
        lir.signal = Signal::polymorphic(0);

        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let members = vec![BatchMember {
            sym: SymbolId(0),
            lir: &lir,
        }];
        let result = compiler.compile_batch(&members);
        assert!(matches!(result, Err(JitError::Polymorphic)));
    }

    #[test]
    fn test_compile_yielding_function() {
        use crate::lir::YieldPointInfo;

        let mut func = LirFunction::new(Arity::Exact(0));
        func.num_regs = 2;
        func.num_captures = 0;
        func.signal = Signal::yields();

        let mut b0 = BasicBlock::new(Label(0));
        b0.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(0),
                value: crate::lir::LirConst::Int(42),
            },
            Span::synthetic(),
        ));
        b0.terminator = SpannedTerminator::new(
            Terminator::Yield {
                value: Reg(0),
                resume_label: Label(1),
            },
            Span::synthetic(),
        );

        let mut b1 = BasicBlock::new(Label(1));
        b1.instructions.push(SpannedInstr::new(
            LirInstr::LoadResumeValue { dst: Reg(1) },
            Span::synthetic(),
        ));
        b1.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), Span::synthetic());

        func.blocks = vec![b0, b1];
        func.entry = Label(0);
        func.yield_points = vec![YieldPointInfo {
            resume_ip: 5,
            stack_regs: vec![],
            num_locals: 0,
        }];

        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let result = compiler.compile(&func, None);
        assert!(
            result.is_ok(),
            "Yielding function should compile: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap().yield_points.len(), 1);
    }

    #[test]
    fn test_reject_struct_variadic() {
        let mut lir = make_simple_lir();
        lir.arity = Arity::AtLeast(1);
        lir.vararg_kind = crate::hir::VarargKind::Struct;

        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let result = compiler.compile(&lir, None);
        assert!(
            matches!(result, Err(JitError::UnsupportedInstruction(_))),
            "Struct variadic functions should be rejected: {:?}",
            result,
        );
    }

    #[test]
    fn test_reject_strict_struct_variadic() {
        let mut lir = make_simple_lir();
        lir.arity = Arity::AtLeast(1);
        lir.vararg_kind = crate::hir::VarargKind::StrictStruct(vec!["key".to_string()]);

        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let result = compiler.compile(&lir, None);
        assert!(
            matches!(result, Err(JitError::UnsupportedInstruction(_))),
            "StrictStruct variadic functions should be rejected: {:?}",
            result,
        );
    }

    #[test]
    fn test_compile_list_variadic() {
        // AtLeast(1) + VarargKind::List should now compile successfully.
        // fn(x & rest) -> x  (ignores rest, just returns first arg)
        let mut lir = make_simple_lir();
        lir.arity = Arity::AtLeast(1);
        lir.vararg_kind = crate::hir::VarargKind::List;
        lir.num_params = 2; // x + rest

        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let result = compiler.compile(&lir, None);
        assert!(
            result.is_ok(),
            "List variadic functions should compile: {:?}",
            result.err(),
        );
    }

    #[test]
    fn test_compile_batch_rejects_struct_variadic() {
        let mut lir = make_simple_lir();
        lir.arity = Arity::AtLeast(1);
        lir.vararg_kind = crate::hir::VarargKind::Struct;

        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let members = vec![BatchMember {
            sym: SymbolId(0),
            lir: &lir,
        }];
        let result = compiler.compile_batch(&members);
        assert!(
            matches!(result, Err(JitError::UnsupportedInstruction(_))),
            "Struct variadic functions should be rejected from batch: {:?}",
            result,
        );
    }

    #[test]
    fn test_compile_batch_accepts_list_variadic() {
        let mut lir = make_simple_lir();
        lir.arity = Arity::AtLeast(1);
        lir.vararg_kind = crate::hir::VarargKind::List;
        lir.num_params = 2;

        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let members = vec![BatchMember {
            sym: SymbolId(0),
            lir: &lir,
        }];
        let result = compiler.compile_batch(&members);
        assert!(
            result.is_ok(),
            "List variadic functions should compile in batch: {:?}",
            result.err(),
        );
    }
}
