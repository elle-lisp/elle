//! JIT compiler: LirFunction -> Cranelift IR -> Native code
//!
//! This module translates LIR (Low-level Intermediate Representation) to
//! Cranelift IR, then compiles to native x86_64 code.

use std::collections::HashMap;
use std::sync::Arc;

use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::{AbiParam, Function, InstBuilder, MemFlags, Signature, UserFuncName};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

use crate::lir::{Label, LirFunction, LirInstr};
use crate::value::SymbolId;

use super::code::JitCode;
use super::dispatch;
use super::runtime;
use super::translate::FunctionTranslator;
use super::JitError;

/// Helper to create a Variable from a register/slot index
#[inline]
fn var(n: u32) -> Variable {
    Variable::from_u32(n)
}

/// A member of a compilation group (SCC) for batch JIT compilation.
pub struct BatchMember<'a> {
    /// Symbol ID for this function (used to identify it in LoadGlobal)
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

/// Pre-declared runtime helper function IDs
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
    #[allow(dead_code)]
    pub(crate) is_truthy: FuncId,
    pub(crate) make_cell: FuncId,
    pub(crate) load_cell: FuncId,
    pub(crate) load_capture: FuncId,
    pub(crate) store_cell: FuncId,
    pub(crate) store_capture: FuncId,
    pub(crate) load_global: FuncId,
    pub(crate) store_global: FuncId,
    pub(crate) call: FuncId,
    pub(crate) tail_call: FuncId,
    pub(crate) has_exception: FuncId,
    pub(crate) resolve_tail_call: FuncId,
    pub(crate) call_depth_enter: FuncId,
    pub(crate) call_depth_exit: FuncId,
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

        // Register runtime helper symbols (arithmetic, comparison, type checking)
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

        // Register dispatch helper symbols (data structures, cells, globals, calls)
        builder.symbol("elle_jit_cons", dispatch::elle_jit_cons as *const u8);
        builder.symbol("elle_jit_car", dispatch::elle_jit_car as *const u8);
        builder.symbol("elle_jit_cdr", dispatch::elle_jit_cdr as *const u8);
        builder.symbol(
            "elle_jit_make_array",
            dispatch::elle_jit_make_array as *const u8,
        );
        builder.symbol("elle_jit_is_pair", dispatch::elle_jit_is_pair as *const u8);
        builder.symbol(
            "elle_jit_make_cell",
            dispatch::elle_jit_make_cell as *const u8,
        );
        builder.symbol(
            "elle_jit_load_cell",
            dispatch::elle_jit_load_cell as *const u8,
        );
        builder.symbol(
            "elle_jit_load_capture",
            dispatch::elle_jit_load_capture as *const u8,
        );
        builder.symbol(
            "elle_jit_store_cell",
            dispatch::elle_jit_store_cell as *const u8,
        );
        builder.symbol(
            "elle_jit_store_capture",
            dispatch::elle_jit_store_capture as *const u8,
        );
        builder.symbol(
            "elle_jit_load_global",
            dispatch::elle_jit_load_global as *const u8,
        );
        builder.symbol(
            "elle_jit_store_global",
            dispatch::elle_jit_store_global as *const u8,
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

        let mut module = JITModule::new(builder);

        // Declare runtime helper functions
        let helpers = Self::declare_helpers(&mut module)?;

        Ok(JitCompiler { module, helpers })
    }

    /// Build the standard JIT function signature.
    /// fn(env: *const Value, args: *const Value, nargs: u32, vm: *mut VM, self_bits: u64) -> Value
    fn make_jit_signature(&self) -> Signature {
        let mut sig = self.module.make_signature();
        sig.call_conv = CallConv::SystemV;
        sig.params.push(AbiParam::new(I64)); // env pointer
        sig.params.push(AbiParam::new(I64)); // args pointer
        sig.params.push(AbiParam::new(I64)); // nargs
        sig.params.push(AbiParam::new(I64)); // vm pointer
        sig.params.push(AbiParam::new(I64)); // self_bits
        sig.returns.push(AbiParam::new(I64)); // return value
        sig
    }

    /// Declare runtime helper functions in the module
    fn declare_helpers(module: &mut JITModule) -> Result<RuntimeHelpers, JitError> {
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
            is_truthy: declare(module, "elle_jit_is_truthy", &unary_sig)?,
            make_cell: declare(module, "elle_jit_make_cell", &unary_sig)?,
            load_cell: declare(module, "elle_jit_load_cell", &unary_sig)?,
            load_capture: declare(module, "elle_jit_load_capture", &unary_sig)?,
            store_cell: declare(module, "elle_jit_store_cell", &binary_sig)?,
            store_capture: declare(module, "elle_jit_store_capture", &ternary_sig)?,
            load_global: declare(module, "elle_jit_load_global", &binary_sig)?,
            store_global: declare(module, "elle_jit_store_global", &ternary_sig)?,
            call: declare(module, "elle_jit_call", &call_sig)?,
            tail_call: declare(module, "elle_jit_tail_call", &call_sig)?,
            has_exception: declare(module, "elle_jit_has_exception", &unary_sig)?,
            resolve_tail_call: declare(module, "elle_jit_resolve_tail_call", &binary_sig)?,
            call_depth_enter: declare(module, "elle_jit_call_depth_enter", &unary_sig)?,
            call_depth_exit: declare(module, "elle_jit_call_depth_exit", &unary_sig)?,
        })
    }

    /// Compile a LirFunction to native code
    pub fn compile(
        mut self,
        lir: &LirFunction,
        self_sym: Option<SymbolId>,
    ) -> Result<JitCode, JitError> {
        // JIT can't handle suspension (yield/debug) — only non-suspending functions
        if lir.effect.may_suspend() {
            return Err(JitError::NotPure);
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

        // Wrap in JitCode (module is moved to keep code alive)
        Ok(JitCode::new(fn_ptr, self.module))
    }

    /// Build Cranelift IR for a LirFunction and return it as lines of text.
    /// Does NOT compile to native code — this is for diagnostic display only.
    pub fn clif_text(
        mut self,
        lir: &LirFunction,
        self_sym: Option<SymbolId>,
    ) -> Result<Vec<String>, JitError> {
        if lir.effect.may_suspend() {
            return Err(JitError::NotPure);
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
        // Validate all members are non-suspending
        for member in members {
            if member.lir.effect.may_suspend() {
                return Err(JitError::NotPure);
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

            self.translate_function(
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
    /// For self-tail-call optimization, we use this block structure:
    /// ```text
    /// entry_block:
    ///     // Extract function params (env, args, nargs, vm, self_bits)
    ///     // Load initial args into arg variables
    ///     // Jump to loop_header
    ///
    /// loop_header:
    ///     // Merge point for self-tail-calls
    ///     // Jump to first LIR block
    ///
    /// lir_block_0 (first LIR block):
    ///     // ... instructions ...
    ///     // TailCall: if self-call, update arg vars, jump to loop_header
    ///     //           if not self-call, call elle_jit_tail_call, return
    /// ```
    ///
    /// When `scc_peers` is provided, calls to functions in the peer map are
    /// emitted as direct Cranelift calls instead of going through `elle_jit_call`.
    fn translate_function(
        &mut self,
        lir: &LirFunction,
        func: &mut Function,
        scc_peers: Option<&HashMap<SymbolId, FuncId>>,
        self_sym: Option<SymbolId>,
    ) -> Result<(), JitError> {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(func, &mut builder_ctx);

        // Create translator context
        let mut translator = FunctionTranslator::new(&mut self.module, &self.helpers, lir);

        // Set self_sym for self-call detection in emit_direct_scc_call
        translator.self_sym = self_sym;

        // Set up SCC peer map for direct calls between mutually recursive functions
        if let Some(peers) = scc_peers {
            translator.scc_peers = peers.clone();
            // Build global_load_map: scan all blocks for LoadGlobal instructions
            // to map Reg -> SymbolId. Since LIR is SSA, each Reg is assigned once.
            for bb in &lir.blocks {
                for spanned in &bb.instructions {
                    if let LirInstr::LoadGlobal { dst, sym } = &spanned.instr {
                        translator.global_load_map.insert(*dst, *sym);
                    }
                }
            }
        }

        // Declare variables for all registers, local slots, arg variables, and locally-defined variables
        // - Registers: 0..num_regs (used by LIR instructions)
        // - Local slots: 0..num_locals (used by LoadLocal/StoreLocal)
        // - Arg variables: num_regs..num_regs+arity (used for self-tail-call)
        // - Locally-defined variables: num_regs+arity..num_regs+arity+num_locally_defined
        //   (used for let-bindings inside the function body)
        // All use the same Cranelift variable namespace, so declare enough for all
        let arg_var_base = lir.num_regs;
        let arity_params = lir.arity.fixed_params() as u16;
        let num_locally_defined = lir.num_locals.saturating_sub(arity_params) as u32;
        let local_var_base = arg_var_base + arity_params as u32;
        let max_var = std::cmp::max(
            std::cmp::max(lir.num_regs, lir.num_locals as u32),
            local_var_base + num_locally_defined,
        );
        for i in 0..max_var {
            builder.declare_var(var(i), I64);
        }
        translator.arg_var_base = arg_var_base;
        translator.local_var_base = local_var_base;

        // Create blocks: entry, loop_header, and LIR blocks
        let entry_block = builder.create_block();
        let loop_header = builder.create_block();

        let mut block_map: HashMap<Label, cranelift_codegen::ir::Block> = HashMap::new();
        for bb in &lir.blocks {
            let cl_block = builder.create_block();
            block_map.insert(bb.label, cl_block);
        }

        // Entry block: extract params, load initial args into variables
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block); // No predecessors

        let env_ptr = builder.block_params(entry_block)[0];
        let args_ptr = builder.block_params(entry_block)[1];
        let _nargs = builder.block_params(entry_block)[2];
        let vm_ptr = builder.block_params(entry_block)[3];
        let self_bits = builder.block_params(entry_block)[4];

        translator.env_ptr = Some(env_ptr);
        translator.vm_ptr = Some(vm_ptr);
        translator.self_bits = Some(self_bits);

        // Load initial args into arg variables
        for i in 0..arity_params as u32 {
            let offset = (i as i32) * 8;
            let addr = builder.ins().iadd_imm(args_ptr, offset as i64);
            let val = builder.ins().load(I64, MemFlags::trusted(), addr, 0);
            builder.def_var(var(arg_var_base + i), val);
        }

        // Initialize locally-defined variables to LocalCell(NIL)
        // These are let-bindings inside the function body
        if num_locally_defined > 0 {
            translator.init_locally_defined_vars(&mut builder, num_locally_defined)?;
        }

        builder.ins().jump(loop_header, &[]);

        // Loop header: merge point for self-tail-calls
        // DON'T seal yet — self-tail-calls will add back-edges
        builder.switch_to_block(loop_header);
        let first_lir_block = block_map[&lir.entry];
        builder.ins().jump(first_lir_block, &[]);

        // Store loop_header for TailCall to jump to
        translator.loop_header = Some(loop_header);

        // Translate LIR blocks
        for bb in &lir.blocks {
            let cl_block = block_map[&bb.label];
            builder.switch_to_block(cl_block);
            builder.seal_block(cl_block);

            // Translate instructions
            let mut block_terminated = false;
            for spanned in &bb.instructions {
                if translator.translate_instr(&mut builder, &spanned.instr, &block_map)? {
                    // Instruction emitted a terminator (e.g., TailCall)
                    block_terminated = true;
                    break;
                }
            }

            // Translate terminator (unless already terminated by an instruction)
            if !block_terminated {
                translator.translate_terminator(
                    &mut builder,
                    &bb.terminator.terminator,
                    &block_map,
                )?;
            }
        }

        // NOW seal loop_header — all self-tail-call back-edges have been emitted
        builder.seal_block(loop_header);

        builder.finalize();
        Ok(())
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
    use crate::effects::Effect;
    use crate::lir::{
        BasicBlock, BinOp, LirInstr, Reg, SpannedInstr, SpannedTerminator, Terminator,
    };
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
        func.effect = Effect::none();

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
        func.effect = Effect::none();

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

        // Call the compiled function
        // self_bits = 0 since we're not testing self-tail-calls here
        let args = [crate::value::Value::int(42).to_bits()];
        let result =
            unsafe { code.call(std::ptr::null(), args.as_ptr(), 1, std::ptr::null_mut(), 0) };
        let value = unsafe { crate::value::Value::from_bits(result) };
        assert_eq!(value.as_int(), Some(42));
    }

    #[test]
    fn test_compile_add() {
        let lir = make_add_lir();
        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let code = compiler.compile(&lir, None).expect("Failed to compile");

        // Call the compiled function
        // self_bits = 0 since we're not testing self-tail-calls here
        let args = [
            crate::value::Value::int(10).to_bits(),
            crate::value::Value::int(32).to_bits(),
        ];
        let result =
            unsafe { code.call(std::ptr::null(), args.as_ptr(), 2, std::ptr::null_mut(), 0) };
        let value = unsafe { crate::value::Value::from_bits(result) };
        assert_eq!(value.as_int(), Some(42));
    }

    #[test]
    fn test_reject_non_pure() {
        let mut lir = make_simple_lir();
        lir.effect = Effect::yields();

        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let result = compiler.compile(&lir, None);
        assert!(matches!(result, Err(JitError::NotPure)));
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

        let args = [crate::value::Value::int(42).to_bits()];
        let result =
            unsafe { code.call(std::ptr::null(), args.as_ptr(), 1, std::ptr::null_mut(), 0) };
        let value = unsafe { crate::value::Value::from_bits(result) };
        assert_eq!(value.as_int(), Some(42));
    }

    #[test]
    fn test_compile_batch_mutual_calls() {
        // Two functions that call each other via LoadGlobal + Call.
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
        f.effect = Effect::none();

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
            LirInstr::LoadGlobal {
                dst: Reg(5),
                sym: sym_g,
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
        g.effect = Effect::none();

        let mut gb0 = BasicBlock::new(Label(0));
        gb0.instructions.push(SpannedInstr::new(
            LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            },
            Span::synthetic(),
        ));
        gb0.instructions.push(SpannedInstr::new(
            LirInstr::LoadGlobal {
                dst: Reg(1),
                sym: sym_f,
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
    fn test_compile_batch_rejects_suspending() {
        let mut lir = make_simple_lir();
        lir.effect = Effect::yields();

        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let members = vec![BatchMember {
            sym: SymbolId(0),
            lir: &lir,
        }];
        let result = compiler.compile_batch(&members);
        assert!(matches!(result, Err(JitError::NotPure)));
    }
}
