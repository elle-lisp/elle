//! JIT compiler: LirFunction -> Cranelift IR -> Native code
//!
//! This module translates LIR (Low-level Intermediate Representation) to
//! Cranelift IR, then compiles to native x86_64 code.

use std::collections::HashMap;

use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::{AbiParam, Function, InstBuilder, MemFlags, Signature, UserFuncName};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

use crate::lir::{Label, LirFunction};

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
    pub(crate) make_vector: FuncId,
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
            "elle_jit_make_vector",
            dispatch::elle_jit_make_vector as *const u8,
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

        let mut module = JITModule::new(builder);

        // Declare runtime helper functions
        let helpers = Self::declare_helpers(&mut module)?;

        Ok(JitCompiler { module, helpers })
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

        // Make vector signature: (ptr, count) -> i64
        let mut make_vector_sig = module.make_signature();
        make_vector_sig.params.push(AbiParam::new(I64)); // elements ptr
        make_vector_sig.params.push(AbiParam::new(I64)); // count (as i64)
        make_vector_sig.returns.push(AbiParam::new(I64));

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
            make_vector: declare(module, "elle_jit_make_vector", &make_vector_sig)?,
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
        })
    }

    /// Compile a LirFunction to native code
    pub fn compile(mut self, lir: &LirFunction) -> Result<JitCode, JitError> {
        // Check that function is pure
        if !lir.effect.is_pure() {
            return Err(JitError::NotPure);
        }

        // Create function signature
        // fn(env: *const Value, args: *const Value, nargs: u32, vm: *mut VM, self_bits: u64) -> Value
        let mut sig = self.module.make_signature();
        sig.call_conv = CallConv::SystemV;
        sig.params.push(AbiParam::new(I64)); // env pointer
        sig.params.push(AbiParam::new(I64)); // args pointer
        sig.params.push(AbiParam::new(I64)); // nargs (as i64 for simplicity)
        sig.params.push(AbiParam::new(I64)); // vm pointer
        sig.params.push(AbiParam::new(I64)); // self_bits (closure identity for self-tail-call detection)
        sig.returns.push(AbiParam::new(I64)); // return value

        // Declare the function
        let func_name = lir.name.as_deref().unwrap_or("jit_func");
        let func_id = self
            .module
            .declare_function(func_name, Linkage::Local, &sig)
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        // Create function context
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(0, func_id.as_u32());

        // Translate LIR to Cranelift IR
        self.translate_function(lir, &mut ctx.func)?;

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
    pub fn clif_text(mut self, lir: &LirFunction) -> Result<Vec<String>, JitError> {
        if !lir.effect.is_pure() {
            return Err(JitError::NotPure);
        }

        let mut sig = self.module.make_signature();
        sig.call_conv = CallConv::SystemV;
        sig.params.push(AbiParam::new(I64));
        sig.params.push(AbiParam::new(I64));
        sig.params.push(AbiParam::new(I64));
        sig.params.push(AbiParam::new(I64));
        sig.params.push(AbiParam::new(I64));
        sig.returns.push(AbiParam::new(I64));

        let func_name = lir.name.as_deref().unwrap_or("jit_func");
        let func_id = self
            .module
            .declare_function(func_name, Linkage::Local, &sig)
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(0, func_id.as_u32());

        self.translate_function(lir, &mut ctx.func)?;

        let text = format!("{}", ctx.func);
        Ok(text.lines().map(String::from).collect())
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
    fn translate_function(
        &mut self,
        lir: &LirFunction,
        func: &mut Function,
    ) -> Result<(), JitError> {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(func, &mut builder_ctx);

        // Create translator context
        let mut translator = FunctionTranslator::new(&mut self.module, &self.helpers, lir);

        // Declare variables for all registers, local slots, arg variables, and locally-defined variables
        // - Registers: 0..num_regs (used by LIR instructions)
        // - Local slots: 0..num_locals (used by LoadLocal/StoreLocal)
        // - Arg variables: num_regs..num_regs+arity (used for self-tail-call)
        // - Locally-defined variables: num_regs+arity..num_regs+arity+num_locally_defined
        //   (used for let-bindings inside the function body)
        // All use the same Cranelift variable namespace, so declare enough for all
        let arg_var_base = lir.num_regs;
        let num_locally_defined = lir.num_locals.saturating_sub(lir.arity) as u32;
        let local_var_base = arg_var_base + lir.arity as u32;
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
        for i in 0..lir.arity as u32 {
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

    fn make_simple_lir() -> LirFunction {
        // Create a simple function that returns its first argument
        // fn(x) -> x
        // The LIR uses LoadCapture to access parameters.
        // With num_captures=0, LoadCapture index 0 loads from args[0].
        let mut func = LirFunction::new(1);
        func.num_regs = 1;
        func.num_captures = 0;
        func.effect = Effect::pure();

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
        let mut func = LirFunction::new(2);
        func.num_regs = 3;
        func.num_captures = 0;
        func.effect = Effect::pure();

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
        let code = compiler.compile(&lir).expect("Failed to compile");

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
        let code = compiler.compile(&lir).expect("Failed to compile");

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
        let result = compiler.compile(&lir);
        assert!(matches!(result, Err(JitError::NotPure)));
    }
}
